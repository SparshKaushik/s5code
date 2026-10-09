import { describe, expect, it } from "@effect/vitest";
import * as NodeServices from "@effect/platform-node/NodeServices";
import { ThreadId, type OrchestrationV2TurnItem } from "@t3tools/contracts";
import * as Clock from "effect/Clock";
import * as DateTime from "effect/DateTime";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";
import * as DeviceService from "./DeviceService.ts";
import * as SimboxHost from "./SimboxDeviceHost.ts";
import * as Registration from "./SimboxDeviceRegistration.ts";

const id = "f1234567-1234-1234-1234-123456789abc";
const marker = `[simbox-device:${id}]`;
const threadId = ThreadId.make("thread-a");
const descriptor = (createdAt: number): SimboxHost.Registration => ({
  version: 1,
  createdAt,
  runId: "039dd761-9144-4a9c-a03b-e44c893d73ad",
  tunnelUrl: "https://fixture.trycloudflare.com",
  daemonToken: "private-daemon-token",
  expiresAt: null,
  stop: false,
  autoBoot: true,
  apiUrl: "https://api.simbox.touchtech.club",
  userToken: "private-user-token",
});
const encode = Schema.encodeEffect(Schema.fromJsonString(SimboxHost.Registration));
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
  const previous = process.env.SIMBOX_DEVICE_REGISTRATION_DIR;
  yield* Effect.acquireRelease(
    Effect.sync(() => {
      process.env.SIMBOX_DEVICE_REGISTRATION_DIR = directory;
    }),
    () =>
      Effect.sync(() => {
        if (previous === undefined) delete process.env.SIMBOX_DEVICE_REGISTRATION_DIR;
        else process.env.SIMBOX_DEVICE_REGISTRATION_DIR = previous;
      }),
  );
  return { fs, directory, file: path.join(directory, `${id}.json`) };
});

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
    "consumes a private receipt once, binds to the originating thread, and never trusts credentials from output",
    () =>
      Effect.gen(function* () {
        const { fs, directory, file } = yield* receiptDirectory;
        const now = yield* Clock.currentTimeMillis;
        yield* fs.writeFileString(file, yield* encode(descriptor(now)));
        const calls: Array<{ threadId: string; token: string }> = [];
        yield* Effect.gen(function* () {
          const registration = yield* Registration.SimboxDeviceRegistration;
          const event = item(
            { content: [{ text: `daemonToken=untrusted-output-token ${marker}` }] },
            "dynamic_tool",
          );
          yield* registration.observe(event);
          yield* registration.observe({ ...event, threadId: ThreadId.make("thread-b") });
        }).pipe(
          Effect.provide(
            Registration.layer.pipe(
              Layer.provide(
                Layer.mock(DeviceService.DeviceService)({
                  registerSimbox: (thread, input) =>
                    Effect.sync(() => {
                      calls.push({ threadId: thread, token: input.daemonToken });
                      return [];
                    }),
                }),
              ),
            ),
          ),
        );
        expect(calls).toEqual([{ threadId, token: "private-daemon-token" }]);
        expect(yield* fs.readDirectory(directory)).toEqual([]);
        expect(yield* fs.exists(file)).toBe(false);
      }).pipe(Effect.provide(NodeServices.layer), Effect.scoped),
  );

  it.effect("discards expired receipts without attaching a device", () =>
    Effect.gen(function* () {
      const { fs, directory, file } = yield* receiptDirectory;
      const now = yield* Clock.currentTimeMillis;
      yield* fs.writeFileString(file, yield* encode(descriptor(now - 600_000)));
      let called = false;
      yield* Effect.gen(function* () {
        yield* (yield* Registration.SimboxDeviceRegistration).observe(item(marker));
      }).pipe(
        Effect.provide(
          Registration.layer.pipe(
            Layer.provide(
              Layer.mock(DeviceService.DeviceService)({
                registerSimbox: () =>
                  Effect.sync(() => {
                    called = true;
                    return [];
                  }),
              }),
            ),
          ),
        ),
      );
      expect(called).toBe(false);
      expect(yield* fs.readDirectory(directory)).toEqual([]);
    }).pipe(Effect.provide(NodeServices.layer), Effect.scoped),
  );
});
