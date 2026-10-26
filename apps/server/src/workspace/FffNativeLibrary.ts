/**
 * fff-node finds `libfff_c` through `node_modules`, which a `bun build --compile`
 * binary lacks, and the kernel cannot dlopen from `/$bunfs`. Extract the embedded
 * library to disk and point fff-node at it via the patched override env var.
 */
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import * as PlatformError from "effect/PlatformError";

import * as HostProcess from "@t3tools/shared/HostProcess";

/**
 * The env var `fff-node` reads first. Also honoured when set from outside, which
 * is the escape hatch if an operator has to point at a hand-built library.
 */
export const FFF_NATIVE_LIB_PATH_ENV = "T3CODE_FFF_NATIVE_LIB_PATH";

/**
 * One embedded asset, structurally. `Bun.embeddedFiles` is typed as `Blob[]` and
 * is `File[]` at runtime; this is the part of a `File` that matters here, and
 * taking it as a parameter is what makes the extraction testable off-bun.
 */
export interface FffLibraryAsset {
  /**
   * Bun rewrites an embedded asset's name to carry a content hash
   * (`libfff_c-fpsdhehf.so`), which is what makes the extracted copy safe to
   * cache by name: a rebuilt binary with a different library gets a different
   * filename rather than silently reusing the old one.
   */
  readonly name: string;
  readonly size: number;
  readonly arrayBuffer: () => Promise<ArrayBuffer>;
}

const isFffLibraryAsset = (name: string) =>
  name.startsWith("libfff_c") && (name.endsWith(".so") || name.endsWith(".dylib"));

/**
 * The embedded library, or undefined when this process is not a compiled binary
 * (or was compiled without the asset).
 *
 * `Bun.embeddedFiles` only exists under bun and is empty outside a compiled
 * executable, so the two cases collapse into one check.
 */
function embeddedFffLibrary(): FffLibraryAsset | undefined {
  // bun-types declares embeddedFiles as Blob[], but the runtime objects carry
  // the asset name that Blob's type omits.
  const files =
    typeof Bun === "undefined"
      ? undefined
      : (Bun.embeddedFiles as unknown as ReadonlyArray<FffLibraryAsset>);
  return files?.find((file) => isFffLibraryAsset(file.name));
}

/**
 * Writes the asset under `<baseDir>/caches/native` and returns its path, reusing
 * an intact previous extraction.
 *
 * Errors propagate; the caller decides how loud to be.
 */
export const extractFffNativeLibrary = Effect.fn("extractFffNativeLibrary")(function* (input: {
  readonly baseDir: string;
  readonly asset: FffLibraryAsset;
}): Effect.fn.Return<string, PlatformError.PlatformError, FileSystem.FileSystem | Path.Path> {
  const fileSystem = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;

  const directory = path.join(input.baseDir, "caches", "native");
  const target = path.join(directory, input.asset.name);

  // Size is the whole check: the name already carries bun's content hash, so a
  // full-size file under that name is this exact library. Comparing bytes would
  // read 5MB off disk on every start to learn nothing new.
  const existing = yield* fileSystem.stat(target).pipe(Effect.option);
  const intact =
    existing._tag === "Some" &&
    existing.value.type === "File" &&
    Number(existing.value.size) === input.asset.size;
  if (intact) return target;

  yield* fileSystem.makeDirectory(directory, { recursive: true });
  const bytes = yield* Effect.promise(() => input.asset.arrayBuffer());
  // Written under a pid-scoped name and renamed into place: two servers starting
  // at once must not have one dlopen the other's half-written file.
  const staging = `${target}.${process.pid}.partial`;
  yield* fileSystem.writeFile(staging, new Uint8Array(bytes));
  yield* fileSystem.chmod(staging, 0o755);
  yield* fileSystem.rename(staging, target);
  return target;
});

/**
 * Extracts the embedded library, if any, and points fff-node at it. Failures are
 * logged and swallowed: losing the file tree is no reason to refuse to boot.
 */
export const ensureFffNativeLibrary = Effect.fn("ensureFffNativeLibrary")(function* (input: {
  readonly baseDir: string;
  /** Overridable so the swallow can be tested off bun, where nothing is embedded. */
  readonly asset?: FffLibraryAsset | undefined;
}): Effect.fn.Return<void, never, FileSystem.FileSystem | Path.Path> {
  const env = yield* HostProcess.Environment;
  const fileSystem = yield* FileSystem.FileSystem;

  // An operator's explicit path wins, and is left exactly as given.
  const override = env[FFF_NATIVE_LIB_PATH_ENV]?.trim();
  if (override !== undefined && override.length > 0) {
    const exists = yield* fileSystem.exists(override).pipe(Effect.orElseSucceed(() => false));
    if (exists) return;
    yield* Effect.logWarning(`${FFF_NATIVE_LIB_PATH_ENV} points at a file that does not exist`, {
      path: override,
    });
  }

  const asset = input.asset ?? embeddedFffLibrary();
  if (asset === undefined) return;

  yield* extractFffNativeLibrary({ baseDir: input.baseDir, asset }).pipe(
    Effect.flatMap((target) =>
      Effect.sync(() => {
        env[FFF_NATIVE_LIB_PATH_ENV] = target;
      }),
    ),
    Effect.catchCause((cause) =>
      Effect.logWarning(
        "Failed to extract the fff native library; file listings and path search will be unavailable",
        { cause },
      ),
    ),
  );
});
