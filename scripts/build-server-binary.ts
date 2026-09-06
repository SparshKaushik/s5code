#!/usr/bin/env node

import * as NodeRuntime from "@effect/platform-node/NodeRuntime";
import * as NodeServices from "@effect/platform-node/NodeServices";
import * as Config from "effect/Config";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Logger from "effect/Logger";
import * as Option from "effect/Option";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";
import * as Stream from "effect/Stream";
import { Command, Flag } from "effect/unstable/cli";
import { ChildProcess, ChildProcessSpawner } from "effect/unstable/process";
import { createRequire } from "node:module";

import { HostProcessArchitecture, HostProcessPlatform } from "@t3tools/shared/hostProcess";

import serverPackageJson from "../apps/server/package.json" with { type: "json" };

/**
 * Build a precompiled, standalone server binary for headless deployments.
 *
 * The server is Bun-first (server.ts gates on `typeof Bun` to pick
 * BunHttpServer / BunServices / BunPtyAdapter, and persistence uses bun:sqlite),
 * so `bun build --compile` produces a single self-contained executable with the
 * runtime embedded — no Node, pnpm, `vp`, or `node_modules` needed on the host.
 *
 * We compile the already-bundled `apps/server/dist/bin.mjs` (`vp pack` with
 * `onlyBundle: false` inlines every importable dependency).
 *
 * Two flags are load-bearing, both verified empirically against bun 1.3.13:
 *
 *   `--splitting` is required, not an optimization. Without it the bundler
 *   flattens `Sqlite.ts`'s runtime-gated `import("../NodeSqliteClient.ts")`
 *   into the entry chunk, which hoists its top-level `import "node:sqlite"` to
 *   the top of the program. Bun does not implement `node:sqlite`, so the binary
 *   dies at startup with `No such built-in module: node:sqlite` before the
 *   `process.versions.bun` check can ever pick `bun:sqlite`. With `--splitting`
 *   the Node driver stays in its own lazily-loaded chunk and is never touched.
 *
 *   `--bytecode` cannot be combined with `--splitting` (bun fails with
 *   "Failed to generate bytecode"), so it is deliberately not used.
 *
 * The fff native library IS embedded, as a bun asset. It has to be: every file
 * listing and path search dlopens `libfff_c`, and `@ff-labs/fff-node` locates it
 * by resolving its own platform package out of `node_modules` relative to its own
 * `import.meta.url` — which inside a compiled binary is `/$bunfs/root/...`, with
 * no `node_modules` above it and none on the host. Without the asset the server
 * boots, serves threads and terminals, and shows an empty file tree on every
 * client, with the reason only in its log. See
 * apps/server/src/workspace/FffNativeLibrary.ts for the extraction at startup.
 *
 * The web client is NOT embedded. Bun's `--asset` flag (the only way to embed a
 * directory tree at a stable path) ships in Bun 1.4; on 1.3.x it is silently
 * ignored as an unknown flag, producing a binary that looks correct and serves
 * nothing. This binary therefore runs API + WebSocket only: connect to it from
 * app.s5code.touchtech.club, the desktop app, or mobile.
 *
 * Only the Linux server targets are configured here (the scenario this fills):
 *   - bun-linux-x64
 *   - bun-linux-arm64
 *
 * Usage:
 *   node scripts/build-server-binary.ts --arch x64 --output-dir release
 */

const ServerBinaryArch = Schema.Literals(["x64", "arm64"]);

const archToTarget = {
  x64: "bun-linux-x64",
  arm64: "bun-linux-arm64",
} as const;

/** Fallback when neither --release-repo nor GITHUB_REPOSITORY is set. */
const DEFAULT_RELEASE_REPO = "SparshKaushik/s5code";

const ServerBinaryEnvConfig = Config.all({
  arch: Config.string("T3CODE_SERVER_ARCH").pipe(Config.option),
  outputDir: Config.string("T3CODE_SERVER_OUTPUT_DIR").pipe(Config.option),
  buildVersion: Config.string("T3CODE_SERVER_VERSION").pipe(Config.option),
  skipBuild: Config.boolean("T3CODE_SERVER_SKIP_BUILD").pipe(Config.option),
  verbose: Config.boolean("T3CODE_SERVER_VERBOSE").pipe(Config.option),
  /** owner/repo the built binary checks for self-update releases. */
  releaseRepo: Config.string("T3CODE_SERVER_BINARY_REPO").pipe(Config.option),
  githubRepository: Config.string("GITHUB_REPOSITORY").pipe(Config.option),
});

const RepoRoot = Effect.service(Path.Path).pipe(
  Effect.flatMap((path) => path.fromFileUrl(new URL("..", import.meta.url))),
);

const resolveBooleanFlag = (flag: Option.Option<boolean>, envValue: boolean) =>
  Option.getOrElse(flag, () => envValue);

const mergeOptions = <A>(a: Option.Option<A>, b: Option.Option<A>, defaultValue: A) =>
  Option.getOrElse(a, () => Option.getOrElse(b, () => defaultValue));

class ServerBinaryBuildError extends Schema.TaggedErrorClass<ServerBinaryBuildError>()(
  "ServerBinaryBuildError",
  {
    kind: Schema.String,
    detail: Schema.String,
  },
) {
  override get message(): string {
    return this.detail;
  }
}

class ServerBinaryBuildCommandError extends Schema.TaggedErrorClass<ServerBinaryBuildCommandError>()(
  "ServerBinaryBuildCommandError",
  {
    command: Schema.String,
    exitCode: Schema.Int,
    stdoutTail: Schema.optionalKey(Schema.String),
    stderrTail: Schema.optionalKey(Schema.String),
  },
) {
  override get message(): string {
    const sections = [
      `Command: ${this.command}`,
      formatOutputSection("stdout", this.stdoutTail ?? ""),
      formatOutputSection("stderr", this.stderrTail ?? ""),
    ].filter((section): section is string => section !== undefined);
    return `Command exited with non-zero exit code (${this.exitCode})\n\n${sections.join("\n\n")}`;
  }
}

const COMMAND_OUTPUT_TAIL_LENGTH = 20_000;

function appendOutputTail(acc: string, chunk: string): string {
  const next = acc + chunk;
  return next.length > COMMAND_OUTPUT_TAIL_LENGTH ? next.slice(-COMMAND_OUTPUT_TAIL_LENGTH) : next;
}

function formatOutputSection(label: string, output: string): string | undefined {
  const trimmed = output.trim();
  return trimmed ? `${label} tail:\n${trimmed}` : undefined;
}

/** Mirrors build-desktop-artifact.ts: collect for diagnostics, echo when verbose. */
const collectCommandStream = <E>(
  stream: Stream.Stream<Uint8Array, E>,
  output: NodeJS.WriteStream,
  verbose: boolean,
): Effect.Effect<string, E> =>
  stream.pipe(
    Stream.decodeText(),
    Stream.runFoldEffect(
      () => "",
      (acc, chunk) =>
        Effect.as(
          verbose ? Effect.sync(() => output.write(chunk)) : Effect.void,
          appendOutputTail(acc, chunk),
        ),
    ),
  );

const runCommand = Effect.fn("runCommand")(function* (
  label: string,
  command: string,
  args: ReadonlyArray<string>,
  options: { readonly verbose: boolean; readonly cwd?: string },
) {
  const spawner = yield* ChildProcessSpawner.ChildProcessSpawner;
  const child = yield* spawner.spawn(
    ChildProcess.make(command, [...args], { cwd: options.cwd, shell: false }),
  );
  const [stdout, stderr, exitCode] = yield* Effect.all(
    [
      collectCommandStream(child.stdout, process.stdout, options.verbose),
      collectCommandStream(child.stderr, process.stderr, options.verbose),
      child.exitCode.pipe(Effect.map(Number)),
    ],
    { concurrency: "unbounded" },
  );

  if (exitCode !== 0) {
    return yield* new ServerBinaryBuildCommandError({
      command: label,
      exitCode,
      ...(stdout.trim() ? { stdoutTail: stdout } : {}),
      ...(stderr.trim() ? { stderrTail: stderr } : {}),
    });
  }
});

/**
 * Absolute path of the `libfff_c` shared library for one build target.
 *
 * Resolved from the install location rather than with `require.resolve`:
 * `@ff-labs/fff-node`'s exports map has no `./package.json` entry and no CJS
 * main, so neither `require.resolve` nor `import.meta.resolve` can name the
 * package directory. Its own optional platform dependencies resolve normally
 * once we are standing in it.
 *
 * The *target's* library, not the host's: release CI cross-compiles, and an
 * arm64 binary carrying an x64 `.so` fails at dlopen with a message about file
 * format that points nowhere near the real cause.
 *
 * A failure here fails the build. A binary missing the library looks healthy and
 * has no file tree, which is the silent degradation this embed exists to end.
 */
const resolveFffLibraryPath = Effect.fn("resolveFffLibraryPath")(function* (
  repoRoot: string,
  arch: keyof typeof archToTarget,
): Effect.fn.Return<string, ServerBinaryBuildError, FileSystem.FileSystem | Path.Path> {
  const fs = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const packageName = `@ff-labs/fff-bin-linux-${arch}-gnu`;
  const missing = (detail: string) =>
    new ServerBinaryBuildError({ kind: "missing-fff-library", detail });

  const packageDirectory = yield* fs
    .realPath(path.join(repoRoot, "apps/server/node_modules/@ff-labs/fff-node"))
    .pipe(
      Effect.mapError((cause) =>
        missing(`@ff-labs/fff-node is not installed under apps/server. (${String(cause)})`),
      ),
    );

  const resolved = yield* Effect.try({
    try: () =>
      createRequire(path.join(packageDirectory, "package.json")).resolve(
        `${packageName}/package.json`,
      ),
    catch: (cause) =>
      missing(
        `Could not resolve ${packageName}. pnpm only installs the platform binaries allowed by \`supportedArchitectures\` in pnpm-workspace.yaml, so \`vp i\` on a host that allows linux-${arch} is what provides it. (${String(cause)})`,
      ),
  });

  const libraryPath = path.join(path.dirname(resolved), "libfff_c.so");
  const exists = yield* fs.exists(libraryPath).pipe(Effect.orElseSucceed(() => false));
  if (!exists) {
    return yield* missing(`Resolved the ${packageName} package but ${libraryPath} does not exist.`);
  }
  return libraryPath;
});

const resolveBuildOptions = Effect.fn("resolveBuildOptions")(function* (input: {
  readonly arch: Option.Option<string>;
  readonly outputDir: Option.Option<string>;
  readonly buildVersion: Option.Option<string>;
  readonly releaseRepo: Option.Option<string>;
  readonly skipBuild: Option.Option<boolean>;
  readonly verbose: Option.Option<boolean>;
}) {
  const env = yield* ServerBinaryEnvConfig;
  const path = yield* Path.Path;
  const repoRoot = yield* RepoRoot;

  const arch = mergeOptions(input.arch, env.arch, "x64");
  if (!(arch in archToTarget)) {
    return yield* new ServerBinaryBuildError({
      kind: "unsupported-arch",
      detail: `Unsupported server binary arch "${arch}". Supported: ${Object.keys(archToTarget).join(", ")}.`,
    });
  }

  const outputDir = path.resolve(repoRoot, mergeOptions(input.outputDir, env.outputDir, "release"));

  return {
    arch: arch as keyof typeof archToTarget,
    target: archToTarget[arch as keyof typeof archToTarget],
    outputDir,
    version:
      mergeOptions(input.buildVersion, env.buildVersion, undefined) ?? serverPackageJson.version,
    releaseRepo: mergeOptions(
      input.releaseRepo,
      Option.orElse(env.releaseRepo, () => env.githubRepository),
      DEFAULT_RELEASE_REPO,
    ),
    skipBuild: resolveBooleanFlag(input.skipBuild, Option.getOrUndefined(env.skipBuild) ?? false),
    verbose: resolveBooleanFlag(input.verbose, Option.getOrUndefined(env.verbose) ?? false),
  };
});

const buildServerBinary = Effect.fn("buildServerBinary")(function* (options: {
  readonly arch: keyof typeof archToTarget;
  readonly target: string;
  readonly outputDir: string;
  readonly version: string;
  readonly releaseRepo: string;
  readonly skipBuild: boolean;
  readonly verbose: boolean;
}) {
  const path = yield* Path.Path;
  const fs = yield* FileSystem.FileSystem;
  const repoRoot = yield* RepoRoot;
  const serverDist = path.join(repoRoot, "apps/server/dist");
  const entryPath = path.join(serverDist, "bin.mjs");

  if (!options.skipBuild) {
    yield* Effect.log("[server-binary] Building server bundle...");
    yield* runCommand("vp run --filter t3 build", "vp", ["run", "--filter", "t3", "build"], {
      verbose: options.verbose,
      cwd: repoRoot,
    });
  }

  if (!(yield* fs.exists(entryPath))) {
    return yield* new ServerBinaryBuildError({
      kind: "missing-build-input",
      detail: `Server bundle missing at ${entryPath}. Run without --skip-build, or run \`vp run --filter t3 build\` first.`,
    });
  }

  const binaryName = `s5code-server-${options.version}-linux-${options.arch}`;
  const outfile = path.join(options.outputDir, binaryName);

  yield* fs.makeDirectory(options.outputDir, { recursive: true });

  const fffLibraryPath = yield* resolveFffLibraryPath(repoRoot, options.arch);

  // --splitting is required (see the module comment): it keeps the Node SQLite
  // driver in a lazily-loaded chunk so its top-level `node:sqlite` import is
  // never hoisted into the entry, which would kill the binary at startup.
  // --bytecode is intentionally absent: bun cannot generate bytecode with
  // --splitting. The two --define values are what let the running binary
  // recognize itself as a self-updating release artifact.
  const args = [
    "build",
    "--compile",
    "--splitting",
    "--target",
    options.target,
    "--outfile",
    outfile,
    `--define=process.env.T3CODE_SERVER_BINARY_TARGET="linux-${options.arch}"`,
    `--define=process.env.T3CODE_SERVER_BINARY_REPO="${options.releaseRepo}"`,
    "--minify",
    "./bin.mjs",
    // Extra entry points that are not JS become embedded assets, readable at
    // runtime through `Bun.embeddedFiles`.
    fffLibraryPath,
  ];

  yield* Effect.log(
    `[server-binary] Compiling ${options.target} -> ${outfile} (version ${options.version}, releases from ${options.releaseRepo})...`,
  );
  yield* runCommand("bun build --compile", "bun", args, {
    verbose: options.verbose,
    cwd: serverDist,
  });

  const stat = yield* fs.stat(outfile).pipe(Effect.orElseSucceed(() => null));
  if (!stat || stat.type !== "File") {
    return yield* new ServerBinaryBuildError({
      kind: "output-missing",
      detail: `Compilation succeeded but no binary was produced at ${outfile}.`,
    });
  }

  // Prove the --define values actually landed. Bun only rewrites literal
  // `process.env.<NAME>` member expressions, so a refactor that reads them
  // indirectly would compile fine and produce a binary that can never
  // recognize itself as updatable.
  //
  // The assertion is that the variable *names* are gone: a successful inline
  // replaces them with their values, leaving no trace. Searching for the values
  // instead would be useless, since bun's own runtime already contains strings
  // like "bun-linux-x64".
  const compiled = yield* fs.readFile(outfile);
  // The asset is only useful if it is actually in there. Bun names embedded
  // files `libfff_c-<hash>.so`, and the un-hashed stem survives in the manifest,
  // so its absence means the extra entry point was ignored rather than embedded.
  if (!containsBytes(compiled, "libfff_c")) {
    return yield* new ServerBinaryBuildError({
      kind: "fff-library-not-embedded",
      detail: `\`${fffLibraryPath}\` did not end up embedded in ${outfile}, so the server would run with no file tree. Check that this bun version treats non-JS entry points as assets.`,
    });
  }
  for (const name of ["T3CODE_SERVER_BINARY_TARGET", "T3CODE_SERVER_BINARY_REPO"] as const) {
    if (containsBytes(compiled, name)) {
      return yield* new ServerBinaryBuildError({
        kind: "define-not-inlined",
        detail: `\`${name}\` survived as a runtime lookup in ${outfile}, so \`bun build --define\` did not inline it. The reads in apps/server/src/cloud/binaryRuntime.ts must stay literal \`process.env.<NAME>\` expressions.`,
      });
    }
  }

  yield* Effect.log("[server-binary] Done.").pipe(Effect.annotateLogs({ artifact: outfile }));

  // Only runnable when the host can execute the target. Release CI therefore
  // builds each Linux arch on a matching runner: a cross-compiled arm64 binary
  // can omit @yuuang/ffi-rs-linux-arm64-gnu (pulled in by @ff-labs/fff-node) and
  // still look like a successful compile until --version dies at process start.
  const hostPlatform = yield* HostProcessPlatform;
  const hostArch = yield* HostProcessArchitecture;
  if (hostPlatform === "linux" && hostArch === options.arch) {
    yield* Effect.log(`[server-binary] Smoke-testing ${outfile} --version...`);
    yield* runCommand("server binary --version", outfile, ["--version"], {
      verbose: options.verbose,
    });
  }
});

/** Substring search over raw bytes, so a 100MB binary needs no string copy. */
function containsBytes(haystack: Uint8Array, needle: string): boolean {
  const bytes = new TextEncoder().encode(needle);
  if (bytes.length === 0 || haystack.length < bytes.length) return false;
  const first = bytes[0];
  outer: for (let index = 0; index <= haystack.length - bytes.length; index += 1) {
    if (haystack[index] !== first) continue;
    for (let offset = 1; offset < bytes.length; offset += 1) {
      if (haystack[index + offset] !== bytes[offset]) continue outer;
    }
    return true;
  }
  return false;
}

const buildServerBinaryCli = Command.make("build-server-binary", {
  arch: Flag.choice("arch", ServerBinaryArch.literals).pipe(
    Flag.withDescription("Target Linux arch: x64 or arm64 (env: T3CODE_SERVER_ARCH)."),
    Flag.optional,
  ),
  outputDir: Flag.string("output-dir").pipe(
    Flag.withDescription("Output directory for the binary (env: T3CODE_SERVER_OUTPUT_DIR)."),
    Flag.optional,
  ),
  buildVersion: Flag.string("build-version").pipe(
    Flag.withDescription("Version string baked into the binary name (env: T3CODE_SERVER_VERSION)."),
    Flag.optional,
  ),
  releaseRepo: Flag.string("release-repo").pipe(
    Flag.withDescription(
      "owner/repo the binary checks for self-update releases (env: T3CODE_SERVER_BINARY_REPO, GITHUB_REPOSITORY).",
    ),
    Flag.optional,
  ),
  skipBuild: Flag.boolean("skip-build").pipe(
    Flag.withDescription(
      "Skip `vp run --filter t3 build` and use existing apps/server/dist artifacts (env: T3CODE_SERVER_SKIP_BUILD).",
    ),
    Flag.optional,
  ),
  verbose: Flag.boolean("verbose").pipe(
    Flag.withDescription("Stream subprocess stdout (env: T3CODE_SERVER_VERBOSE)."),
    Flag.optional,
  ),
}).pipe(
  Command.withDescription("Build a precompiled standalone Linux server binary."),
  Command.withHandler((input) =>
    Effect.gen(function* () {
      const options = yield* resolveBuildOptions(input);
      yield* buildServerBinary(options);
    }),
  ),
);

const cliRuntimeLayer = Layer.mergeAll(Logger.layer([Logger.consolePretty()]), NodeServices.layer);

if (import.meta.main) {
  Command.run(buildServerBinaryCli, { version: "0.0.0" }).pipe(
    Effect.scoped,
    Effect.provide(cliRuntimeLayer),
    NodeRuntime.runMain,
  );
}
