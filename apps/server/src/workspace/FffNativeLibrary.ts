/**
 * FffNativeLibrary — make `libfff_c` loadable from a compiled server binary.
 *
 * Every file listing in every client (`projects.listEntries`), every composer
 * path search, and every content search goes through `WorkspaceSearchIndex`,
 * which is a `FileFinder` from `@ff-labs/fff-node`, which dlopens `libfff_c`.
 * `fff-node` finds that library by resolving its own platform package out of
 * `node_modules`, relative to its own `import.meta.url`.
 *
 * Inside a `bun build --compile` binary both of those are gone. The module's
 * `import.meta.url` is `file:///$bunfs/root/...`, so there is no `node_modules`
 * above it to resolve from, and the host machine has no reason to have one.
 * `findBinary()` returns null, the index fails to create, and the user sees an
 * empty file tree on every client with the real reason only in the server log.
 *
 * Embedding the `.so` as a bun asset is necessary but not sufficient: bun's
 * virtual filesystem is not a real filesystem, and `dlopen("/$bunfs/root/...")`
 * fails with ENOENT because the dynamic loader is the kernel's, not bun's. So the
 * library has to be materialised on disk once, and `fff-node` has to be told
 * where. The patch in `patches/@ff-labs__fff-node@0.9.4.patch` gives
 * `findBinary()` an explicit `T3CODE_FFF_NATIVE_LIB_PATH` override for exactly
 * this, checked before every other heuristic.
 *
 * A no-op in every other way of running the server (`npx t3`, the desktop app, a
 * dev checkout): those all have a real `node_modules`, where `fff-node`'s own
 * resolution is correct.
 *
 * @module workspace/FffNativeLibrary
 */
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import * as PlatformError from "effect/PlatformError";

import { HostProcessEnvironment } from "@t3tools/shared/hostProcess";

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
export function embeddedFffLibrary(): FffLibraryAsset | undefined {
  if (typeof Bun === "undefined") return undefined;
  const files = Bun.embeddedFiles as unknown as ReadonlyArray<FffLibraryAsset> | undefined;
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
 * Extracts the embedded library, if there is one, and points `fff-node` at it.
 *
 * Failure is logged and swallowed. A server that cannot write its cache should
 * still serve threads, terminals, and git; it just cannot serve the file tree,
 * which is the same outcome as not having tried and not worth refusing to boot
 * over.
 */
export const ensureFffNativeLibrary = Effect.fn("ensureFffNativeLibrary")(function* (input: {
  readonly baseDir: string;
  /** Overridable so the swallow can be tested off bun, where nothing is embedded. */
  readonly asset?: FffLibraryAsset | undefined;
}): Effect.fn.Return<void, never, FileSystem.FileSystem | Path.Path> {
  const env = yield* HostProcessEnvironment;
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
