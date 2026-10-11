// @effect-diagnostics nodeBuiltinImport:off -- Disk-backed loader for the browser resources owned outside Effect.
import * as NodeFSP from "node:fs/promises";
import * as NodeModule from "node:module";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";

interface RuntimeAsset {
  readonly size: number;
  readonly arrayBuffer: () => Promise<ArrayBuffer>;
}

/** Restore Playwright's package layout, reusing intact files and repairing interrupted writes. */
export async function extractPlaywrightRuntime(
  directory: string,
  files: ReadonlyMap<string, RuntimeAsset>,
): Promise<string> {
  for (const [name, asset] of files) {
    const target = NodePath.join(directory, name);
    const existing = await NodeFSP.stat(target).catch(() => undefined);
    if (existing?.isFile() && existing.size === asset.size) continue;
    await NodeFSP.mkdir(NodePath.dirname(target), { recursive: true });
    const staging = `${target}.${process.pid}.partial`;
    await NodeFSP.writeFile(staging, new Uint8Array(await asset.arrayBuffer()));
    await NodeFSP.rename(staging, target);
  }
  return NodePath.join(directory, "index.js");
}

const requirePlaywright = NodeModule.createRequire(import.meta.url);
let standaloneRuntime: Promise<typeof import("playwright-core")> | undefined;

/** Playwright reads its own files; compiled Bun servers carry an archive instead of node_modules. */
export async function loadPlaywright(): Promise<typeof import("playwright-core")> {
  const asset =
    typeof Bun === "undefined"
      ? undefined
      : (Bun.embeddedFiles as ReadonlyArray<Blob & { readonly name: string }>).find(
          (file) => file.name.startsWith("playwright-core-") && file.name.endsWith(".tgz"),
        );
  if (asset === undefined) return requirePlaywright("playwright-core");
  if (!standaloneRuntime) {
    standaloneRuntime = (async () => {
      const cacheHome = process.env.XDG_CACHE_HOME || NodePath.join(NodeOS.homedir(), ".cache");
      const directory = NodePath.join(cacheHome, "s5code", "browser", asset.name);
      const files = await new Bun.Archive(await asset.arrayBuffer()).files();
      const entry = await extractPlaywrightRuntime(directory, files);
      return requirePlaywright(entry) as typeof import("playwright-core");
    })();
    void standaloneRuntime.catch(() => {
      standaloneRuntime = undefined;
    });
  }
  return standaloneRuntime;
}
