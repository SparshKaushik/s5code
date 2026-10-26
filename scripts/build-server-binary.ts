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
import { Command, Flag } from "effect/cli";
import { ChildProcess, ChildProcessSpawner } from "effect/process";
import * as NodeModule from "node:module";

import * as HostProcess from "@t3tools/shared/HostProcess";

import serverPackageJson from "../apps/server/package.json" with { type: "json" };
import { patchStandaloneCursorSdk } from "./lib/standalone-cursor-sdk.ts";

/**
 * Builds a standalone Bun server binary for Linux x64/arm64 from `apps/server/dist/bin.mjs`.
 * It embeds libfff_c, Cursor's platform package, and Playwright as assets, but not the web client.
 * Usage: node scripts/build-server-binary.ts --arch x64 --output-dir release
 */

const encodeJsonString = Schema.encodeEffect(Schema.fromJsonString(Schema.Unknown));

const archToTarget = {
  x64: "bun-linux-x64",
  arm64: "bun-linux-arm64",
} as const;

/** Fallback when neither --release-repo nor GITHUB_REPOSITORY is set. */
const DEFAULT_RELEASE_REPO = "SparshKaushik/s5code";

const ServerBinaryEnvConfig = Config.all({
  arch: Config.String("T3CODE_SERVER_ARCH").pipe(Config.option),
  outputDir: Config.String("T3CODE_SERVER_OUTPUT_DIR").pipe(Config.option),
  buildVersion: Config.String("T3CODE_SERVER_VERSION").pipe(Config.option),
  skipBuild: Config.Boolean("T3CODE_SERVER_SKIP_BUILD").pipe(Config.option),
  verbose: Config.Boolean("T3CODE_SERVER_VERBOSE").pipe(Config.option),
  /** owner/repo the built binary checks for self-update releases. */
  releaseRepo: Config.String("T3CODE_SERVER_BINARY_REPO").pipe(Config.option),
  githubRepository: Config.String("GITHUB_REPOSITORY").pipe(Config.option),
});

const RepoRoot = Effect.service(Path.Path).pipe(
  Effect.flatMap((path) => path.fromFileUrl(new URL("..", import.meta.url))),
);

const resolveBooleanFlag = (flag: Option.Option<boolean>, envValue: boolean) =>
  Option.getOrElse(flag, () => envValue);

const mergeOptions = <A>(a: Option.Option<A>, b: Option.Option<A>, defaultValue: A) =>
  Option.getOrElse(a, () => Option.getOrElse(b, () => defaultValue));

class ServerBinaryBuildError extends Schema.TaggedError<ServerBinaryBuildError>()(
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

class ServerBinaryBuildCommandError extends Schema.TaggedError<ServerBinaryBuildCommandError>()(
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
 * Resolves the target's `libfff_c` from the install location: fff-node's exports
 * name neither package.json nor CJS main. Cross-compiled binaries need the target's
 * library, and a missing one fails the build rather than shipping an empty file tree.
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
      NodeModule.createRequire(path.join(packageDirectory, "package.json")).resolve(
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

const resolveCursorPlatformDirectory = Effect.fn("resolveCursorPlatformDirectory")(function* (
  repoRoot: string,
  arch: keyof typeof archToTarget,
) {
  const fs = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const sdkDirectory = yield* fs.realPath(
    path.join(repoRoot, "apps/server/node_modules/@cursor/sdk"),
  );
  const packageName = `@cursor/sdk-linux-${arch}`;
  const manifestPath = yield* Effect.try({
    try: () =>
      NodeModule.createRequire(path.join(sdkDirectory, "package.json")).resolve(
        `${packageName}/package.json`,
      ),
    catch: (cause) =>
      new ServerBinaryBuildError({
        kind: "missing-cursor-native-assets",
        detail: `Could not resolve ${packageName}; install the target's optional dependencies before building. (${String(cause)})`,
      }),
  });
  const directory = path.dirname(manifestPath);
  for (const name of [
    "bin/rg",
    "bin/cursorsandbox",
    "vendor/tree-sitter/index.js",
    "vendor/tree-sitter/binding.node",
    "vendor/tree-sitter-bash/index.js",
    "vendor/tree-sitter-bash/binding.node",
  ]) {
    if (!(yield* fs.exists(path.join(directory, name)))) {
      return yield* new ServerBinaryBuildError({
        kind: "missing-cursor-native-assets",
        detail: `${packageName} is missing ${name}.`,
      });
    }
  }
  return directory;
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

  const scratch = yield* fs.makeTempDirectoryScoped({ prefix: "s5code-server-build-" });
  const cursorDirectory = yield* resolveCursorPlatformDirectory(repoRoot, options.arch);
  const cursorArchive = path.join(scratch, `cursor-sdk-native-linux-${options.arch}.tgz`);
  yield* runCommand(
    "archive Cursor native helpers",
    "tar",
    ["-czf", cursorArchive, "-C", cursorDirectory, "bin", "vendor", "package.json"],
    { verbose: options.verbose },
  );

  const playwrightManifest = yield* Effect.try({
    try: () =>
      NodeModule.createRequire(path.join(repoRoot, "apps/server/package.json")).resolve(
        "playwright-core/package.json",
      ),
    catch: (cause) =>
      new ServerBinaryBuildError({
        kind: "missing-playwright-runtime",
        detail: `Could not resolve playwright-core for standalone packaging. (${String(cause)})`,
      }),
  });
  const playwrightArchive = path.join(scratch, "playwright-core-runtime.tgz");
  yield* runCommand(
    "archive Playwright runtime",
    "tar",
    [
      "-czf",
      playwrightArchive,
      "--exclude=node_modules",
      "-C",
      path.dirname(playwrightManifest),
      ".",
    ],
    { verbose: options.verbose },
  );

  // --splitting keeps `node:sqlite` in a lazy chunk; hoisted, it kills Bun at startup.
  // --bytecode cannot combine with splitting. The defines mark a self-updating
  // release; the loader plugin is scoped to Cursor's flattened entry.
  const compileScript = `
    import { dirname } from "node:path";
    ${patchStandaloneCursorSdk.toString()}
    let cursorPatched = false;
    const result = await Bun.build({
      entrypoints: ${yield* encodeJsonString([entryPath, fffLibraryPath, cursorArchive, playwrightArchive])},
      compile: ${yield* encodeJsonString({ target: options.target, outfile })},
      splitting: true,
      minify: true,
      define: ${yield* encodeJsonString({
        "process.env.T3CODE_SERVER_BINARY_TARGET": yield* encodeJsonString(`linux-${options.arch}`),
        "process.env.T3CODE_SERVER_BINARY_REPO": yield* encodeJsonString(options.releaseRepo),
      })},
      plugins: [{
        name: "standalone-cursor-native-helpers",
        setup(build) {
          build.onLoad({ filter: /[/\\\\]@cursor[/\\\\]sdk[/\\\\]dist[/\\\\]bundled[/\\\\]index\\.js$/ }, async ({ path }) => {
            cursorPatched = true;
            return {
              contents: patchStandaloneCursorSdk(await Bun.file(path).text()),
              loader: "js",
              resolveDir: dirname(path),
            };
          });
        },
      }],
    });
    if (!result.success) throw new AggregateError(result.logs, "Standalone compilation failed");
    if (!cursorPatched) throw new Error("Cursor SDK was not included in the standalone binary");
  `;

  yield* Effect.log(
    `[server-binary] Compiling ${options.target} -> ${outfile} (version ${options.version}, releases from ${options.releaseRepo})...`,
  );
  yield* runCommand("Bun standalone compile", "bun", ["--eval", compileScript], {
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

  // Bun inlines only literal `process.env.<NAME>` reads. Assert the names are gone;
  // searching for values is useless because Bun's runtime contains strings like them.
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
  if (!containsBytes(compiled, "cursor-sdk-native-")) {
    return yield* new ServerBinaryBuildError({
      kind: "cursor-native-assets-not-embedded",
      detail: `Cursor's native-helper archive did not end up embedded in ${outfile}.`,
    });
  }
  if (!containsBytes(compiled, "playwright-core-runtime-")) {
    return yield* new ServerBinaryBuildError({
      kind: "playwright-runtime-not-embedded",
      detail: `Playwright's runtime archive did not end up embedded in ${outfile}.`,
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
  const hostPlatform = yield* HostProcess.Platform;
  const hostArch = yield* HostProcess.Architecture;
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
  arch: Flag.Literals("arch", ["x64", "arm64"]).pipe(
    Flag.withDescription("Target Linux arch: x64 or arm64 (env: T3CODE_SERVER_ARCH)."),
    Flag.optional,
  ),
  outputDir: Flag.String("output-dir").pipe(
    Flag.withDescription("Output directory for the binary (env: T3CODE_SERVER_OUTPUT_DIR)."),
    Flag.optional,
  ),
  buildVersion: Flag.String("build-version").pipe(
    Flag.withDescription("Version string baked into the binary name (env: T3CODE_SERVER_VERSION)."),
    Flag.optional,
  ),
  releaseRepo: Flag.String("release-repo").pipe(
    Flag.withDescription(
      "owner/repo the binary checks for self-update releases (env: T3CODE_SERVER_BINARY_REPO, GITHUB_REPOSITORY).",
    ),
    Flag.optional,
  ),
  skipBuild: Flag.Boolean("skip-build").pipe(
    Flag.withDescription(
      "Skip `vp run --filter t3 build` and use existing apps/server/dist artifacts (env: T3CODE_SERVER_SKIP_BUILD).",
    ),
    Flag.optional,
  ),
  verbose: Flag.Boolean("verbose").pipe(
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
