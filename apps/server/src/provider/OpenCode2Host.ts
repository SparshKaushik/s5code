/**
 * OpenCode2Host — manages the HTTP client for an OpenCode 2 instance.
 *
 * When `serverUrl` is empty, the OpenCode client service helper discovers or starts an
 * out-of-process `opencode2 serve --service` daemon. The OpenCode SDK is never loaded into
 * the S5 Code server process.
 *
 * @module provider/OpenCode2Host
 */
import { OpenCode as OpenCodeClient } from "@opencode-ai/client-v2";
import { OpenCode2Settings, ProviderDriverKind, ProviderInstanceId } from "@t3tools/contracts";
import * as Effect from "effect/Effect";

import { ProviderDriverError } from "./Errors.ts";

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
): Effect.Effect<OpenCode2HostHandle, ProviderDriverError> {
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
      }) as OpenCode2ClientFacade;

      return {
        instanceId: options.instanceId,
        client,
        isRemote: true,
        databasePath: null,
      } satisfies OpenCode2HostHandle;
    }

    const localService = yield* Effect.tryPromise({
      try: async () => {
        const service = await import("@opencode-ai/client-v2/service");
        const endpoint = await service.ensure({
          command: ["opencode2", "serve", "--service"],
          ...(options.environment ? { env: options.environment } : {}),
        });
        return { endpoint, headers: service.headers(endpoint) };
      },
      catch: (cause) =>
        new ProviderDriverError({
          driver: ProviderDriverKind.make("opencode2"),
          instanceId: options.instanceId,
          detail:
            "Failed to connect to or start the local OpenCode 2 service. " +
            "Install the OpenCode 2 CLI and ensure `opencode2` is available on PATH.",
          cause,
        }),
    });

    const client = OpenCodeClient.make({
      baseUrl: localService.endpoint.url,
      ...(localService.headers ? { headers: localService.headers } : {}),
    }) as OpenCode2ClientFacade;

    return {
      instanceId: options.instanceId,
      client,
      isRemote: false,
      // databasePath is retained in settings for backwards compatibility, but an
      // out-of-process service owns its database selection and lifecycle.
      databasePath: null,
    } satisfies OpenCode2HostHandle;
  });
}
