import { assert, describe, it } from "@effect/vitest";
import { ProviderInstanceId } from "@t3tools/contracts";
import * as Effect from "effect/Effect";

import { checkOpenCodeProviderStatus } from "../src/provider/Layers/OpenCodeProvider.ts";
import { makeOpenCodeHost } from "../src/provider/OpenCodeHost.ts";

describe("OpenCode live service integration", () => {
  it.live(
    "connects to local opencode service daemon via default opencode command",
    () =>
      Effect.gen(function* () {
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

        assert.strictEqual(handle.instanceId, "opencode-integration-test-1");
        assert.isFalse(handle.isRemote);

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

        assert.strictEqual(status.displayName, "OpenCode");
        assert.isTrue(status.installed);
        assert.strictEqual(status.status, "ready");
      }),
    30_000,
  );

  it.live(
    "connects to local opencode service daemon via explicit binaryPath",
    () =>
      Effect.gen(function* () {
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

        assert.strictEqual(handle.instanceId, "opencode-integration-test-2");
        assert.isFalse(handle.isRemote);

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

        assert.strictEqual(status.displayName, "OpenCode");
        assert.isTrue(status.installed);
        assert.strictEqual(status.status, "ready");
      }),
    30_000,
  );
});
