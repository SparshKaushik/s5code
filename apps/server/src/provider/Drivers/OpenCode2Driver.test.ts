// @effect-diagnostics preferSchemaOverJson:off
import * as NodeServices from "@effect/platform-node/NodeServices";
import { describe, expect, it } from "@effect/vitest";
import { ProviderDriverKind, ProviderInstanceId } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";

import { OpenCode2Driver } from "./OpenCode2Driver.ts";
import { makeOpenCode2Host } from "../OpenCode2Host.ts";
import { checkOpenCode2ProviderStatus } from "../Layers/OpenCode2Provider.ts";

describe("OpenCode2Driver", () => {
  it("exports correct driver metadata and defaults", () => {
    expect(OpenCode2Driver.driverKind).toBe(ProviderDriverKind.make("opencode2"));
    expect(OpenCode2Driver.metadata.displayName).toBe("OpenCode 2");
    expect(OpenCode2Driver.metadata.supportsMultipleInstances).toBe(true);

    const defaultConfig = OpenCode2Driver.defaultConfig();
    expect(defaultConfig.enabled).toBe(false);
    expect(defaultConfig.serverUrl).toBe("");
    expect(defaultConfig.databasePath).toBe("");
  });

  it.effect("instantiates host and probes provider snapshot in embedded mode", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const path = yield* Path.Path;
      const tempDir = yield* fileSystem.makeTempDirectoryScoped();

      const hostHandle = yield* makeOpenCode2Host({
        instanceId: ProviderInstanceId.make("opencode2-test"),
        config: {
          enabled: true,
          serverUrl: "",
          serverPassword: "",
          databasePath: path.join(tempDir, "sessions.db"),
          customModels: [],
        },
        defaultDirectory: tempDir,
        stateDir: tempDir,
      });

      expect(hostHandle.isRemote).toBe(false);
      expect(typeof hostHandle.client.session.create).toBe("function");

      const snapshot = yield* checkOpenCode2ProviderStatus(
        hostHandle,
        {
          enabled: true,
          serverUrl: "",
          serverPassword: "",
          databasePath: path.join(tempDir, "sessions.db"),
          customModels: [],
        },
        tempDir,
      );

      expect(snapshot.displayName).toBe("OpenCode 2");
      expect(snapshot.status).toBe("ready");
      expect(snapshot.enabled).toBe(true);
      expect(snapshot.supportsConversationFork).toBe(true);
      expect(snapshot.supportsInboxSteering).toBe(true);
      expect(snapshot.supportsInboxQueueing).toBe(true);
    }).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
  );

  it.effect(
    "ensures __filename and __dirname are defined on globalThis when initializing host",
    () =>
      Effect.gen(function* () {
        const fileSystem = yield* FileSystem.FileSystem;
        const path = yield* Path.Path;
        const tempDir = yield* fileSystem.makeTempDirectoryScoped();

        // Explicitly delete globals to simulate pure ESM scope
        delete (globalThis as any).__filename;
        delete (globalThis as any).__dirname;

        const hostHandle = yield* makeOpenCode2Host({
          instanceId: ProviderInstanceId.make("opencode2-shims-test"),
          config: {
            enabled: true,
            serverUrl: "",
            serverPassword: "",
            databasePath: path.join(tempDir, "sessions.db"),
            customModels: [],
          },
          defaultDirectory: tempDir,
          stateDir: tempDir,
        });

        expect(hostHandle.isRemote).toBe(false);
        expect(typeof (globalThis as any).__filename).toBe("string");
        expect(typeof (globalThis as any).__dirname).toBe("string");
      }).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
  );

  it.effect(
    "discovers models, agents, slash commands, and skills from workspace configuration",
    () =>
      Effect.gen(function* () {
        const fileSystem = yield* FileSystem.FileSystem;
        const path = yield* Path.Path;
        const tempDir = yield* fileSystem.makeTempDirectoryScoped();

        // Write an opencode.json configuring providers and models
        yield* fileSystem.writeFileString(
          path.join(tempDir, "opencode.json"),
          JSON.stringify({
            providers: {
              "custom-llm": {
                name: "Custom LLM Provider",
                models: {
                  "fast-model": {},
                  "smart-model": {},
                },
              },
            },
          }),
        );

        const hostHandle = yield* makeOpenCode2Host({
          instanceId: ProviderInstanceId.make("opencode2-discovery-test"),
          config: {
            enabled: true,
            serverUrl: "",
            serverPassword: "",
            databasePath: path.join(tempDir, "sessions.db"),
            customModels: [],
          },
          defaultDirectory: tempDir,
          stateDir: tempDir,
        });

        const snapshot = yield* checkOpenCode2ProviderStatus(
          hostHandle,
          {
            enabled: true,
            serverUrl: "",
            serverPassword: "",
            databasePath: path.join(tempDir, "sessions.db"),
            customModels: [],
          },
          tempDir,
        );

        const modelSlugs = snapshot.models.map((m) => m.slug);
        expect(modelSlugs).toContain("custom-llm/fast-model");
        expect(modelSlugs).toContain("custom-llm/smart-model");

        const fastModel = snapshot.models.find((m) => m.slug === "custom-llm/fast-model");
        expect(fastModel?.subProvider).toBe("Custom LLM Provider");
        expect(fastModel?.capabilities?.optionDescriptors?.length).toBeGreaterThan(0);

        // Verify agents and slash commands are discovered
        expect(snapshot.slashCommands.length).toBeGreaterThan(0);
        expect(snapshot.slashCommands.map((c) => c.name)).toContain("compact");
      }).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
  );

  it.effect("applies instance environment to process.env and defaults to shared opencode.db", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const tempDir = yield* fileSystem.makeTempDirectoryScoped();

      const hostHandle = yield* makeOpenCode2Host({
        instanceId: ProviderInstanceId.make("opencode2-env-test"),
        config: {
          enabled: true,
          serverUrl: "",
          serverPassword: "",
          databasePath: "",
          customModels: [],
        },
        defaultDirectory: tempDir,
        stateDir: tempDir,
        environment: {
          TEST_OPENCODE2_INJECTED_KEY: "test-injected-value",
        },
      });

      expect(hostHandle.isRemote).toBe(false);
      expect(hostHandle.databasePath).toBeNull();
      expect(process.env.TEST_OPENCODE2_INJECTED_KEY).toBe("test-injected-value");
      delete process.env.TEST_OPENCODE2_INJECTED_KEY;
    }).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
  );
});
