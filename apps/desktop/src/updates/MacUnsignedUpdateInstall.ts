import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";
import * as Stream from "effect/Stream";
import { ChildProcess } from "effect/unstable/process";
import * as ChildProcessSpawner from "effect/unstable/process/ChildProcessSpawner";

export class MacUnsignedUpdateInstallError extends Schema.TaggedErrorClass<MacUnsignedUpdateInstallError>()(
  "MacUnsignedUpdateInstallError",
  {
    operation: Schema.Literals(["extract", "replace"]),
    cause: Schema.Defect(),
  },
) {
  override get message(): string {
    return `Unsigned macOS update ${this.operation} failed.`;
  }
}

export function isSquirrelCompatibleMacSignature(codesignVerboseOutput: string): boolean {
  let signature = "";
  let teamIdentifier = "";
  for (const line of codesignVerboseOutput.split(/\r?\n/)) {
    if (line.startsWith("Signature=")) {
      signature = line.slice("Signature=".length).trim();
    }
    if (line.startsWith("TeamIdentifier=")) {
      teamIdentifier = line.slice("TeamIdentifier=".length).trim();
    }
  }
  if (signature === "adhoc") {
    return false;
  }
  return teamIdentifier !== "" && teamIdentifier !== "not set";
}

export function findMacAppBundleName(entries: ReadonlyArray<string>): string | null {
  const appBundles = entries.filter((entry) => entry.endsWith(".app"));
  return appBundles.length === 1 ? (appBundles[0] ?? null) : null;
}

export function resolveMacAppBundlePath(startPath: string): string | null {
  const appSuffixIndex = startPath.indexOf(".app/");
  if (appSuffixIndex >= 0) {
    return startPath.slice(0, appSuffixIndex + ".app".length);
  }
  return startPath.endsWith(".app") ? startPath : null;
}

const shellSingleQuote = (value: string): string => `'${value.replaceAll("'", "'\\''")}'`;

export function macBundleReplaceShellScript(input: {
  readonly pid: number;
  readonly sourceAppPath: string;
  readonly destAppPath: string;
  readonly cleanupPath: string;
}): string {
  const source = shellSingleQuote(input.sourceAppPath);
  const dest = shellSingleQuote(input.destAppPath);
  const cleanup = shellSingleQuote(input.cleanupPath);
  return `#!/bin/bash
set -eu
while kill -0 ${input.pid} 2>/dev/null; do
  sleep 0.2
done
backup=${dest}.update-backup
rm -rf "$backup"
if ! mv ${dest} "$backup"; then
  exit 1
fi
if ditto ${source} ${dest}; then
  xattr -cr ${dest} || true
  rm -rf "$backup" ${cleanup}
  open ${dest}
else
  rm -rf ${dest}
  mv "$backup" ${dest}
  rm -rf ${cleanup}
  open ${dest}
  exit 1
fi
`;
}

export class MacUnsignedUpdateInstall extends Context.Service<
  MacUnsignedUpdateInstall,
  {
    readonly usesSquirrelCompatibleSignature: (appPath: string) => Effect.Effect<boolean>;
    readonly installDownloadedZip: (input: {
      readonly downloadedZipPath: string;
      readonly appPath: string;
    }) => Effect.Effect<void, MacUnsignedUpdateInstallError>;
  }
>()("@t3tools/desktop/updates/MacUnsignedUpdateInstall") {}

const concatChunks = (chunks: Iterable<Uint8Array>): Uint8Array => {
  const arrays = Array.from(chunks);
  const total = arrays.reduce((sum, chunk) => sum + chunk.byteLength, 0);
  const bytes = new Uint8Array(total);
  let offset = 0;
  for (const chunk of arrays) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return bytes;
};

const decodeUtf8 = (bytes: Uint8Array): string => new TextDecoder().decode(bytes);

const make = Effect.gen(function* () {
  const fileSystem = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const spawner = yield* ChildProcessSpawner.ChildProcessSpawner;

  const usesSquirrelCompatibleSignature = Effect.fn(
    "macUnsignedUpdateInstall.usesSquirrelCompatibleSignature",
  )(function* (appPath: string) {
    const result = yield* Effect.scoped(
      Effect.gen(function* () {
        const handle = yield* spawner.spawn(
          ChildProcess.make("codesign", ["-dv", "--verbose=4", appPath], {
            stdin: "ignore",
            stdout: "pipe",
            stderr: "pipe",
          }),
        );
        const [stdoutBytes, stderrBytes] = yield* Effect.all(
          [Stream.runCollect(handle.stdout), Stream.runCollect(handle.stderr)],
          { concurrency: "unbounded" },
        );
        const exitCode = yield* handle.exitCode;
        return {
          exitCode,
          output: `${decodeUtf8(concatChunks(stdoutBytes))}\n${decodeUtf8(concatChunks(stderrBytes))}`,
        };
      }),
    ).pipe(Effect.option);
    return Option.match(result, {
      onNone: () => false,
      onSome: ({ exitCode, output }) => exitCode === 0 && isSquirrelCompatibleMacSignature(output),
    });
  });

  const installDownloadedZip = Effect.fn("macUnsignedUpdateInstall.installDownloadedZip")(
    function* (input: { readonly downloadedZipPath: string; readonly appPath: string }) {
      const extractDir = yield* fileSystem
        .makeTempDirectory({ prefix: "s5code-mac-update-" })
        .pipe(
          Effect.mapError(
            (cause) => new MacUnsignedUpdateInstallError({ operation: "extract", cause }),
          ),
        );
      const extractExitCode = yield* spawner
        .exitCode(
          ChildProcess.make("ditto", ["-x", "-k", input.downloadedZipPath, extractDir], {
            stdin: "ignore",
            stdout: "ignore",
            stderr: "ignore",
          }),
        )
        .pipe(
          Effect.mapError(
            (cause) => new MacUnsignedUpdateInstallError({ operation: "extract", cause }),
          ),
        );
      if (extractExitCode !== 0) {
        return yield* new MacUnsignedUpdateInstallError({
          operation: "extract",
          cause: new Error(`ditto exited with code ${String(extractExitCode)}`),
        });
      }
      const entries = yield* fileSystem
        .readDirectory(extractDir)
        .pipe(
          Effect.mapError(
            (cause) => new MacUnsignedUpdateInstallError({ operation: "extract", cause }),
          ),
        );
      const appName = findMacAppBundleName(entries);
      if (appName === null) {
        return yield* new MacUnsignedUpdateInstallError({
          operation: "extract",
          cause: new Error("Downloaded update zip must contain exactly one top-level .app bundle"),
        });
      }
      const sourceAppPath = path.join(extractDir, appName);
      const script = macBundleReplaceShellScript({
        pid: process.pid,
        sourceAppPath,
        destAppPath: input.appPath,
        cleanupPath: extractDir,
      });
      yield* spawner
        .spawn(
          ChildProcess.make("/bin/bash", ["-c", script], {
            detached: true,
            stdin: "ignore",
            stdout: "ignore",
            stderr: "ignore",
          }),
        )
        .pipe(
          Effect.flatMap((handle) => handle.unref),
          Effect.scoped,
          Effect.asVoid,
          Effect.mapError(
            (cause) => new MacUnsignedUpdateInstallError({ operation: "replace", cause }),
          ),
        );
    },
  );

  return MacUnsignedUpdateInstall.of({
    usesSquirrelCompatibleSignature,
    installDownloadedZip,
  });
});

export const layer = Layer.effect(MacUnsignedUpdateInstall, make);
