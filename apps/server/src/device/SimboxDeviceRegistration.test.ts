import { describe, expect, it } from "@effect/vitest";
import * as NodeServices from "@effect/platform-node/NodeServices";
import {
  DeviceHostId,
  type DeviceOpenInput,
  type DeviceServiceState,
  ThreadId,
  type OrchestrationV2TurnItem,
} from "@t3tools/contracts";
import * as Clock from "effect/Clock";
import * as DateTime from "effect/DateTime";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Path from "effect/Path";
import * as PubSub from "effect/PubSub";
import * as Schema from "effect/Schema";
import * as TestClock from "effect/testing/TestClock";
import { HttpClient, HttpClientResponse } from "effect/http";
import * as HostProcess from "@t3tools/shared/HostProcess";
import * as ServerSettings from "../serverSettings.ts";
import * as DeviceService from "./DeviceService.ts";
import * as Registration from "./SimboxDeviceRegistration.ts";

const id = "f1234567-1234-1234-1234-123456789abc";
const marker = `[simbox-device:${id}]`;
const threadId = ThreadId.make("thread-a");
const runId = "039dd761-9144-4a9c-a03b-e44c893d73ad";
const hostId = DeviceHostId.make(`simbox-${runId}`);
const descriptor = (createdAt: number, stop = false) => ({
  version: 1 as const,
  createdAt,
  runId,
  tunnelUrl: "https://r-0123456789abcdef.simboxtouchtech.dpdns.org/",
  daemonToken: "private-daemon-token",
  platform: "ios" as const,
  stop,
});
const encode = Schema.encodeEffect(Schema.fromJsonString(Registration.Registration));
const item = (
  output: unknown,
  type: "command_execution" | "dynamic_tool" = "command_execution",
): OrchestrationV2TurnItem => ({
  id: "item" as OrchestrationV2TurnItem["id"],
  threadId,
  runId: null,
  nodeId: null,
  providerThreadId: null,
  providerTurnId: null,
  nativeItemRef: null,
  parentItemId: null,
  ordinal: 0,
  status: "completed",
  title: null,
  startedAt: null,
  completedAt: null,
  updatedAt: DateTime.nowUnsafe(),
  ...(type === "command_execution"
    ? { type, input: "simbox sim", output: String(output) }
    : { type, toolName: "shell", input: { command: "simbox sim" }, output }),
});
const receiptDirectory = Effect.gen(function* () {
  const fs = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const directory = yield* fs.makeTempDirectoryScoped({ prefix: "s5-simbox-registration-" });
  return { fs, directory, file: path.join(directory, `${id}.json`) };
});

/** A device service whose Simbox host lists one lease, the agent's. */
const fixture = (directory: string, options: { readonly healthy?: boolean } = {}) => {
  const opened: Array<DeviceOpenInput> = [];
  const state = {
    hosts: [{ id: hostId }],
    devices: [
      { hostId, id: "lease-1", platform: "ios", name: "iPhone 17e", booted: true },
      { hostId, id: "simlock:ios:iPhone 17e", platform: "ios", name: "iPhone 17e", booted: false },
    ],
  } as unknown as DeviceServiceState;
  const devices = Layer.mock(DeviceService.DeviceService)({
    state: Effect.succeed(state),
    subscribe: Effect.flatMap(PubSub.unbounded<DeviceServiceState>(), PubSub.subscribe),
    sessionsForThread: () => Effect.succeed([]),
    list: Effect.succeed(state),
    open: (input) =>
      Effect.sync(() => {
        opened.push(input);
        return { ...input, hostId, openedAt: "" };
      }),
  });
  const http = Layer.succeed(
    HttpClient.HttpClient,
    HttpClient.make((request) =>
      Effect.succeed(
        HttpClientResponse.fromWeb(
          request,
          new Response(null, { status: options.healthy === false ? 530 : 200 }),
        ),
      ),
    ),
  );
  const layer = Registration.layer.pipe(
    Layer.provideMerge(
      ServerSettings.layerTest({ enableDeviceSupport: true, enableAgentDeviceAccess: true }),
    ),
    Layer.provide(
      Layer.mergeAll(
        devices,
        http,
        Layer.succeed(HostProcess.Environment, { SIMBOX_DEVICE_REGISTRATION_DIR: directory }),
      ),
    ),
  );
  return { opened, layer };
};

describe("Simbox CLI registration receipts", () => {
  it("recognizes shell results from different providers and ignores instructions and arguments", () => {
    expect(Registration.receiptsForItem(item(marker))).toEqual([id]);
    expect(
      Registration.receiptsForItem(
        item({ content: [{ type: "text", text: marker }] }, "dynamic_tool"),
      ),
    ).toEqual([id]);
    expect(Registration.receiptsForItem({ type: "assistant_message", output: marker })).toEqual([]);
    expect(
      Registration.receiptsForItem({ type: "command_execution", output: "normal output" }),
    ).toEqual([]);
    expect(
      Registration.receiptsForItem({
        type: "command_execution",
        output: "[simbox-device:../../secrets]",
      }),
    ).toEqual([]);
  });

  it.effect(
    "consumes a receipt once, saves the run as a Simlock host, and opens the agent's lease in the originating thread",
    () =>
      Effect.gen(function* () {
        const { fs, directory, file } = yield* receiptDirectory;
        yield* fs.writeFileString(file, yield* encode(descriptor(yield* Clock.currentTimeMillis)));
        const { opened, layer } = fixture(directory);
        const saved = yield* Effect.gen(function* () {
          const registration = yield* Registration.SimboxDeviceRegistration;
          const event = item(
            { content: [{ text: `daemonToken=untrusted-output-token ${marker}` }] },
            "dynamic_tool",
          );
          yield* registration.observe(event);
          yield* registration.observe({ ...event, threadId: ThreadId.make("thread-b") });
          return yield* (yield* ServerSettings.ServerSettingsService).getSettings;
        }).pipe(Effect.provide(layer));
        expect(saved.simlockDeviceHostsEnabled).toBe(true);
        expect(saved.simlockDeviceHosts).toEqual([
          expect.objectContaining({
            id: hostId,
            gatewayUrl: "https://r-0123456789abcdef.simboxtouchtech.dpdns.org/",
            agentToken: "private-daemon-token",
          }),
        ]);
        expect(opened).toEqual([
          { threadId, hostId, deviceId: "lease-1", platform: "ios", boot: false },
        ]);
        expect(yield* fs.readDirectory(directory)).toEqual([]);
      }).pipe(Effect.provide(NodeServices.layer), Effect.scoped),
  );

  it.effect("discards expired receipts without attaching a device", () =>
    Effect.gen(function* () {
      const { fs, directory, file } = yield* receiptDirectory;
      const now = yield* Clock.currentTimeMillis;
      yield* fs.writeFileString(file, yield* encode(descriptor(now - 600_000)));
      const { opened, layer } = fixture(directory);
      const saved = yield* Effect.gen(function* () {
        yield* (yield* Registration.SimboxDeviceRegistration).observe(item(marker));
        return yield* (yield* ServerSettings.ServerSettingsService).getSettings;
      }).pipe(Effect.provide(layer));
      expect(saved.simlockDeviceHosts).toEqual([]);
      expect(opened).toEqual([]);
      expect(yield* fs.readDirectory(directory)).toEqual([]);
    }).pipe(Effect.provide(NodeServices.layer), Effect.scoped),
  );

  it.effect("removes a run on `simbox stop` and once its tunnel stays gone", () =>
    Effect.gen(function* () {
      const { fs, directory, file } = yield* receiptDirectory;
      const { layer } = fixture(directory, { healthy: false });
      const hosts = yield* Effect.gen(function* () {
        const registration = yield* Registration.SimboxDeviceRegistration;
        const settings = yield* ServerSettings.ServerSettingsService;
        const now = yield* Clock.currentTimeMillis;
        yield* fs.writeFileString(file, yield* encode(descriptor(now)));
        yield* registration.observe(item(marker));
        const attached = (yield* settings.getSettings).simlockDeviceHosts.length;
        yield* TestClock.adjust("4 minutes");
        const beforeDeadline = (yield* settings.getSettings).simlockDeviceHosts.length;
        yield* TestClock.adjust("2 minutes");
        const afterDeadline = (yield* settings.getSettings).simlockDeviceHosts.length;

        const stopId = "a1234567-1234-1234-1234-123456789abc";
        yield* settings.updateSettings({
          simlockDeviceHosts: [Registration.hostConfig(descriptor(0))],
        });
        yield* fs.writeFileString(
          file.replace(id, stopId),
          yield* encode(descriptor(yield* Clock.currentTimeMillis, true)),
        );
        yield* registration.observe(item(`[simbox-device:${stopId}]`));
        const afterStop = (yield* settings.getSettings).simlockDeviceHosts.length;
        return { attached, beforeDeadline, afterDeadline, afterStop };
      }).pipe(Effect.provide(layer));
      expect(hosts).toEqual({ attached: 1, beforeDeadline: 1, afterDeadline: 0, afterStop: 0 });
    }).pipe(Effect.provide(NodeServices.layer), Effect.scoped),
  );
});
