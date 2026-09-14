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
// @effect-diagnostics-next-line nodeBuiltinImport:off
import * as NodeFS from "node:fs";

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

    let binary = options.config.binaryPath?.trim() || "opencode";
    if (binary === "opencode") {
      const homedir = process.env.HOME;
      if (homedir) {
        const opencodeUserBin = `${homedir}/.opencode/bin/opencode`;
        if (NodeFS.existsSync(opencodeUserBin)) {
          binary = opencodeUserBin;
        }
      }
    }

    const fetchServiceEndpoint = async () => {
      const service = await import("@opencode-ai/client-v2/service");
      const endpoint = await service.ensure({
        command: [binary, "serve", "--service"],
        version: (v: string) => isOpenCodeVersionSupported(v),
        ...(options.environment ? { env: options.environment } : {}),
      });
      return { endpoint, headers: service.headers(endpoint) };
    };

    let currentService = yield* Effect.tryPromise({
      try: fetchServiceEndpoint,
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

    let refreshPromise: Promise<typeof currentService> | null = null;
    const refreshService = async () => {
      if (refreshPromise) return refreshPromise;
      refreshPromise = (async () => {
        try {
          const updated = await fetchServiceEndpoint();
          currentService = updated;
          return updated;
        } finally {
          refreshPromise = null;
        }
      })();
      return refreshPromise;
    };

    const rewriteRequest = (
      input: string | URL | Request,
      init: RequestInit | undefined,
      svc: typeof currentService,
    ) => {
      const inputUrl =
        typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
      const targetBase = new URL(svc.endpoint.url);
      const parsed = new URL(inputUrl, targetBase);
      parsed.protocol = targetBase.protocol;
      parsed.host = targetBase.host;

      const headers = new Headers(
        init?.headers ??
          (typeof input === "object" && "headers" in input
            ? (input as Request).headers
            : undefined),
      );
      if (svc.headers) {
        for (const [key, val] of Object.entries(svc.headers)) {
          if (val) {
            headers.set(key, val);
          }
        }
      }

      return {
        url: parsed.toString(),
        init: {
          ...init,
          headers,
        },
      };
    };

    const resilientFetch: (
      input: string | URL | Request,
      init?: RequestInit,
    ) => Promise<Response> = async (input, init) => {
      const baseFetch = options.fetch ?? globalThis.fetch;
      const first = rewriteRequest(input, init, currentService);
      try {
        const res = await baseFetch(first.url, first.init);
        if (res.status === 401 && !init?.signal?.aborted) {
          const refreshed = await refreshService();
          const second = rewriteRequest(input, init, refreshed);
          return await baseFetch(second.url, second.init);
        }
        return res;
      } catch (err) {
        if (init?.signal?.aborted) {
          throw err;
        }
        try {
          const refreshed = await refreshService();
          const second = rewriteRequest(input, init, refreshed);
          return await baseFetch(second.url, second.init);
        } catch {
          throw err;
        }
      }
    };

    const client = OpenCodeClient.make({
      baseUrl: currentService.endpoint.url,
      ...(currentService.headers ? { headers: currentService.headers } : {}),
      fetch: resilientFetch as unknown as typeof fetch,
    }) as OpenCodeClientFacade;

    return {
      instanceId: options.instanceId,
      client,
      isRemote: false,
      databasePath: null,
    } satisfies OpenCodeHostHandle;
  });
}
