import { afterEach, describe, expect, it } from "@effect/vitest";
import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import { NodeServices } from "@effect/platform-node";
import { ThreadId, type OrchestrationV2TurnItem } from "@t3tools/contracts";
import * as DateTime from "effect/DateTime";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as DeviceService from "./DeviceService.ts";
import * as Registration from "./SimboxDeviceRegistration.ts";

const id = "f1234567-1234-1234-1234-123456789abc";
const marker = `[simbox-device:${id}]`;
const threadId = ThreadId.make("thread-a");
const descriptor = () => ({
  version: 1,
  createdAt: Date.now(),
  runId: "039dd761-9144-4a9c-a03b-e44c893d73ad",
  tunnelUrl: "https://fixture.trycloudflare.com",
  daemonToken: "private-daemon-token",
  expiresAt: null,
  stop: false,
  autoBoot: true,
  apiUrl: "https://api.simbox.touchtech.club",
  userToken: "private-user-token",
});
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
const cleanup: Array<() => Promise<void>> = [];
afterEach(async () => {
  for (const fn of cleanup.splice(0)) await fn();
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

  it.live(
    "consumes a private receipt once, binds to the originating thread, and never trusts credentials from output",
    () =>
      Effect.gen(function* () {
        const directory = yield* Effect.promise(() =>
          NodeFSP.mkdtemp(NodePath.join(NodeOS.tmpdir(), "s5-simbox-registration-")),
        );
        const previous = process.env.SIMBOX_DEVICE_REGISTRATION_DIR;
        process.env.SIMBOX_DEVICE_REGISTRATION_DIR = directory;
        cleanup.push(async () => {
          if (previous === undefined) delete process.env.SIMBOX_DEVICE_REGISTRATION_DIR;
          else process.env.SIMBOX_DEVICE_REGISTRATION_DIR = previous;
          await NodeFSP.rm(directory, { recursive: true, force: true });
        });
        const file = NodePath.join(directory, `${id}.json`);
        yield* Effect.promise(() =>
          NodeFSP.writeFile(file, JSON.stringify(descriptor()), { mode: 0o600 }),
        );
        const calls: Array<{ threadId: string; token: string }> = [];
        yield* Effect.gen(function* () {
          const registration = yield* Registration.SimboxDeviceRegistration;
          const event = item({ content: [{ text: marker }] }, "dynamic_tool");
          yield* registration.observe(event);
          yield* registration.observe({ ...event, threadId: ThreadId.make("thread-b") });
        }).pipe(
          Effect.provide(
            Registration.layer.pipe(
              Layer.provide(NodeServices.layer),
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
        expect(yield* Effect.promise(() => NodeFSP.readdir(directory))).toEqual([]);
        yield* Effect.promise(() => expect(NodeFSP.readFile(file)).rejects.toThrow());
      }),
  );

  it.live("discards expired receipts without attaching a device", () =>
    Effect.gen(function* () {
      const directory = yield* Effect.promise(() =>
        NodeFSP.mkdtemp(NodePath.join(NodeOS.tmpdir(), "s5-simbox-expired-")),
      );
      const previous = process.env.SIMBOX_DEVICE_REGISTRATION_DIR;
      process.env.SIMBOX_DEVICE_REGISTRATION_DIR = directory;
      cleanup.push(async () => {
        if (previous === undefined) delete process.env.SIMBOX_DEVICE_REGISTRATION_DIR;
        else process.env.SIMBOX_DEVICE_REGISTRATION_DIR = previous;
        await NodeFSP.rm(directory, { recursive: true, force: true });
      });
      yield* Effect.promise(() =>
        NodeFSP.writeFile(
          NodePath.join(directory, `${id}.json`),
          JSON.stringify({ ...descriptor(), createdAt: Date.now() - 600_000 }),
        ),
      );
      let called = false;
      yield* Effect.gen(function* () {
        yield* (yield* Registration.SimboxDeviceRegistration).observe(item(marker));
      }).pipe(
        Effect.provide(
          Registration.layer.pipe(
            Layer.provide(NodeServices.layer),
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
      expect(yield* Effect.promise(() => NodeFSP.readdir(directory))).toEqual([]);
    }),
  );
});
