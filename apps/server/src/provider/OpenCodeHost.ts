/**
 * OpenCodeHost — manages the HTTP client for an OpenCode instance.
 *
 * When `serverUrl` is empty, the OpenCode client service helper discovers or starts an
 * out-of-process `opencode serve --service` daemon. The OpenCode SDK is never loaded into
 * the S5 Code server process.
 *
 * @module provider/OpenCodeHost
 */
import { OpenCode as OpenCodeClient } from "@opencode-ai/client-v2";
import { OpenCodeSettings, ProviderDriverKind, ProviderInstanceId } from "@t3tools/contracts";
import * as Effect from "effect/Effect";

import { ProviderDriverError } from "./Errors.ts";
import { isOpenCodeVersionSupported } from "./Layers/OpenCodeProvider.ts";

export type OpenCodeClientFacade = ReturnType<typeof OpenCodeClient.make>;

export interface OpenCodeHostHandle {
  readonly instanceId: ProviderInstanceId;
  readonly client: OpenCodeClientFacade;
  readonly isRemote: boolean;
  readonly databasePath: string | null;
}

export interface MakeOpenCodeHostOptions {
  readonly instanceId: ProviderInstanceId;
  readonly config: OpenCodeSettings;
  readonly defaultDirectory: string;
  readonly stateDir: string;
  readonly environment?: Readonly<Record<string, string>> | undefined;
  readonly fetch?: NonNullable<Parameters<typeof OpenCodeClient.make>[0]>["fetch"];
}

export function makeOpenCodeHost(
  options: MakeOpenCodeHostOptions,
): Effect.Effect<OpenCodeHostHandle, ProviderDriverError> {
  return Effect.gen(function* () {
    // The service helper inherits process.env when it needs to start a daemon.
    if (options.environment) {
      for (const [key, value] of Object.entries(options.environment)) {
        if (value !== undefined && value.length > 0) {
          process.env[key] = value;
        }
      }
    }

    const isRemote = options.config.serverUrl.trim().length > 0;

    if (isRemote) {
      const serverUrl = options.config.serverUrl.trim();
      const headers: Record<string, string> = {};
      if (options.config.serverPassword.trim().length > 0) {
        const pass = options.config.serverPassword.trim();
        if (pass.startsWith("Basic ") || pass.startsWith("Bearer ")) {
          headers.authorization = pass;
        } else {
          const cred = pass.includes(":") ? pass : `opencode:${pass}`;
          headers.authorization = `Basic ${Buffer.from(cred).toString("base64")}`;
        }
      }

      const client = OpenCodeClient.make({
        baseUrl: serverUrl,
        ...(Object.keys(headers).length > 0 ? { headers } : {}),
        ...(options.fetch ? { fetch: options.fetch } : {}),
      }) as OpenCodeClientFacade;

      return {
        instanceId: options.instanceId,
        client,
        isRemote: true,
        databasePath: null,
      } satisfies OpenCodeHostHandle;
    }

    const binary = options.config.binaryPath?.trim() || "opencode";

    const localService = yield* Effect.tryPromise({
      try: async () => {
        const service = await import("@opencode-ai/client-v2/service");
        const endpoint = await service.ensure({
          command: [binary, "serve", "--service"],
          version: (v: string) => isOpenCodeVersionSupported(v),
          ...(options.environment ? { env: options.environment } : {}),
        });
        return { endpoint, headers: service.headers(endpoint) };
      },
      catch: (cause) =>
        new ProviderDriverError({
          driver: ProviderDriverKind.make("opencode"),
          instanceId: options.instanceId,
          detail:
            `Failed to connect to or start the local OpenCode service (requires v2.0 or later). ` +
            `Install the OpenCode CLI and ensure \`${binary}\` is available on PATH.`,
          cause,
        }),
    });

    const client = OpenCodeClient.make({
      baseUrl: localService.endpoint.url,
      ...(localService.headers ? { headers: localService.headers } : {}),
      ...(options.fetch ? { fetch: options.fetch } : {}),
    }) as OpenCodeClientFacade;

    return {
      instanceId: options.instanceId,
      client,
      isRemote: false,
      databasePath: null,
    } satisfies OpenCodeHostHandle;
  });
}
