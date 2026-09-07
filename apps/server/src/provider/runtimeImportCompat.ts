/**
 * Cross-runtime module resolution and import helper for OpenCode 2.
 *
 * Replaces `@opencode-ai/util/dist/runtime/import.node.js` so that the
 * non-standard `registerHooks` from Node.js is not required when running
 * inside the standalone Bun server binary.
 *
 * @module provider/runtimeImportCompat
 */
// @effect-diagnostics nodeBuiltinImport:off
import * as NodePath from "node:path";
import * as NodeURL from "node:url";

export async function importModule(specifier: string): Promise<unknown> {
  return await import(specifier);
}

export function resolveModule(specifier: string, directory: string): string {
  if (typeof (globalThis as any).Bun !== "undefined") {
    const resolved = (globalThis as any).Bun.resolveSync(specifier, directory);
    return resolved.startsWith("node:") ? resolved : NodeURL.pathToFileURL(resolved).href;
  }

  try {
    return import.meta.resolve(
      specifier,
      NodeURL.pathToFileURL(NodePath.join(directory, "package.json")).href,
    );
  } catch {
    return NodeURL.pathToFileURL(NodePath.resolve(directory, specifier)).href;
  }
}
