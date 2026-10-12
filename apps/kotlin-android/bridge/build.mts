import { build } from "vite-plus";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { writeFile } from "node:fs/promises";

// Run with `node apps/kotlin-android/bridge/build.mts` after syncing RN transports.
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
for (const [entry, name, file] of [
  ["apps/kotlin-android/bridge/browser.ts", "T3PreviewStream", "preview-stream.js"],
  ["packages/client-runtime/src/mcpApps/host.ts", "T3McpAppHost", "mcp-app-host.js"],
  [
    "apps/mobile/src/features/devices/device-stream.browser.ts",
    "T3DeviceStream",
    "device-stream.js",
  ],
]) {
  const result = await build({
    configFile: false,
    logLevel: "warn",
    build: {
      write: false,
      target: "es2022",
      minify: true,
      lib: { entry: path.join(root, entry!), name, formats: ["iife"] },
    },
  });
  const bundles = Array.isArray(result) ? result : [result];
  const chunk = bundles
    .flatMap((bundle) => ("output" in bundle ? bundle.output : []))
    .find((output) => output.type === "chunk");
  if (!chunk) throw new Error(`${name} emitted no script.`);
  await writeFile(path.join(root, "apps/kotlin-android/app/src/main/assets", file!), chunk.code);
}
