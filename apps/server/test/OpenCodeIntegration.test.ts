import { ProviderInstanceId } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import { describe, expect, it } from "vite-plus/test";

import { checkOpenCodeProviderStatus } from "../src/provider/Layers/OpenCodeProvider.ts";
import { makeOpenCodeHost } from "../src/provider/OpenCodeHost.ts";

describe("OpenCode live service integration", () => {
  it("connects to local opencode service daemon via default opencode command", async () => {
    const program = Effect.gen(function* () {
      const handle = yield* makeOpenCodeHost({
        instanceId: ProviderInstanceId.make("opencode-integration-test-1"),
        config: {
          enabled: true,
          binaryPath: "opencode",
          serverUrl: "",
          serverPassword: "",
          customModels: [],
        },
        defaultDirectory: process.cwd(),
        stateDir: "/tmp/opencode",
      });

      expect(handle.instanceId).toBe("opencode-integration-test-1");
      expect(handle.isRemote).toBe(false);

      const status = yield* checkOpenCodeProviderStatus(
        handle,
        {
          enabled: true,
          binaryPath: "opencode",
          serverUrl: "",
          serverPassword: "",
          customModels: [],
        },
        process.cwd(),
      );

      expect(status.displayName).toBe("OpenCode");
      expect(status.installed).toBe(true);
      expect(status.status).toBe("ready");
    });

    await Effect.runPromise(program);
  }, 30_000);

  it("connects to local opencode service daemon via explicit binaryPath", async () => {
    const program = Effect.gen(function* () {
      const handle = yield* makeOpenCodeHost({
        instanceId: ProviderInstanceId.make("opencode-integration-test-2"),
        config: {
          enabled: true,
          binaryPath: "/home/ubuntu/.local/bin/opencode",
          serverUrl: "",
          serverPassword: "",
          customModels: [],
        },
        defaultDirectory: process.cwd(),
        stateDir: "/tmp/opencode",
      });

      expect(handle.instanceId).toBe("opencode-integration-test-2");
      expect(handle.isRemote).toBe(false);

      const status = yield* checkOpenCodeProviderStatus(
        handle,
        {
          enabled: true,
          binaryPath: "/home/ubuntu/.local/bin/opencode",
          serverUrl: "",
          serverPassword: "",
          customModels: [],
        },
        process.cwd(),
      );

      expect(status.displayName).toBe("OpenCode");
      expect(status.installed).toBe(true);
      expect(status.status).toBe("ready");
    });

    await Effect.runPromise(program);
  }, 30_000);
});
