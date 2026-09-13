/**
 * Cross-runtime plugin source reloader for OpenCode 2.
 *
 * Replaces `@opencode-ai/plugin/dist/source.node.js` so that `registerHooks`
 * is not required when running inside the standalone Bun server binary.
 *
 * @module provider/pluginSourceCompat
 */
let generation = 0;

export async function prepareSource(
  entrypoint: string,
  _track: (file: string, missing?: boolean) => void,
): Promise<{ version: string; load: () => Promise<unknown>; dispose: () => void }> {
  const version = String(++generation);
  const fresh = (specifier: string) => {
    try {
      const url = new URL(specifier);
      url.searchParams.set("__opencode_reload", version);
      return url.href;
    } catch {
      return specifier;
    }
  };

  const specifier = fresh(entrypoint);
  return {
    version: specifier,
    load: async () => import(specifier),
    dispose: () => {},
  };
}
