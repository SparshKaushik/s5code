// @effect-diagnostics nodeBuiltinImport:off -- SDK bootstrap runs before the Effect runtime.
import * as NodeFS from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";

interface NativeAsset {
  readonly size: number;
  readonly arrayBuffer: () => Promise<ArrayBuffer>;
}

/** Materialize native helpers atomically, repairing truncated cached files. */
export async function extractCursorNativeAssets(
  directory: string,
  files: ReadonlyMap<string, NativeAsset>,
): Promise<void> {
  for (const [name, asset] of files) {
    const target = NodePath.join(directory, name);
    const executable = name.startsWith("bin/") || name.endsWith(".node");
    const existing = await NodeFS.stat(target).catch(() => undefined);
    if (
      existing?.isFile() &&
      existing.size === asset.size &&
      (!executable || (existing.mode & 0o111) !== 0)
    ) {
      continue;
    }
    await NodeFS.mkdir(NodePath.dirname(target), { recursive: true });
    const staging = `${target}.${process.pid}.partial`;
    await NodeFS.writeFile(staging, new Uint8Array(await asset.arrayBuffer()));
    await NodeFS.chmod(staging, executable ? 0o755 : 0o644);
    await NodeFS.rename(staging, target);
  }
}

/**
 * The SDK loads tree-sitter while importing, before server configuration exists.
 * Keep its compiled-only resources in the OS cache and initialize them first.
 * Bun's asset hash separates rebuilt helpers from previously extracted copies.
 */
export async function initializeCursorNativeAssets(): Promise<void> {
  if (typeof Bun === "undefined") return;
  const asset = (Bun.embeddedFiles as ReadonlyArray<Blob & { readonly name: string }>).find(
    (file) => file.name.startsWith("cursor-sdk-native-") && file.name.endsWith(".tgz"),
  );
  if (asset === undefined) return;
  const cacheHome = process.env.XDG_CACHE_HOME || NodePath.join(NodeOS.homedir(), ".cache");
  const directory = NodePath.join(cacheHome, "s5code", "native", asset.name);
  const files = await new Bun.Archive(await asset.arrayBuffer()).files();
  await extractCursorNativeAssets(directory, files);
  process.env.T3CODE_CURSOR_SDK_PLATFORM_DIR = directory;
}
