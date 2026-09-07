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
});
