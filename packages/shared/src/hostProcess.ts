import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as NodeDns from "node:dns";
import * as NodeOS from "node:os";

// No static `node:sea` import: Bun does not implement it, and a static import
// would hoist onto the compiled server binary's startup graph and kill it
// before any runtime check runs. `getBuiltinModule` probes it lazily; runtimes
// without the builtin (Bun, bundled Graph) fall through to false.
const isNodeSea = (): boolean => {
  try {
    const sea = process.getBuiltinModule?.("node:sea") as { isSea?: () => boolean } | undefined;
    return sea?.isSea?.() === true;
  } catch {
    return false;
  }
};

export const HostProcessPlatform = Context.Reference<NodeJS.Platform>(
  "@t3tools/shared/hostProcess/HostProcessPlatform",
  {
    defaultValue: () => process.platform,
  },
);

export const HostProcessArchitecture = Context.Reference<NodeJS.Architecture>(
  "@t3tools/shared/hostProcess/HostProcessArchitecture",
  {
    defaultValue: () => process.arch,
  },
);

export const HostProcessHostname = Context.Reference<string>(
  "@t3tools/shared/hostProcess/HostProcessHostname",
  {
    defaultValue: () => NodeOS.hostname(),
  },
);

export const HostProcessEnvironment = Context.Reference<NodeJS.ProcessEnv>(
  "@t3tools/shared/hostProcess/HostProcessEnvironment",
  {
    defaultValue: () => process.env,
  },
);

export const HostProcessWorkingDirectory = Context.Reference<string>(
  "@t3tools/shared/hostProcess/HostProcessWorkingDirectory",
  {
    defaultValue: () => process.cwd(),
  },
);

export const HostProcessExecutablePath = Context.Reference<string>(
  "@t3tools/shared/hostProcess/HostProcessExecutablePath",
  {
    defaultValue: () => process.execPath,
  },
);

export const HostProcessArguments = Context.Reference<ReadonlyArray<string>>(
  "@t3tools/shared/hostProcess/HostProcessArguments",
  {
    defaultValue: () => process.argv,
  },
);

/**
 * The command the shell was given, before Node resolved it to the binary:
 * `t3` for a PATH lookup, `./t3` or the launcher symlink for an explicit
 * path. `process.argv[0]` and `execPath` are always the resolved binary.
 */
export const HostProcessInvokedAs = Context.Reference<string>(
  "@t3tools/shared/hostProcess/HostProcessInvokedAs",
  {
    defaultValue: () => process.argv0,
  },
);

/**
 * Whether this process is a self-contained executable — a Node SEA or a
 * bun-compiled binary — rather than a script run by a runtime on the machine.
 * Code that needs a sibling file or a Node to run one branches on this: an
 * executable hosts such things as hidden subcommands of itself.
 */
export const HostProcessIsExecutable = Context.Reference<boolean>(
  "@t3tools/shared/hostProcess/HostProcessIsExecutable",
  {
    // Node single-executables answer `node:sea`'s probe; our release binaries
    // are `bun build --compile` outputs whose entry lives under the virtual
    // `/$bunfs/` root.
    defaultValue: () => isNodeSea() || (process.argv[1]?.startsWith("/$bunfs/") ?? false),
  },
);

/**
 * Every IP address this machine answers to: the interface addresses, plus
 * whatever the resolver returns for the machine's own hostname. The latter
 * matters because a hostname can map to an address no interface carries —
 * Debian-style hosts put `127.0.1.1` in `/etc/hosts` — and a program that
 * records "its" address by resolving its hostname (Firefox's profile lock
 * does) will write that one. "Is this address ours" has to accept both.
 *
 * Best effort: a failed lookup just leaves the interface set.
 */
export const HostProcessAddresses = Context.Reference<Effect.Effect<ReadonlySet<string>>>(
  "@t3tools/shared/hostProcess/HostProcessAddresses",
  {
    defaultValue: () =>
      Effect.gen(function* () {
        const interfaces = Object.values(NodeOS.networkInterfaces())
          .flat()
          .flatMap((entry) => (entry ? [entry.address] : []));
        const resolved = yield* Effect.tryPromise(() =>
          NodeDns.promises.lookup(NodeOS.hostname(), { all: true }),
        ).pipe(
          Effect.map((entries) => entries.map((entry) => entry.address)),
          Effect.orElseSucceed(() => [] as ReadonlyArray<string>),
        );
        return new Set([...interfaces, ...resolved]);
      }),
  },
);

/** Undefined on platforms without POSIX uids (Windows). */
export const HostProcessUserId = Context.Reference<number | undefined>(
  "@t3tools/shared/hostProcess/HostProcessUserId",
  {
    defaultValue: () => process.getuid?.(),
  },
);

export const isHostWindows = Effect.map(HostProcessPlatform, (platform) => platform === "win32");
