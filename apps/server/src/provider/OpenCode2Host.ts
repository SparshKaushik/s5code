/**
 * OpenCode2Host — manages the embedded SDK host or remote client for an OpenCode 2 instance.
 *
 * Scoped lifecycle:
 * - Embedded mode (default, serverUrl empty): starts an in-process SDK host with a dedicated
 *   SQLite database at `<stateDir>/opencode2/<instanceId>/sessions.db`.
 *   When the scope closes, `host.close()` safely releases SQLite database locks, background
 *   fibers, and file handles.
 * - Remote mode (serverUrl set): connects using the generated client.
 *
 * @module provider/OpenCode2Host
 */
import { OpenCode as OpenCodeClient } from "@opencode-ai/client-v2";
import { OpenCode2Settings, ProviderDriverKind, ProviderInstanceId } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import type * as Scope from "effect/Scope";

import { ProviderDriverError } from "./Errors.ts";

function ensureCommonJsGlobals(): void {
  if (typeof (globalThis as any).__filename === "undefined") {
    (globalThis as any).__filename = import.meta.filename ?? "";
  }
  if (typeof (globalThis as any).__dirname === "undefined") {
    (globalThis as any).__dirname = import.meta.dirname ?? "";
  }
  // OpenCode uses createRequire(import.meta.url).resolve(...) at module-evaluation time
  // for WASM and PTY assets. In a bundled environment (such as Electron or Rolldown),
  // these external paths cannot be resolved relative to the bundle chunk.
  // Setting default environment variables prevents require.resolve from throwing.
  if (process.env.OPENCODE_TREE_SITTER_WASM_PATH === undefined) {
    process.env.OPENCODE_TREE_SITTER_WASM_PATH = "";
  }
  if (process.env.OPENCODE_TREE_SITTER_BASH_WASM_PATH === undefined) {
    process.env.OPENCODE_TREE_SITTER_BASH_WASM_PATH = "";
  }
  if (process.env.OPENCODE_TREE_SITTER_POWERSHELL_WASM_PATH === undefined) {
    process.env.OPENCODE_TREE_SITTER_POWERSHELL_WASM_PATH = "";
  }
  if (process.env.OPENCODE_PHOTON_WASM_PATH === undefined) {
    process.env.OPENCODE_PHOTON_WASM_PATH = "";
  }
  if (process.env.OPENCODE_NODE_PTY_PATH === undefined) {
    process.env.OPENCODE_NODE_PTY_PATH = "node-pty";
  }
}

ensureCommonJsGlobals();

export type OpenCode2ClientFacade = ReturnType<typeof OpenCodeClient.make>;

export interface OpenCode2HostHandle {
  readonly instanceId: ProviderInstanceId;
  readonly client: OpenCode2ClientFacade;
  readonly isRemote: boolean;
  readonly databasePath: string | null;
}

export interface MakeOpenCode2HostOptions {
  readonly instanceId: ProviderInstanceId;
  readonly config: OpenCode2Settings;
  readonly defaultDirectory: string;
  readonly stateDir: string;
  readonly environment?: Readonly<Record<string, string>> | undefined;
}

export function makeOpenCode2Host(
  options: MakeOpenCode2HostOptions,
): Effect.Effect<
  OpenCode2HostHandle,
  ProviderDriverError,
  FileSystem.FileSystem | Path.Path | Scope.Scope
> {
  return Effect.gen(function* () {
    const fileSystem = yield* FileSystem.FileSystem;
    const path = yield* Path.Path;
    const isRemote = options.config.serverUrl.trim().length > 0;

    if (isRemote) {
      const serverUrl = options.config.serverUrl.trim();
      const headers: Record<string, string> = {};
      if (options.config.serverPassword.trim().length > 0) {
        headers.authorization = `Bearer ${options.config.serverPassword.trim()}`;
      }

      const client = OpenCodeClient.make({
        baseUrl: serverUrl,
        ...(Object.keys(headers).length > 0 ? { headers } : {}),
      }) as OpenCode2ClientFacade;

      return {
        instanceId: options.instanceId,
        client,
        isRemote: true,
        databasePath: null,
      } satisfies OpenCode2HostHandle;
    }

    // Embedded in-process SDK mode
    if (typeof (globalThis as any).Bun !== "undefined") {
      return yield* new ProviderDriverError({
        driver: ProviderDriverKind.make("opencode2"),
        instanceId: options.instanceId,
        detail:
          "Embedded OpenCode 2 host requires Node.js (node:sqlite is unavailable in Bun standalone binary). Configure a remote serverUrl to connect to OpenCode 2.",
      });
    }

    const databasePath =
      options.config.databasePath.trim().length > 0
        ? options.config.databasePath.trim()
        : path.join(options.stateDir, "opencode2", options.instanceId, "sessions.db");

    const databaseDir = path.dirname(databasePath);
    yield* fileSystem.makeDirectory(databaseDir, { recursive: true }).pipe(
      Effect.mapError(
        (cause) =>
          new ProviderDriverError({
            driver: ProviderDriverKind.make("opencode2"),
            instanceId: options.instanceId,
            detail: `Failed to create OpenCode 2 database directory at '${databaseDir}': ${cause.message}`,
            cause,
          }),
      ),
    );

    const host = yield* Effect.acquireRelease(
      Effect.tryPromise({
        try: async () => {
          ensureCommonJsGlobals();
          const { OpenCode: OpenCodeSdk } = await import("@opencode-ai/sdk-v2");
          return await OpenCodeSdk.create({
            database: { path: databasePath },
            instances: {
              default: {
                directory: options.defaultDirectory,
              },
            },
          });
        },
        catch: (cause) =>
          new ProviderDriverError({
            driver: ProviderDriverKind.make("opencode2"),
            instanceId: options.instanceId,
            detail: `Failed to initialize embedded OpenCode 2 host at '${databasePath}': ${String(cause)}`,
            cause,
          }),
      }),
      (h: any) =>
        Effect.promise(async () => {
          try {
            await h.close();
          } catch {
            // best-effort cleanup on scope close
          }
        }),
    );

    return {
      instanceId: options.instanceId,
      client: host as unknown as OpenCode2ClientFacade,
      isRemote: false,
      databasePath,
    } satisfies OpenCode2HostHandle;
  });
}
