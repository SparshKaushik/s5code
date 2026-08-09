import * as Context from "effect/Context";
import * as Option from "effect/Option";

/**
 * Identity of a precompiled single-file server binary (`bun build --compile`,
 * see scripts/build-server-binary.ts).
 *
 * Two values are inlined at compile time with `--define` so a running binary can
 * recognize itself and find its own replacement on GitHub Releases. They are
 * absent in every other way of running the server (`npx t3`, the desktop app,
 * a dev checkout), which is how the binary update path stays opt-in.
 */
export interface ServerBinaryIdentity {
  /** Release-asset platform suffix this binary was compiled for, e.g. "linux-x64". */
  readonly target: string;
  /** owner/repo whose releases hold the replacement binaries. */
  readonly repo: string;
  /** Absolute path of the currently running executable. */
  readonly executablePath: string;
}

const TARGET_PATTERN = /^linux-(?:x64|arm64)$/;
const REPO_PATTERN = /^[\w.-]+\/[\w.-]+$/;

const trimmed = (value: string | undefined): string | undefined => {
  const result = value?.trim();
  return result !== undefined && result.length > 0 ? result : undefined;
};

/**
 * Bun compiled binaries run their entry point out of the virtual `/$bunfs/`
 * filesystem. That is the load-bearing signal: `process.execPath` points at a
 * real path in every runtime, and `Bun.isStandaloneExecutable` only exists in
 * newer Bun releases than the one the build pins.
 */
export function isStandaloneBunExecutable(argv: ReadonlyArray<string>): boolean {
  const entry = argv[1];
  return entry !== undefined && entry.startsWith("/$bunfs/");
}

/**
 * The release-asset name for one version/target pair. Must stay in sync with
 * the name the build script writes, since the updater resolves assets by exact
 * name rather than by pattern.
 */
export function serverBinaryAssetName(version: string, target: string): string {
  return `s5code-server-${version}-${target}`;
}

/**
 * Resolves the identity of the running binary, or None when this process is not
 * a release binary. Both build-time values must be present and well-formed: a
 * half-configured build gets no update path rather than a broken one.
 */
export function resolveServerBinaryIdentity(input: {
  readonly argv: ReadonlyArray<string>;
  readonly executablePath: string;
  readonly target: string | undefined;
  readonly repo: string | undefined;
}): Option.Option<ServerBinaryIdentity> {
  if (!isStandaloneBunExecutable(input.argv)) return Option.none();

  const target = trimmed(input.target);
  const repo = trimmed(input.repo);
  if (
    target === undefined ||
    repo === undefined ||
    !TARGET_PATTERN.test(target) ||
    !REPO_PATTERN.test(repo)
  ) {
    return Option.none();
  }

  return Option.some({ target, repo, executablePath: input.executablePath });
}

/**
 * These two reads must stay written as literal `process.env.<NAME>` member
 * expressions at module scope. That is the only form `bun build --define`
 * rewrites: passing `process.env` around and reading a property off it leaves
 * the lookup dynamic, so the build-time values would silently never apply and
 * every binary would resolve to no identity.
 */
const BUILD_TARGET = process.env.T3CODE_SERVER_BINARY_TARGET;
const BUILD_REPO = process.env.T3CODE_SERVER_BINARY_REPO;

/**
 * Process-level fact, resolved from the ambient process by default and
 * overridable in tests with `Effect.provideService`.
 */
export class ServerBinaryRuntime extends Context.Reference<Option.Option<ServerBinaryIdentity>>(
  "t3/cloud/binaryRuntime/ServerBinaryRuntime",
  {
    defaultValue: () =>
      resolveServerBinaryIdentity({
        argv: process.argv,
        executablePath: process.execPath,
        target: BUILD_TARGET,
        repo: BUILD_REPO,
      }),
  },
) {}
