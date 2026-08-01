/**
 * PiExtensionAssets — locate the bundled `t3-runtime-mode` pi extension.
 *
 * The file ships as source, not as part of our bundle: pi loads extensions with
 * its own jiti-based loader, which compiles TypeScript itself. So this is a
 * path lookup, not an import.
 *
 * Candidate order mirrors `ResourceMonitorBinary`: the packaged location first,
 * then the in-repo source path so `vp run dev` works without a build step.
 *
 * @module provider/pi/PiExtensionAssets
 */
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";

export const PI_RUNTIME_MODE_EXTENSION_FILENAME = "t3-runtime-mode.ts";

/**
 * Resolve the runtime-mode extension path, or `undefined` when it is not on
 * disk. A missing extension is not fatal: pi still runs, it just runs without
 * approval gating, and the adapter logs that once rather than failing the
 * session.
 */
export const resolvePiRuntimeModeExtensionPath = Effect.fn("resolvePiRuntimeModeExtensionPath")(
  function* (): Effect.fn.Return<string | undefined, never, FileSystem.FileSystem | Path.Path> {
    const fileSystem = yield* FileSystem.FileSystem;
    const path = yield* Path.Path;
    const candidates = [
      // Packaged: `dist/pi-extension/<file>` next to the bundled server entry.
      path.resolve(import.meta.dirname, "pi-extension", PI_RUNTIME_MODE_EXTENSION_FILENAME),
      path.resolve(import.meta.dirname, "../pi-extension", PI_RUNTIME_MODE_EXTENSION_FILENAME),
      // Dev: source tree, from `apps/server/src/provider/pi/`.
      path.resolve(
        import.meta.dirname,
        "../../../pi-extension",
        PI_RUNTIME_MODE_EXTENSION_FILENAME,
      ),
    ];

    for (const candidate of candidates) {
      if (yield* fileSystem.exists(candidate).pipe(Effect.orElseSucceed(() => false))) {
        return candidate;
      }
    }
    return undefined;
  },
);
