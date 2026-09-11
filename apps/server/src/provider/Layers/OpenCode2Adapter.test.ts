import * as NodeServices from "@effect/platform-node/NodeServices";
import { describe, expect, it } from "@effect/vitest";
import {
  ApprovalRequestId,
  ProviderInstanceId,
  ProviderRuntimeEvent,
  ThreadId,
  TurnId,
} from "@t3tools/contracts";
import * as Clock from "effect/Clock";
import * as Effect from "effect/Effect";
import * as Queue from "effect/Queue";
import * as Stream from "effect/Stream";
import * as Layer from "effect/Layer";

import { ServerConfig } from "../../config.ts";
import { makeOpenCode2Adapter } from "./OpenCode2Adapter.ts";
import { type OpenCode2ClientFacade, type OpenCode2HostHandle } from "../OpenCode2Host.ts";

const testLayer = Layer.merge(
  NodeServices.layer,
  ServerConfig.layerTest(process.cwd(), process.cwd()).pipe(Layer.provide(NodeServices.layer)),
);

function createMockHost() {
  const buffer: Array<{ type: string; data?: any }> = [];
  const waiters: Array<() => void> = [];

  const emit = (event: { type: string; data?: any }) => {
    buffer.push(event);
    const w = waiters.shift();
    if (w) w();
  };

  const calls: {
    prompts: Array<any>;
    interrupts: Array<any>;
    forks: Array<any>;
    inboxCancels: Array<any>;
    inboxChanges: Array<any>;
    permissionReplies: Array<any>;
    formReplies: Array<any>;
    reverts: Array<any>;
    switchModels: Array<any>;
    switchAgents: Array<any>;
  } = {
    prompts: [],
    interrupts: [],
    forks: [],
    inboxCancels: [],
    inboxChanges: [],
    permissionReplies: [],
    formReplies: [],
    reverts: [],
    switchModels: [],
    switchAgents: [],
  };

  const client = {
    event: {
      subscribe: () => ({
        [Symbol.asyncIterator]: () => ({
          next: async () => {
            while (buffer.length === 0) {
              await new Promise<void>((resolve) => {
                waiters.push(resolve);
              });
            }
            return { done: false, value: buffer.shift()! };
          },
        }),
      }),
    },
    model: {
      list: async () => ({
        data: [
          {
            id: "glm-5.3",
            providerID: "zhipu",
            variants: [{ id: "high" }, { id: "medium" }],
          },
          {
            id: "Qwen3.8-27B",
            providerID: "hetzner",
            variants: [],
          },
          {
            id: "z-ai/glm-5.3-flash",
            providerID: "cline",
            variants: [],
          },
        ],
      }),
    },
    session: {
      create: async (params: any) => ({
        id: `mock-session-123`,
        directory: params.location?.directory ?? "/tmp",
      }),
      switchModel: async (params: any) => {
        calls.switchModels.push(params);
      },
      switchAgent: async (params: any) => {
        calls.switchAgents.push(params);
      },
      prompt: async (params: any) => {
        calls.prompts.push(params);
        return { id: "prompt-1" };
      },
      interrupt: async (params: any) => {
        calls.interrupts.push(params);
      },
      compact: async (_params: any) => {},
      fork: async (params: any) => {
        calls.forks.push(params);
        return { id: `mock-forked-456` };
      },
      revert: {
        stage: async (params: any) => {
          calls.reverts.push({ stage: params });
        },
        commit: async (params: any) => {
          calls.reverts.push({ commit: params });
        },
      },
      inbox: {
        cancel: async (params: any) => {
          calls.inboxCancels.push(params);
        },
        steer: async (params: any) => {
          calls.inboxChanges.push({ steer: params });
        },
        queue: async (params: any) => {
          calls.inboxChanges.push({ queue: params });
        },
      },
    },
    permission: {
      reply: async (params: any) => {
        calls.permissionReplies.push(params);
      },
    },
    form: {
      reply: async (params: any) => {
        calls.formReplies.push(params);
      },
    },
  } as unknown as OpenCode2ClientFacade;

  const handle: OpenCode2HostHandle = {
    instanceId: ProviderInstanceId.make("opencode2-test"),
    client,
    isRemote: false,
    databasePath: null,
  };

  return { handle, emit, calls };
}

describe("OpenCode2Adapter", () => {
  it.effect("starts session, sends turn, and streams text deltas and turn completion", () =>
    Effect.gen(function* () {
      const { handle, emit, calls } = createMockHost();
      const adapter = yield* makeOpenCode2Adapter(handle);

      const threadId = ThreadId.make("thread-1");
      const session = yield* adapter.startSession({
        threadId,
        cwd: "/tmp/project",
        runtimeMode: "full-access",
      });

      const sessionId = (session.resumeCursor as { sessionID: string }).sessionID;
      expect(sessionId).toContain("mock-session");

      const canonicalEvents = yield* Queue.unbounded<ProviderRuntimeEvent>();
      yield* Effect.forkScoped(
        Stream.runForEach(adapter.streamEvents, (event) => Queue.offer(canonicalEvents, event)),
      );

      const waitForEvent = Effect.fn("waitForEvent")(function* (
        predicate: (event: ProviderRuntimeEvent) => boolean,
      ) {
        while (true) {
          const event = yield* Queue.take(canonicalEvents);
          if (predicate(event)) return event;
        }
      });

      // Send turn
      yield* adapter.sendTurn({
        threadId,
        input: "Hello OpenCode",
        delivery: "steer",
      });

      expect(calls.prompts).toHaveLength(1);
      expect(calls.prompts[0].text).toBe("Hello OpenCode");
      expect(calls.prompts[0].delivery).toBe("steer");

      // Emit execution started, text delta, and execution succeeded
      emit({
        type: "session.execution.started",
        data: { sessionID: sessionId },
      });
      emit({
        type: "session.text.delta",
        data: { sessionID: sessionId, delta: "Hi there!" },
      });
      emit({
        type: "session.execution.succeeded",
        data: { sessionID: sessionId },
      });

      const textDelta = yield* waitForEvent((e) => e.type === "content.delta");
      expect((textDelta.payload as any).delta).toBe("Hi there!");

      const completed = yield* waitForEvent((e) => e.type === "turn.completed");
      expect((completed.payload as any).state).toBe("completed");
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("translates child session events into rich subagent tasks and rolls up tokens", () =>
    Effect.gen(function* () {
      const { handle, emit } = createMockHost();
      const adapter = yield* makeOpenCode2Adapter(handle);

      const threadId = ThreadId.make("thread-subagent");
      const session = yield* adapter.startSession({
        threadId,
        cwd: "/tmp/project",
        runtimeMode: "full-access",
      });

      const sessionId = (session.resumeCursor as { sessionID: string }).sessionID;

      const canonicalEvents = yield* Queue.unbounded<ProviderRuntimeEvent>();
      yield* Effect.forkScoped(
        Stream.runForEach(adapter.streamEvents, (event) => Queue.offer(canonicalEvents, event)),
      );

      const waitForEvent = Effect.fn("waitForEvent")(function* (
        predicate: (event: ProviderRuntimeEvent) => boolean,
      ) {
        while (true) {
          const event = yield* Queue.take(canonicalEvents);
          if (predicate(event)) return event;
        }
      });

      yield* adapter.sendTurn({
        threadId,
        input: "Run reviewer",
      });

      const childSessionId = "child-session-99";

      // Subagent is created under the parent session
      emit({
        type: "session.created",
        data: { sessionID: childSessionId, parentID: sessionId, agent: "reviewer" },
      });

      const started = yield* waitForEvent((e) => e.type === "task.started");
      expect((started.payload as any).taskType).toBe("subagent");
      expect((started.payload as any).agentKind).toBe("agent");
      expect((started.payload as any).description).toBe("Subagent: reviewer");

      // Subagent calls tool
      emit({
        type: "session.tool.called",
        data: { sessionID: childSessionId, parentID: sessionId, tool: "checkDiff" },
      });

      const toolProgress = yield* waitForEvent((e) => e.type === "task.progress");
      expect((toolProgress.payload as any).lastToolName).toBe("checkDiff");

      // Subagent finishes step with token metrics
      emit({
        type: "session.step.ended",
        data: {
          sessionID: childSessionId,
          parentID: sessionId,
          tokens: { input: 150, output: 50, reasoning: 30 },
        },
      });

      const tokenProgress = yield* waitForEvent(
        (e) => e.type === "task.progress" && (e.payload as any).typedUsage,
      );
      expect((tokenProgress.payload as any).typedUsage.totalTokens).toBe(200);
      expect((tokenProgress.payload as any).typedUsage.reasoningOutputTokens).toBe(30);

      // Subagent completes
      emit({
        type: "session.execution.succeeded",
        data: { sessionID: childSessionId, parentID: sessionId },
      });

      const taskCompleted = yield* waitForEvent((e) => e.type === "task.completed");
      expect((taskCompleted.payload as any).status).toBe("completed");

      // Parent turn finishes
      emit({
        type: "session.execution.succeeded",
        data: { sessionID: sessionId },
      });

      const turnCompleted = yield* waitForEvent((e) => e.type === "turn.completed");
      expect((turnCompleted.payload as any).state).toBe("completed");
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("handles permissions and form input requests and replies cleanly", () =>
    Effect.gen(function* () {
      const { handle, emit, calls } = createMockHost();
      const adapter = yield* makeOpenCode2Adapter(handle);

      const threadId = ThreadId.make("thread-perm");
      const session = yield* adapter.startSession({
        threadId,
        cwd: "/tmp/project",
        runtimeMode: "full-access",
      });

      const sessionId = (session.resumeCursor as { sessionID: string }).sessionID;

      const canonicalEvents = yield* Queue.unbounded<ProviderRuntimeEvent>();
      yield* Effect.forkScoped(
        Stream.runForEach(adapter.streamEvents, (event) => Queue.offer(canonicalEvents, event)),
      );

      const waitForEvent = Effect.fn("waitForEvent")(function* (
        predicate: (event: ProviderRuntimeEvent) => boolean,
      ) {
        while (true) {
          const event = yield* Queue.take(canonicalEvents);
          if (predicate(event)) return event;
        }
      });

      // Permission asked
      emit({
        type: "permission.asked",
        data: {
          sessionID: sessionId,
          permissionID: "perm-123",
          description: "Allow bash execution?",
        },
      });

      const permEvent = yield* waitForEvent(
        (e) =>
          e.type === "request.opened" &&
          (e.payload as any).requestType === "command_execution_approval",
      );
      expect(permEvent).toBeDefined();

      // Form created (v2 nests the form under `data.form`)
      emit({
        type: "form.created",
        data: {
          form: {
            id: "form-456",
            sessionID: sessionId,
            title: "Choose environment",
            fields: [
              {
                key: "env",
                type: "string",
                title: "Environment",
                custom: true,
                options: [{ value: "production", label: "Production" }],
              },
            ],
          },
        },
      });

      const formEvent = yield* waitForEvent((e) => e.type === "user-input.requested");
      const questions = (formEvent.payload as any).questions;
      expect(questions).toHaveLength(1);
      expect(questions[0].id).toBe("env");
      expect(questions[0].header).toBe("Environment");
      expect(questions[0].question).toBe("Environment");
      expect(questions[0].options).toEqual([
        { label: "Production", description: "", value: "production" },
      ]);

      // Reply to permission
      yield* adapter.respondToRequest(threadId, ApprovalRequestId.make("perm-123"), "accept");
      expect(calls.permissionReplies).toContainEqual({
        sessionID: sessionId,
        requestID: "perm-123",
        reply: "once",
      });

      // Reply to form
      yield* adapter.respondToUserInput(threadId, ApprovalRequestId.make("form-456"), {
        env: "production",
      });
      expect(calls.formReplies).toContainEqual({
        sessionID: sessionId,
        formID: "form-456",
        answer: { env: "production" },
      });
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("supports durable inbox cancellation, delivery changing, and session forks", () =>
    Effect.gen(function* () {
      const { handle, emit, calls } = createMockHost();
      const adapter = yield* makeOpenCode2Adapter(handle);

      const threadId = ThreadId.make("thread-inbox");
      const session = yield* adapter.startSession({
        threadId,
        cwd: "/tmp/project",
        runtimeMode: "full-access",
      });

      const sessionId = (session.resumeCursor as { sessionID: string }).sessionID;

      const turnResult = yield* adapter.sendTurn({
        threadId,
        input: "Queue this task",
        delivery: "queue",
      });
      expect(calls.prompts[0].delivery).toBe("queue");

      const canonicalEvents = yield* Queue.unbounded<ProviderRuntimeEvent>();
      yield* Effect.forkScoped(
        Stream.runForEach(adapter.streamEvents, (event) => Queue.offer(canonicalEvents, event)),
      );

      const waitForEvent = Effect.fn("waitForEvent")(function* (
        predicate: (event: ProviderRuntimeEvent) => boolean,
      ) {
        while (true) {
          const event = yield* Queue.take(canonicalEvents);
          if (predicate(event)) return event;
        }
      });

      // Record a message for this turn
      emit({
        type: "session.text.delta",
        data: {
          sessionID: sessionId,
          assistantMessageID: "msg-123",
          delta: "queued ack",
        },
      });

      yield* waitForEvent((e) => e.type === "content.delta");

      // Cancel inbox item
      yield* adapter.cancelInboxItem!(threadId, "inbox-item-1");
      expect(calls.inboxCancels).toContainEqual({
        sessionID: sessionId,
        inboxID: "inbox-item-1",
      });

      // Change delivery
      yield* adapter.changeInboxDelivery!(threadId, "inbox-item-2", "steer");
      expect(calls.inboxChanges).toContainEqual({
        steer: { sessionID: sessionId, inboxID: "inbox-item-2" },
      });

      // Fork thread with mapped turn
      const targetThreadId = ThreadId.make("thread-forked-target");
      const forkResult = yield* adapter.forkThread!(threadId, targetThreadId, turnResult.turnId);

      expect(calls.forks).toHaveLength(1);
      expect(calls.forks[0].sessionID).toBe(sessionId);
      expect(calls.forks[0].boundary).toEqual({
        type: "through",
        messageID: "msg-123",
      });
      expect((forkResult.resumeCursor as any).sessionID).toContain("mock-forked");

      // Verify forked session is registered and usable
      expect(yield* adapter.hasSession(targetThreadId)).toBe(true);
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("switches the session model and agent from the model selection before prompting", () =>
    Effect.gen(function* () {
      const { handle, calls } = createMockHost();
      const adapter = yield* makeOpenCode2Adapter(handle);

      const threadId = ThreadId.make("thread-model");
      yield* adapter.startSession({
        threadId,
        cwd: "/tmp/project",
        runtimeMode: "full-access",
      });

      yield* adapter.sendTurn({
        threadId,
        input: "Hello",
        modelSelection: {
          instanceId: "opencode2",
          model: "zhipu/glm-5.3",
          options: [
            { id: "variant", value: "high" },
            { id: "agent", value: "Build" },
          ],
        } as any,
      });

      expect(calls.switchModels).toEqual([
        {
          sessionID: "mock-session-123",
          model: { id: "glm-5.3", providerID: "zhipu", variant: "high" },
        },
      ]);
      expect(calls.switchAgents).toEqual([{ sessionID: "mock-session-123", agent: "build" }]);
      expect(calls.prompts).toHaveLength(1);
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("omits variant when target model does not support variants", () =>
    Effect.gen(function* () {
      const { handle, calls } = createMockHost();
      const adapter = yield* makeOpenCode2Adapter(handle);

      const threadId = ThreadId.make("thread-model-no-variant");
      yield* adapter.startSession({
        threadId,
        cwd: "/tmp/project",
        runtimeMode: "full-access",
      });

      // Even if variant 'high' is present in model selection options,
      // it must not be sent to OpenCode 2 for models with no variants.
      yield* adapter.sendTurn({
        threadId,
        input: "Hello",
        modelSelection: {
          instanceId: "opencode2",
          model: "hetzner/Qwen3.8-27B",
          options: [
            { id: "variant", value: "high" },
            { id: "agent", value: "build" },
          ],
        } as any,
      });

      expect(calls.switchModels).toEqual([
        {
          sessionID: "mock-session-123",
          model: { id: "Qwen3.8-27B", providerID: "hetzner" },
        },
      ]);
      expect(calls.prompts).toHaveLength(1);
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect(
    "resolves models with slashed IDs like z-ai/glm-5.3-flash to their correct provider",
    () =>
      Effect.gen(function* () {
        const { handle, calls } = createMockHost();
        const adapter = yield* makeOpenCode2Adapter(handle);

        const threadId = ThreadId.make("thread-model-slashed-id");
        yield* adapter.startSession({
          threadId,
          cwd: "/tmp/project",
          runtimeMode: "full-access",
        });

        // Selection may be 'cline/z-ai/glm-5.3-flash' or 'z-ai/glm-5.3-flash'
        yield* adapter.sendTurn({
          threadId,
          input: "Hello GLM",
          modelSelection: {
            instanceId: "opencode2",
            model: "z-ai/glm-5.3-flash",
            options: [{ id: "agent", value: "build" }],
          } as any,
        });

        expect(calls.switchModels).toEqual([
          {
            sessionID: "mock-session-123",
            model: { id: "z-ai/glm-5.3-flash", providerID: "cline" },
          },
        ]);
        expect(calls.prompts).toHaveLength(1);
      }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("replies to a permission asked by a subagent's child session", () =>
    Effect.gen(function* () {
      const { handle, emit, calls } = createMockHost();
      const adapter = yield* makeOpenCode2Adapter(handle);

      const threadId = ThreadId.make("thread-child-perm");
      const session = yield* adapter.startSession({
        threadId,
        cwd: "/tmp/project",
        runtimeMode: "full-access",
      });
      const sessionId = (session.resumeCursor as { sessionID: string }).sessionID;
      const childSessionId = "ses_child_1";

      const canonicalEvents = yield* Queue.unbounded<ProviderRuntimeEvent>();
      yield* Effect.forkScoped(
        Stream.runForEach(adapter.streamEvents, (event) => Queue.offer(canonicalEvents, event)),
      );
      const waitForEvent = Effect.fn("waitForEvent")(function* (
        predicate: (event: ProviderRuntimeEvent) => boolean,
      ) {
        while (true) {
          const event = yield* Queue.take(canonicalEvents);
          if (predicate(event)) return event;
        }
      });

      // Register the child session under the parent. Subagents only run
      // inside a turn, so send one first.
      yield* adapter.sendTurn({ threadId, input: "run reviewer" });

      emit({
        type: "session.created",
        data: { sessionID: childSessionId, parentID: sessionId, agent: "reviewer" },
      });
      yield* waitForEvent((e) => e.type === "task.started");

      // The child session owns the permission request
      emit({
        type: "permission.asked",
        data: {
          id: "per_child_1",
          sessionID: childSessionId,
          action: "shell",
          resources: ["rm -rf /"],
          message: "Allow shell command?",
        },
      });

      const permEvent = yield* waitForEvent((e) => e.type === "request.opened");
      expect((permEvent.payload as any).detail).toBe("Allow shell command?");

      yield* adapter.respondToRequest(threadId, ApprovalRequestId.make("per_child_1"), "accept");
      expect(calls.permissionReplies).toEqual([
        { sessionID: childSessionId, requestID: "per_child_1", reply: "once" },
      ]);
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("carries tool names from input.started into tool items and titles them by input", () =>
    Effect.gen(function* () {
      const { handle, emit } = createMockHost();
      const adapter = yield* makeOpenCode2Adapter(handle);

      const threadId = ThreadId.make("thread-tool-names");
      const session = yield* adapter.startSession({
        threadId,
        cwd: "/tmp/project",
        runtimeMode: "full-access",
      });
      const sessionId = (session.resumeCursor as { sessionID: string }).sessionID;

      const canonicalEvents = yield* Queue.unbounded<ProviderRuntimeEvent>();
      yield* Effect.forkScoped(
        Stream.runForEach(adapter.streamEvents, (event) => Queue.offer(canonicalEvents, event)),
      );
      const waitForEvent = Effect.fn("waitForEvent")(function* (
        predicate: (event: ProviderRuntimeEvent) => boolean,
      ) {
        while (true) {
          const event = yield* Queue.take(canonicalEvents);
          if (predicate(event)) return event;
        }
      });

      yield* adapter.sendTurn({ threadId, input: "list files" });

      emit({
        type: "session.tool.input.started",
        data: { sessionID: sessionId, assistantMessageID: "msg-1", callID: "call-1", name: "bash" },
      });
      emit({
        type: "session.tool.called",
        data: {
          sessionID: sessionId,
          assistantMessageID: "msg-1",
          callID: "call-1",
          input: { command: "ls -la" },
          executed: true,
        },
      });
      emit({
        type: "session.tool.success",
        data: {
          sessionID: sessionId,
          assistantMessageID: "msg-1",
          callID: "call-1",
          content: [{ type: "text", text: "file-a\nfile-b" }],
          executed: true,
        },
      });

      const started = yield* waitForEvent((e) => e.type === "item.started");
      expect((started.payload as any).title).toBe("ls -la");
      expect((started.payload as any).itemType).toBe("command_execution");
      expect((started.payload as any).data.tool).toBe("bash");
      expect((started.payload as any).data.command).toBe("ls -la");
      expect(started.turnId).not.toBeNull();

      const completed = yield* waitForEvent((e) => e.type === "item.completed");
      expect((completed.payload as any).status).toBe("completed");
      expect((completed.payload as any).title).toBe("ls -la");
      expect((completed.payload as any).detail).toBe("file-a\nfile-b");
      expect((completed.payload as any).data.tool).toBe("bash");
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("attributes late events after a completed turn to that turn, not a synthetic one", () =>
    Effect.gen(function* () {
      const { handle, emit } = createMockHost();
      const adapter = yield* makeOpenCode2Adapter(handle);

      const threadId = ThreadId.make("thread-stragglers");
      const session = yield* adapter.startSession({
        threadId,
        cwd: "/tmp/project",
        runtimeMode: "full-access",
      });
      const sessionId = (session.resumeCursor as { sessionID: string }).sessionID;

      const canonicalEvents = yield* Queue.unbounded<ProviderRuntimeEvent>();
      yield* Effect.forkScoped(
        Stream.runForEach(adapter.streamEvents, (event) => Queue.offer(canonicalEvents, event)),
      );
      const waitForEvent = Effect.fn("waitForEvent")(function* (
        predicate: (event: ProviderRuntimeEvent) => boolean,
      ) {
        while (true) {
          const event = yield* Queue.take(canonicalEvents);
          if (predicate(event)) return event;
        }
      });

      yield* adapter.sendTurn({ threadId, input: "do work" });

      emit({
        type: "session.tool.input.started",
        data: { sessionID: sessionId, assistantMessageID: "msg-1", callID: "call-1", name: "bash" },
      });
      emit({
        type: "session.tool.called",
        data: {
          sessionID: sessionId,
          assistantMessageID: "msg-1",
          callID: "call-1",
          input: { command: "git status" },
          executed: true,
        },
      });
      emit({ type: "session.execution.succeeded", data: { sessionID: sessionId } });

      const completed = yield* waitForEvent((e) => e.type === "turn.completed");
      const settledTurnId = completed.turnId;

      // A tool result trailing the closed turn stays on that turn.
      emit({
        type: "session.tool.success",
        data: {
          sessionID: sessionId,
          assistantMessageID: "msg-1",
          callID: "call-1",
          content: [{ type: "text", text: "clean" }],
          executed: true,
        },
      });

      const lateItem = yield* waitForEvent((e) => e.type === "item.completed");
      expect(lateItem.turnId).toBe(settledTurnId);
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("rejects attachments when connected to a remote OpenCode host", () =>
    Effect.gen(function* () {
      const { handle } = createMockHost();
      (handle as any).isRemote = true;
      const adapter = yield* makeOpenCode2Adapter(handle);

      const threadId = ThreadId.make("thread-remote-attachments");
      yield* adapter.startSession({
        threadId,
        cwd: "/tmp/project",
        runtimeMode: "full-access",
      });

      const exit = yield* adapter
        .sendTurn({
          threadId,
          input: "Process image",
          attachments: [
            {
              type: "file",
              id: "att-1" as any,
              name: "file.txt",
              mimeType: "text/plain",
              sizeBytes: 100,
            },
          ],
        })
        .pipe(Effect.exit);

      expect(exit._tag).toBe("Failure");
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );
});
