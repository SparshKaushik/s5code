// @effect-diagnostics preferSchemaOverJson:off
import * as NodeServices from "@effect/platform-node/NodeServices";
import { beforeEach, describe, expect, it } from "@effect/vitest";
import { ProviderDriverKind, ProviderInstanceId } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import { vi } from "vite-plus/test";

import { OpenCodeDriver } from "./OpenCodeDriver.ts";
import { makeOpenCodeHost } from "../OpenCodeHost.ts";
import { checkOpenCodeProviderStatus } from "../Layers/OpenCodeProvider.ts";

const localService = vi.hoisted(() => ({
  ensure: vi.fn(async () => ({
    url: "http://127.0.0.1:1",
    auth: { type: "basic" as const, username: "opencode", password: "test-password" },
  })),
}));

vi.mock("@opencode-ai/client-v2/service", () => ({
  ensure: localService.ensure,
  headers: () => ({ authorization: "Basic test-credentials" }),
}));

type OpenCodeFetch = NonNullable<Parameters<typeof makeOpenCodeHost>[0]["fetch"]>;

const mockFetch: OpenCodeFetch = (async (input, init) => {
  const url = String(input);
  if (url.includes("/health")) {
    return new Response(JSON.stringify({ healthy: true, version: "2.0.0" }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  }
  return new Response(JSON.stringify({ data: [] }), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}) as OpenCodeFetch;

describe("OpenCodeDriver", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("exports correct driver metadata and defaults", () => {
    expect(OpenCodeDriver.driverKind).toBe(ProviderDriverKind.make("opencode"));
    expect(OpenCodeDriver.metadata.displayName).toBe("OpenCode");
    expect(OpenCodeDriver.metadata.supportsMultipleInstances).toBe(true);

    const defaultConfig = OpenCodeDriver.defaultConfig();
    expect(defaultConfig.enabled).toBe(false);
    expect(defaultConfig.serverUrl).toBe("");
    expect(defaultConfig.binaryPath).toBe("opencode");
  });

  it.effect("creates an HTTP client for the out-of-process local service", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const tempDir = yield* fileSystem.makeTempDirectoryScoped();

      const hostHandle = yield* makeOpenCodeHost({
        instanceId: ProviderInstanceId.make("opencode-test"),
        config: {
          enabled: true,
          binaryPath: "",
          serverUrl: "",
          serverPassword: "",
          customModels: [],
        },
        defaultDirectory: tempDir,
        stateDir: tempDir,
        fetch: mockFetch,
      });

      expect(hostHandle.isRemote).toBe(false);
      expect(typeof hostHandle.client.session.create).toBe("function");

      const snapshot = yield* checkOpenCodeProviderStatus(
        hostHandle,
        {
          enabled: true,
          binaryPath: "",
          serverUrl: "",
          serverPassword: "",
          customModels: [],
        },
        tempDir,
      );

      expect(snapshot.displayName).toBe("OpenCode");
      expect(snapshot.status).toBe("ready");
      expect(snapshot.enabled).toBe(true);
      expect(snapshot.supportsConversationFork).toBe(true);
      expect(snapshot.supportsInboxSteering).toBe(true);
      expect(snapshot.supportsInboxQueueing).toBe(true);
    }).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
  );

  it.effect("discovers agents, slash commands, and skills via OpenCode client APIs", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const tempDir = yield* fileSystem.makeTempDirectoryScoped();

      const hostHandle = yield* makeOpenCodeHost({
        instanceId: ProviderInstanceId.make("opencode-discovery-test"),
        config: {
          enabled: true,
          binaryPath: "",
          serverUrl: "",
          serverPassword: "",
          customModels: [
            {
              slug: "openai/gpt-4o",
              name: "GPT-4o",
            },
          ],
        },
        defaultDirectory: tempDir,
        stateDir: tempDir,
        fetch: mockFetch,
      });

      const snapshot = yield* checkOpenCodeProviderStatus(
        hostHandle,
        {
          enabled: true,
          binaryPath: "",
          serverUrl: "",
          serverPassword: "",
          customModels: [
            {
              slug: "openai/gpt-4o",
              name: "GPT-4o",
            },
          ],
        },
        tempDir,
      );

      expect(snapshot.models.map((m) => m.slug)).toContain("openai/gpt-4o");
      expect(snapshot.slashCommands.length).toBeGreaterThan(0);
      expect(snapshot.slashCommands.map((c) => c.name)).toContain("compact");
    }).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
  );

  it.effect("applies custom binaryPath and environment to service ensure", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const tempDir = yield* fileSystem.makeTempDirectoryScoped();

      const hostHandle = yield* makeOpenCodeHost({
        instanceId: ProviderInstanceId.make("opencode-env-test"),
        config: {
          enabled: true,
          binaryPath: "opencode-custom",
          serverUrl: "",
          serverPassword: "",
          customModels: [],
        },
        defaultDirectory: tempDir,
        stateDir: tempDir,
        environment: {
          TEST_OPENCODE_INJECTED_KEY: "test-injected-value",
        },
        fetch: mockFetch,
      });

      expect(hostHandle.isRemote).toBe(false);
      expect(hostHandle.databasePath).toBeNull();
      expect(process.env.TEST_OPENCODE_INJECTED_KEY).toBe("test-injected-value");
      expect(localService.ensure).toHaveBeenCalledWith({
        command: ["opencode-custom", "serve", "--service"],
        env: { TEST_OPENCODE_INJECTED_KEY: "test-injected-value" },
        version: expect.any(Function),
      });
      delete process.env.TEST_OPENCODE_INJECTED_KEY;
    }).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
  );

  it.effect("rejects OpenCode versions prior to 2.0.0 with error status", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const tempDir = yield* fileSystem.makeTempDirectoryScoped();

      const legacyFetch: OpenCodeFetch = (async (input, init) => {
        const url = String(input);
        if (url.includes("/health")) {
          return new Response(JSON.stringify({ healthy: true, version: "1.14.19" }), {
            status: 200,
            headers: { "content-type": "application/json" },
          });
        }
        return new Response(JSON.stringify({ data: [] }), {
          status: 200,
          headers: { "content-type": "application/json" },
        });
      }) as OpenCodeFetch;

      const hostHandle = yield* makeOpenCodeHost({
        instanceId: ProviderInstanceId.make("opencode-legacy-test"),
        config: {
          enabled: true,
          binaryPath: "",
          serverUrl: "",
          serverPassword: "",
          customModels: [],
        },
        defaultDirectory: tempDir,
        stateDir: tempDir,
        fetch: legacyFetch,
      });

      const snapshot = yield* checkOpenCodeProviderStatus(
        hostHandle,
        {
          enabled: true,
          binaryPath: "",
          serverUrl: "",
          serverPassword: "",
          customModels: [],
        },
        tempDir,
      );

      expect(snapshot.status).toBe("error");
      expect(snapshot.installed).toBe(true);
      expect(snapshot.version).toBe("1.14.19");
      expect(snapshot.message).toContain("OpenCode v1.14.19 is not supported");
      expect(snapshot.message).toContain("2.0.0 or newer");
    }).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
  );
});
