import * as NodeServices from "@effect/platform-node/NodeServices";
import { describe, expect, it } from "@effect/vitest";
import {
  ApprovalRequestId,
  ProviderInstanceId,
  ProviderRuntimeEvent,
  ThreadId,
  TurnId,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as Queue from "effect/Queue";
import * as Stream from "effect/Stream";

import { makeOpenCode2Adapter } from "./OpenCode2Adapter.ts";
import { type OpenCode2ClientFacade, type OpenCode2HostHandle } from "../OpenCode2Host.ts";

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
  } = {
    prompts: [],
    interrupts: [],
    forks: [],
    inboxCancels: [],
    inboxChanges: [],
    permissionReplies: [],
    formReplies: [],
    reverts: [],
  };

  const client = {
    event: {
      subscribe: async () => ({
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
    session: {
      create: async (params: any) => ({
        id: `mock-session-${Date.now()}`,
        directory: params.directory,
      }),
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
        return { id: `mock-forked-${Date.now()}` };
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
        directory: "/tmp/project",
      });

      expect(session.sessionId).toContain("mock-session");

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
        data: { sessionID: session.sessionId },
      });
      emit({
        type: "session.text.delta",
        data: { sessionID: session.sessionId, delta: "Hi there!" },
      });
      emit({
        type: "session.execution.succeeded",
        data: { sessionID: session.sessionId },
      });

      const textDelta = yield* waitForEvent((e) => e.type === "thread.token.delta");
      expect((textDelta.payload as any).delta).toBe("Hi there!");

      const completed = yield* waitForEvent((e) => e.type === "turn.completed");
      expect((completed.payload as any).status).toBe("completed");
      expect((completed.payload as any).hasSubagents).toBe(false);
    }).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
  );

  it.effect("translates child session events into rich subagent tasks and rolls up tokens", () =>
    Effect.gen(function* () {
      const { handle, emit } = createMockHost();
      const adapter = yield* makeOpenCode2Adapter(handle);

      const threadId = ThreadId.make("thread-subagent");
      const session = yield* adapter.startSession({
        threadId,
        directory: "/tmp/project",
      });

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
        data: { sessionID: childSessionId, parentID: session.sessionId, agent: "reviewer" },
      });

      const started = yield* waitForEvent((e) => e.type === "task.started");
      expect((started.payload as any).taskType).toBe("subagent");
      expect((started.payload as any).agentKind).toBe("agent");
      expect((started.payload as any).description).toBe("Subagent: reviewer");

      // Subagent calls tool
      emit({
        type: "session.tool.called",
        data: { sessionID: childSessionId, parentID: session.sessionId, tool: "checkDiff" },
      });

      const toolProgress = yield* waitForEvent((e) => e.type === "task.progress");
      expect((toolProgress.payload as any).lastToolName).toBe("checkDiff");

      // Subagent finishes step with token metrics
      emit({
        type: "session.step.ended",
        data: {
          sessionID: childSessionId,
          parentID: session.sessionId,
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
        data: { sessionID: childSessionId, parentID: session.sessionId },
      });

      const taskCompleted = yield* waitForEvent((e) => e.type === "task.completed");
      expect((taskCompleted.payload as any).status).toBe("completed");

      // Parent turn finishes
      emit({
        type: "session.execution.succeeded",
        data: { sessionID: session.sessionId },
      });

      const turnCompleted = yield* waitForEvent((e) => e.type === "turn.completed");
      expect((turnCompleted.payload as any).hasSubagents).toBe(true);
    }).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
  );

  it.effect("handles permissions and form input requests and replies cleanly", () =>
    Effect.gen(function* () {
      const { handle, emit, calls } = createMockHost();
      const adapter = yield* makeOpenCode2Adapter(handle);

      const threadId = ThreadId.make("thread-perm");
      const session = yield* adapter.startSession({
        threadId,
        directory: "/tmp/project",
      });

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
          sessionID: session.sessionId,
          permissionID: "perm-123",
          description: "Allow bash execution?",
        },
      });

      const permEvent = yield* waitForEvent(
        (e) => e.type === "request.opened" && (e.payload as any).action === "permission",
      );
      expect((permEvent.payload as any).requestId).toBe(ApprovalRequestId.make("perm-123"));

      // Form created
      emit({
        type: "form.created",
        data: {
          sessionID: session.sessionId,
          formID: "form-456",
          title: "Choose environment",
          fields: [{ name: "env", type: "string" }],
        },
      });

      const formEvent = yield* waitForEvent(
        (e) => e.type === "request.opened" && (e.payload as any).action === "user-input",
      );
      expect((formEvent.payload as any).requestId).toBe(ApprovalRequestId.make("form-456"));

      // Reply to permission
      yield* adapter.respondToRequest(threadId, ApprovalRequestId.make("perm-123"), "accept");
      expect(calls.permissionReplies).toContainEqual({
        sessionID: session.sessionId,
        permissionID: "perm-123",
        response: "once",
      });

      // Reply to form
      yield* adapter.respondToUserInput(threadId, ApprovalRequestId.make("form-456"), {
        env: "production",
      });
      expect(calls.formReplies).toContainEqual({
        sessionID: session.sessionId,
        formID: "form-456",
        answers: { env: "production" },
      });
    }).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
  );

  it.effect("supports durable inbox cancellation, delivery changing, and session forks", () =>
    Effect.gen(function* () {
      const { handle, calls } = createMockHost();
      const adapter = yield* makeOpenCode2Adapter(handle);

      const threadId = ThreadId.make("thread-inbox");
      const session = yield* adapter.startSession({
        threadId,
        directory: "/tmp/project",
      });

      // Send queued turn
      yield* adapter.sendTurn({
        threadId,
        input: "Queue this task",
        delivery: "queue",
      });
      expect(calls.prompts[0].delivery).toBe("queue");

      // Cancel inbox item
      yield* adapter.cancelInboxItem!(threadId, "inbox-item-1");
      expect(calls.inboxCancels).toContainEqual({
        sessionID: session.sessionId,
        inboxID: "inbox-item-1",
      });

      // Change delivery
      yield* adapter.changeInboxDelivery!(threadId, "inbox-item-2", "steer");
      expect(calls.inboxChanges).toContainEqual({
        steer: { sessionID: session.sessionId, inboxID: "inbox-item-2" },
      });

      // Fork thread
      const targetThreadId = ThreadId.make("thread-forked-target");
      const forkResult = yield* adapter.forkThread!(
        threadId,
        targetThreadId,
        TurnId.make("turn-1"),
      );

      expect(calls.forks).toHaveLength(1);
      expect(calls.forks[0].sessionID).toBe(session.sessionId);
      expect((forkResult.resumeCursor as any).sessionID).toContain("mock-forked");

      // Verify forked session is registered and usable
      expect(yield* adapter.hasSession(targetThreadId)).toBe(true);
    }).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
  );
});
