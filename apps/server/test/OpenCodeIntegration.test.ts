// @effect-diagnostics nodeBuiltinImport:off
import * as NodeChildProcess from "node:child_process";

import { assert, describe, it } from "@effect/vitest";
import { ProviderInstanceId } from "@t3tools/contracts";
import * as Effect from "effect/Effect";

import { checkOpenCodeProviderStatus } from "../src/provider/Layers/OpenCodeProvider.ts";
import { makeOpenCodeHost } from "../src/provider/OpenCodeHost.ts";

// These tests drive a real `opencode serve` process; environments without the
// CLI (CI runners) can't satisfy that, so they skip.
const openCodeBinaryPath = NodeChildProcess.spawnSync("which", ["opencode"], {
  encoding: "utf8",
}).stdout.trim();
const hasOpenCodeBinary =
  openCodeBinaryPath.length > 0 &&
  NodeChildProcess.spawnSync("opencode", ["--version"], { stdio: "ignore" }).status === 0;

describe("OpenCode live service integration", () => {
  it.live.skipIf(!hasOpenCodeBinary)(
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

  it.live.skipIf(!hasOpenCodeBinary)(
    "connects to local opencode service daemon via explicit binaryPath",
    () =>
      Effect.gen(function* () {
        const handle = yield* makeOpenCodeHost({
          instanceId: ProviderInstanceId.make("opencode-integration-test-2"),
          config: {
            enabled: true,
            binaryPath: openCodeBinaryPath,
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
            binaryPath: openCodeBinaryPath,
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
