// @effect-diagnostics nodeBuiltinImport:off -- The SDK must load from disk beside its Webpack chunks.
import * as NodeModule from "node:module";

import { initializeCursorNativeAssets } from "./CursorNativeAssets.ts";

await initializeCursorNativeAssets();

// Cursor's Webpack chunks and local helpers must stay beside the SDK entry.
// Bun bundles its flattened entry after extracting the native helpers.
const requireCursorSdk = NodeModule.createRequire(import.meta.url);
export const {
  Agent,
  AuthenticationError,
  createAgentPlatform,
  Cursor,
  CursorSdkError,
  InMemoryCredentialStore,
} = (
  typeof Bun === "undefined" ? requireCursorSdk("@cursor/sdk") : await import("@cursor/sdk")
) as typeof import("@cursor/sdk");
