import * as NodeServices from "@effect/platform-node/NodeServices";
import { describe, expect, it } from "@effect/vitest";
import {
  ApprovalRequestId,
  ProviderInstanceId,
  ProviderRuntimeEvent,
  ThreadId,
  TurnId,
} from "@t3tools/contracts";
import * as Cause from "effect/Cause";
import * as Clock from "effect/Clock";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import * as Queue from "effect/Queue";
import * as Stream from "effect/Stream";
import * as Layer from "effect/Layer";

import { ServerConfig } from "../../config.ts";
import { makeOpenCodeAdapter, openCodeClientErrorMessage } from "./OpenCodeAdapter.ts";
import { type OpenCodeClientFacade, type OpenCodeHostHandle } from "../OpenCodeHost.ts";

const testLayer = Layer.merge(
  NodeServices.layer,
  ServerConfig.layerTest(process.cwd(), process.cwd()).pipe(Layer.provide(NodeServices.layer)),
);

function createMockHost() {
  const buffer: Array<{ type: string; data?: any }> = [];
  const waiters: Array<() => void> = [];

  // The daemon's durable message rows, keyed by session id. Rollback reads
  // this history to pick a boundary, so the mock mirrors the rows a real
  // daemon records: a user/synthetic row per delivered inbox item and an
  // assistant row per assistant message id.
  const sessionMessages = new Map<string, Array<{ id: string; type: string }>>();
  const pendingInputTypes = new Map<string, Map<string, string>>();
  const stagedReverts = new Map<string, string>();
  let sessionCount = 0;

  const messagesFor = (sessionId: string) => {
    let rows = sessionMessages.get(sessionId);
    if (!rows) {
      rows = [];
      sessionMessages.set(sessionId, rows);
    }
    return rows;
  };

  const pushMessage = (sessionId: string, id: string, type: string) => {
    const rows = messagesFor(sessionId);
    if (!rows.some((row) => row.id === id)) {
      rows.push({ id, type });
    }
  };

  const emit = (event: { type: string; data?: any }) => {
    const data = event.data ?? {};
    const sessionId = data.sessionID ?? data.form?.sessionID ?? data.info?.id;
    if (typeof sessionId === "string") {
      if (event.type === "session.created") {
        messagesFor(sessionId);
      } else if (typeof data.assistantMessageID === "string") {
        pushMessage(sessionId, data.assistantMessageID, "assistant");
      } else if (event.type === "session.inbox.enqueued") {
        const inboxId = typeof data.inboxID === "string" ? data.inboxID : undefined;
        const kind = typeof data.item?.type === "string" ? data.item.type : undefined;
        if (inboxId && kind) {
          let inputs = pendingInputTypes.get(sessionId);
          if (!inputs) {
            inputs = new Map();
            pendingInputTypes.set(sessionId, inputs);
          }
          inputs.set(inboxId, kind);
        }
      } else if (event.type === "session.inbox.delivered") {
        const inboxId =
          typeof data.inboxID === "string"
            ? data.inboxID
            : typeof data.messageID === "string"
              ? data.messageID
              : undefined;
        if (inboxId) {
          const type = pendingInputTypes.get(sessionId)?.get(inboxId) ?? "user";
          pendingInputTypes.get(sessionId)?.delete(inboxId);
          pushMessage(sessionId, inboxId, type);
        }
      }
    }
    buffer.push(event);
    const w = waiters.shift();
    if (w) w();
  };

  const calls: {
    prompts: Array<any>;
    interrupts: Array<any>;
    waits: Array<any>;
    forks: Array<any>;
    inboxCancels: Array<any>;
    inboxChanges: Array<any>;
    permissionReplies: Array<any>;
    formReplies: Array<any>;
    reverts: Array<any>;
    switchModels: Array<any>;
    switchAgents: Array<any>;
    creates: Array<any>;
    removes: Array<any>;
  } = {
    prompts: [],
    interrupts: [],
    waits: [],
    forks: [],
    inboxCancels: [],
    inboxChanges: [],
    permissionReplies: [],
    formReplies: [],
    reverts: [],
    switchModels: [],
    switchAgents: [],
    creates: [],
    removes: [],
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
    message: {
      list: async (params: any) => ({
        data: [...messagesFor(params.sessionID)],
        cursor: { next: null },
      }),
    },
    session: {
      create: async (params: any) => {
        calls.creates.push(params);
        sessionCount += 1;
        const id = sessionCount === 1 ? "mock-session-123" : `mock-session-${122 + sessionCount}`;
        sessionMessages.set(id, []);
        return {
          id,
          directory: params.location?.directory ?? "/tmp",
          time: { created: 1_700_000_000_000 + sessionCount },
        };
      },
      get: async (params: any) => {
        if (!sessionMessages.has(params.sessionID)) {
          throw new Error("Session not found");
        }
        return { id: params.sessionID, time: { created: 1_700_000_000_000 } };
      },
      remove: async (params: any) => {
        calls.removes.push(params);
        sessionMessages.delete(params.sessionID);
      },
      message: async (params: any) => {
        const row = sessionMessages
          .get(params.sessionID)
          ?.find((message) => message.id === params.messageID);
        if (!row) {
          throw new Error("Message not found");
        }
        return row;
      },
      wait: async (params: any) => {
        calls.waits.push(params);
      },
      switchModel: async (params: any) => {
        calls.switchModels.push(params);
      },
      switchAgent: async (params: any) => {
        calls.switchAgents.push(params);
      },
      prompt: async (params: any) => {
        calls.prompts.push(params);
        const inboxId = `prompt-${calls.prompts.length}`;
        let inputs = pendingInputTypes.get(params.sessionID);
        if (!inputs) {
          inputs = new Map();
          pendingInputTypes.set(params.sessionID, inputs);
        }
        inputs.set(inboxId, "user");
        return { id: inboxId };
      },
      interrupt: async (params: any) => {
        calls.interrupts.push(params);
      },
      compact: async (_params: any) => {},
      fork: async (params: any) => {
        calls.forks.push(params);
        const id = `mock-forked-456`;
        sessionMessages.set(id, []);
        return { id, time: { created: 1_800_000_000_000 } };
      },
      revert: {
        stage: async (params: any) => {
          calls.reverts.push({ stage: params });
          stagedReverts.set(params.sessionID, params.messageID);
        },
        commit: async (params: any) => {
          calls.reverts.push({ commit: params });
          const boundary = stagedReverts.get(params.sessionID);
          stagedReverts.delete(params.sessionID);
          const rows = sessionMessages.get(params.sessionID);
          if (!rows || boundary === undefined) return;
          const index = rows.findIndex((row) => row.id === boundary);
          if (index === -1) return;
          // Released daemons (v2.0.8) delete the boundary row itself.
          rows.splice(index);
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
  } as unknown as OpenCodeClientFacade;

  const handle: OpenCodeHostHandle = {
    instanceId: ProviderInstanceId.make("opencode-test"),
    client,
    isRemote: false,
    databasePath: null,
    serviceVersion: null,
  };

  return { handle, emit, calls };
}

describe("OpenCodeAdapter", () => {
  it.effect("starts session, sends turn, and streams text deltas and turn completion", () =>
    Effect.gen(function* () {
      const { handle, emit, calls } = createMockHost();
      const adapter = yield* makeOpenCodeAdapter(handle);

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

      const sessionStarted = yield* waitForEvent((e) => e.type === "session.started");
      expect((sessionStarted.payload as any).message).toBe("OpenCode session started");

      const threadStarted = yield* waitForEvent((e) => e.type === "thread.started");
      expect((threadStarted.payload as any).providerThreadId).toBe(sessionId);

      const turnStarted = yield* waitForEvent((e) => e.type === "turn.started");
      expect(turnStarted.turnId).toBeTruthy();

      const runningSessions = yield* adapter.listSessions();
      expect(runningSessions).toHaveLength(1);
      expect(runningSessions[0]?.status).toBe("running");
      expect(runningSessions[0]?.activeTurnId).toBe(turnStarted.turnId);

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
      const adapter = yield* makeOpenCodeAdapter(handle);

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
      const adapter = yield* makeOpenCodeAdapter(handle);

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

      const resolvedPermEvent = yield* waitForEvent(
        (e) => e.type === "request.resolved" && e.requestId === "perm-123",
      );
      expect(resolvedPermEvent).toBeDefined();

      // Duplicate reply to permission should be a no-op (idempotent)
      yield* adapter.respondToRequest(threadId, ApprovalRequestId.make("perm-123"), "accept");
      expect(calls.permissionReplies.filter((r) => r.requestID === "perm-123")).toHaveLength(1);

      // Reply to form
      yield* adapter.respondToUserInput(threadId, ApprovalRequestId.make("form-456"), {
        env: "production",
      });
      expect(calls.formReplies).toContainEqual({
        sessionID: sessionId,
        formID: "form-456",
        answer: { env: "production" },
      });

      const resolvedFormEvent = yield* waitForEvent(
        (e) => e.type === "user-input.resolved" && e.requestId === "form-456",
      );
      expect(resolvedFormEvent).toBeDefined();

      // Duplicate reply to form should also be a no-op
      yield* adapter.respondToUserInput(threadId, ApprovalRequestId.make("form-456"), {
        env: "production",
      });
      expect(calls.formReplies.filter((r) => r.formID === "form-456")).toHaveLength(1);
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("supports durable inbox cancellation, delivery changing, and session forks", () =>
    Effect.gen(function* () {
      const { handle, emit, calls } = createMockHost();
      const adapter = yield* makeOpenCodeAdapter(handle);

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

  it.effect("rolls back a turn by reverting at its input message, not an assistant message", () =>
    Effect.gen(function* () {
      const { handle, emit, calls } = createMockHost();
      const adapter = yield* makeOpenCodeAdapter(handle);

      const threadId = ThreadId.make("thread-rollback");
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

      // Two full turns, each bounded by its delivered user input.
      const firstTurn = yield* adapter.sendTurn({ threadId, input: "first" });
      emit({
        type: "session.inbox.delivered",
        data: { sessionID: sessionId, inboxID: "prompt-1" },
      });
      emit({
        type: "session.text.delta",
        data: { sessionID: sessionId, assistantMessageID: "msg-a", delta: "one" },
      });
      emit({ type: "session.execution.succeeded", data: { sessionID: sessionId } });
      yield* waitForEvent((e) => e.type === "turn.completed");

      const secondTurn = yield* adapter.sendTurn({ threadId, input: "second" });
      emit({
        type: "session.inbox.delivered",
        data: { sessionID: sessionId, inboxID: "prompt-2" },
      });
      emit({
        type: "session.text.delta",
        data: { sessionID: sessionId, assistantMessageID: "msg-b", delta: "two" },
      });
      emit({ type: "session.execution.succeeded", data: { sessionID: sessionId } });
      yield* waitForEvent((e) => e.type === "turn.completed" && e.turnId === secondTurn.turnId);

      yield* adapter.rollbackThread!(threadId, 1);

      // The revert must stage at the turn's user input row. Staging at the
      // assistant message (the old behavior) would keep the turn's own input
      // and silently leave a ghost turn behind.
      const stages = calls.reverts.filter((call) => call.stage);
      expect(stages).toHaveLength(1);
      expect(stages[0].stage).toEqual({
        sessionID: sessionId,
        messageID: "prompt-2",
        files: false,
      });
      expect(calls.reverts.filter((call) => call.commit)).toHaveLength(1);

      // The rolled-back turn's message mapping is gone, so forking at it fails.
      const forkExit = yield* adapter.forkThread!(
        threadId,
        ThreadId.make("thread-rollback-fork"),
        secondTurn.turnId,
      ).pipe(Effect.exit);
      expect(Exit.isFailure(forkExit)).toBe(true);

      // The surviving turn still forks cleanly.
      const forkResult = yield* adapter.forkThread!(
        threadId,
        ThreadId.make("thread-rollback-fork"),
        firstTurn.turnId,
      );
      expect((forkResult.resumeCursor as any).sessionID).toContain("mock-forked");
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("ignores mid-turn steered input when picking a rollback boundary", () =>
    Effect.gen(function* () {
      const { handle, emit, calls } = createMockHost();
      const adapter = yield* makeOpenCodeAdapter(handle);

      const threadId = ThreadId.make("thread-rollback-steer");
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

      yield* adapter.sendTurn({ threadId, input: "first" });
      emit({
        type: "session.inbox.delivered",
        data: { sessionID: sessionId, inboxID: "prompt-1" },
      });
      // Input delivered mid-execution with no matching send merges into the
      // running turn; its message row must not count as a turn boundary.
      emit({
        type: "session.inbox.delivered",
        data: { sessionID: sessionId, inboxID: "ext-1" },
      });
      emit({
        type: "session.text.delta",
        data: { sessionID: sessionId, assistantMessageID: "msg-a", delta: "one" },
      });
      // Wait for the pump to consume the deliveries before rolling back.
      yield* waitForEvent((e) => e.type === "content.delta");

      // The turn is still running: rollback interrupts it first.
      yield* adapter.rollbackThread!(threadId, 1);
      expect(calls.interrupts).toEqual([{ sessionID: sessionId }]);
      expect(calls.waits).toEqual([{ sessionID: sessionId }]);

      const cancelled = yield* waitForEvent(
        (e) => e.type === "turn.completed" && (e.payload as any).state === "cancelled",
      );
      expect(cancelled).toBeDefined();

      // Boundary is the turn's input, not the steered "ext-1" row that the
      // daemon also recorded as a user message.
      const stages = calls.reverts.filter((call) => call.stage);
      expect(stages).toHaveLength(1);
      expect(stages[0].stage.messageID).toBe("prompt-1");
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("recreates the session when the daemon keeps a first-message boundary", () =>
    Effect.gen(function* () {
      const { handle, emit, calls } = createMockHost();
      const adapter = yield* makeOpenCodeAdapter(handle);

      const threadId = ThreadId.make("thread-rollback-recreate");
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

      yield* adapter.sendTurn({ threadId, input: "only" });
      emit({
        type: "session.inbox.delivered",
        data: { sessionID: sessionId, inboxID: "prompt-1" },
      });
      emit({
        type: "session.text.delta",
        data: { sessionID: sessionId, assistantMessageID: "msg-a", delta: "work" },
      });
      emit({ type: "session.execution.succeeded", data: { sessionID: sessionId } });
      yield* waitForEvent((e) => e.type === "turn.completed");

      // Dev-branch daemons keep the boundary row on commit; simulate that.
      (handle.client.session.revert as any).commit = async (params: any) => {
        calls.reverts.push({ commit: params });
      };

      yield* adapter.rollbackThread!(threadId, 1);

      // The boundary row (prompt-1) survived the commit and has no
      // predecessor, so the session is replaced wholesale.
      expect(calls.reverts.filter((call) => call.stage)).toHaveLength(1);
      expect(calls.creates).toHaveLength(2);
      expect(calls.removes).toEqual([{ sessionID: sessionId }]);

      const sessions = yield* adapter.listSessions();
      expect(sessions).toHaveLength(1);
      const newSessionId = (sessions[0]!.resumeCursor as { sessionID: string }).sessionID;
      expect(newSessionId).not.toBe(sessionId);

      // The replacement session accepts new work under the same thread.
      yield* adapter.sendTurn({ threadId, input: "again" });
      expect(calls.prompts.at(-1)?.sessionID).toBe(newSessionId);
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("stages a second revert at the predecessor when the boundary survives", () =>
    Effect.gen(function* () {
      const { handle, emit, calls } = createMockHost();
      const adapter = yield* makeOpenCodeAdapter(handle);

      const threadId = ThreadId.make("thread-rollback-gt");
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

      for (const [turn, assistant] of [
        ["prompt-1", "msg-a"],
        ["prompt-2", "msg-b"],
      ] as const) {
        yield* adapter.sendTurn({ threadId, input: "turn" });
        emit({
          type: "session.inbox.delivered",
          data: { sessionID: sessionId, inboxID: turn },
        });
        emit({
          type: "session.text.delta",
          data: { sessionID: sessionId, assistantMessageID: assistant, delta: "x" },
        });
        emit({ type: "session.execution.succeeded", data: { sessionID: sessionId } });
        yield* waitForEvent((e) => e.type === "turn.completed");
      }

      // Dev-branch daemon: commit deletes rows strictly after the boundary.
      (handle.client.session.revert as any).commit = async (params: any) => {
        calls.reverts.push({ commit: params });
      };

      yield* adapter.rollbackThread!(threadId, 1);

      const stages = calls.reverts.filter((call) => call.stage);
      // First stage at the turn's input; the boundary survives, so a second
      // stage lands on its predecessor (the previous turn's assistant row).
      expect(stages).toHaveLength(2);
      expect(stages[0].stage.messageID).toBe("prompt-2");
      expect(stages[1].stage.messageID).toBe("msg-a");
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("opens a new turn for provider-initiated input delivered while idle", () =>
    Effect.gen(function* () {
      const { handle, emit, calls } = createMockHost();
      const adapter = yield* makeOpenCodeAdapter(handle);

      const threadId = ThreadId.make("thread-bg-completion");
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

      // Turn 1: the agent finishes while a background job still runs.
      const firstTurn = yield* adapter.sendTurn({ threadId, input: "build in background" });
      emit({
        type: "session.inbox.delivered",
        data: { sessionID: sessionId, inboxID: "prompt-1" },
      });
      emit({
        type: "session.text.delta",
        data: { sessionID: sessionId, assistantMessageID: "msg-a", delta: "Started." },
      });
      emit({ type: "session.execution.succeeded", data: { sessionID: sessionId } });
      yield* waitForEvent(
        (e) =>
          e.type === "turn.completed" &&
          e.turnId === firstTurn.turnId &&
          (e.payload as any).state === "completed",
      );

      // The bg job finishes; the daemon enqueues its completion as a
      // synthetic item and delivers it into a fresh busy period.
      emit({
        type: "session.inbox.enqueued",
        data: { sessionID: sessionId, inboxID: "bg-1", item: { type: "synthetic" } },
      });
      emit({
        type: "session.inbox.delivered",
        data: { sessionID: sessionId, inboxID: "bg-1" },
      });
      emit({ type: "session.execution.started", data: { sessionID: sessionId } });
      emit({
        type: "session.text.delta",
        data: { sessionID: sessionId, assistantMessageID: "msg-b", delta: "Build finished." },
      });
      emit({ type: "session.execution.succeeded", data: { sessionID: sessionId } });

      const secondStarted = yield* waitForEvent(
        (e) => e.type === "turn.started" && e.turnId !== firstTurn.turnId,
      );
      const secondTurnId = secondStarted.turnId!;

      const secondDelta = yield* waitForEvent(
        (e) => e.type === "content.delta" && (e.payload as any).delta === "Build finished.",
      );
      expect(secondDelta.turnId).toBe(secondTurnId);

      const secondCompleted = yield* waitForEvent(
        (e) => e.type === "turn.completed" && e.turnId === secondTurnId,
      );
      expect((secondCompleted.payload as any).state).toBe("completed");

      // The minted turn's input row is a real turn boundary for rollback.
      yield* adapter.rollbackThread!(threadId, 1);
      const stages = calls.reverts.filter((call) => call.stage);
      expect(stages).toHaveLength(1);
      expect(stages[0].stage.messageID).toBe("bg-1");
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("does not open a turn for a compaction item delivered while idle", () =>
    Effect.gen(function* () {
      const { handle, emit } = createMockHost();
      const adapter = yield* makeOpenCodeAdapter(handle);

      const threadId = ThreadId.make("thread-compaction-delivery");
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

      // A daemon-internal control item delivered while idle must not open a
      // turn; only its own dedicated events should surface.
      emit({
        type: "session.inbox.enqueued",
        data: { sessionID: sessionId, inboxID: "cmp-1", item: { type: "compaction" } },
      });
      emit({
        type: "session.inbox.delivered",
        data: { sessionID: sessionId, inboxID: "cmp-1" },
      });
      emit({
        type: "session.compaction.ended",
        data: { sessionID: sessionId, reason: "auto", text: "summary" },
      });

      // Pump order is FIFO: once the compaction event lands, both inbox
      // events were already handled.
      while (true) {
        const event = yield* Queue.take(canonicalEvents);
        if (event.type === "turn.started") {
          throw new Error("compaction delivery minted a turn");
        }
        if (event.type === "thread.state.changed") return;
      }
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("switches the session model and agent from the model selection before prompting", () =>
    Effect.gen(function* () {
      const { handle, calls } = createMockHost();
      const adapter = yield* makeOpenCodeAdapter(handle);

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
          instanceId: "opencode",
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
      const adapter = yield* makeOpenCodeAdapter(handle);

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
          instanceId: "opencode",
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
        const adapter = yield* makeOpenCodeAdapter(handle);

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
            instanceId: "opencode",
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
      const adapter = yield* makeOpenCodeAdapter(handle);

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
      const adapter = yield* makeOpenCodeAdapter(handle);

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
      expect((completed.payload as any).data.command).toBe("ls -la");
      expect((completed.payload as any).data.rawOutput?.output).toBe("file-a\nfile-b");
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("attributes late events after a completed turn to that turn, not a synthetic one", () =>
    Effect.gen(function* () {
      const { handle, emit } = createMockHost();
      const adapter = yield* makeOpenCodeAdapter(handle);

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

  it.effect(
    "defers a mid-turn follow-up until its inbox item is delivered and splits the turns",
    () =>
      Effect.gen(function* () {
        const { handle, emit } = createMockHost();
        const adapter = yield* makeOpenCodeAdapter(handle);

        const threadId = ThreadId.make("thread-deferred");
        const session = yield* adapter.startSession({
          threadId,
          cwd: "/tmp/project",
          runtimeMode: "full-access",
        });
        const sessionId = (session.resumeCursor as { sessionID: string }).sessionID;

        const canonicalEvents = yield* Queue.unbounded<ProviderRuntimeEvent>();
        const deltas: Array<{ turnId: string | undefined; delta: string }> = [];
        yield* Effect.forkScoped(
          Stream.runForEach(adapter.streamEvents, (event) =>
            Effect.gen(function* () {
              if (event.type === "content.delta") {
                deltas.push({
                  turnId: event.turnId === undefined ? undefined : String(event.turnId),
                  delta: (event.payload as any).delta,
                });
              }
              yield* Queue.offer(canonicalEvents, event);
            }),
          ),
        );
        const waitForEvent = Effect.fn("waitForEvent")(function* (
          predicate: (event: ProviderRuntimeEvent) => boolean,
        ) {
          while (true) {
            const event = yield* Queue.take(canonicalEvents);
            if (predicate(event)) return event;
          }
        });

        const firstTurn = yield* adapter.sendTurn({ threadId, input: "first" });
        emit({
          type: "session.inbox.delivered",
          data: { sessionID: sessionId, inboxID: "prompt-1" },
        });
        emit({
          type: "session.text.delta",
          data: { sessionID: sessionId, assistantMessageID: "msg-a", delta: "one " },
        });

        // A second send while the turn runs must not open its turn yet.
        const secondTurn = yield* adapter.sendTurn({ threadId, input: "second" });
        expect(secondTurn.turnId).not.toBe(firstTurn.turnId);
        const runningSessions = yield* adapter.listSessions();
        expect(runningSessions[0]?.activeTurnId).toBe(firstTurn.turnId);

        // More text from the first assistant message still belongs to turn 1.
        emit({
          type: "session.text.delta",
          data: { sessionID: sessionId, assistantMessageID: "msg-a", delta: "one more " },
        });

        // The queued item is delivered mid-execution: the first turn ends and
        // the follow-up turn begins there.
        emit({
          type: "session.inbox.delivered",
          data: { sessionID: sessionId, inboxID: "prompt-2" },
        });
        emit({
          type: "session.text.delta",
          data: { sessionID: sessionId, assistantMessageID: "msg-b", delta: "two" },
        });
        emit({ type: "session.execution.succeeded", data: { sessionID: sessionId } });

        const superseded = yield* waitForEvent(
          (e) => e.type === "turn.completed" && e.turnId === firstTurn.turnId,
        );
        expect((superseded.payload as any).state).toBe("cancelled");

        const secondStarted = yield* waitForEvent(
          (e) => e.type === "turn.started" && e.turnId === secondTurn.turnId,
        );
        expect(secondStarted).toBeDefined();

        // The execution terminal closes only the follow-up turn — the
        // superseded turn already emitted its own turn.completed.
        const finalCompleted = yield* waitForEvent(
          (e) =>
            e.type === "turn.completed" &&
            e.turnId === secondTurn.turnId &&
            (e.payload as any).state === "completed",
        );
        expect(finalCompleted).toBeDefined();
        expect(deltas).toEqual([
          { turnId: firstTurn.turnId, delta: "one " },
          { turnId: firstTurn.turnId, delta: "one more " },
          { turnId: secondTurn.turnId, delta: "two" },
        ]);
      }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("reports native compaction completion without an active turn", () =>
    Effect.gen(function* () {
      const { handle, emit } = createMockHost();
      const adapter = yield* makeOpenCodeAdapter(handle);
      const threadId = ThreadId.make("thread-compaction");
      const session = yield* adapter.startSession({
        threadId,
        cwd: "/tmp/project",
        runtimeMode: "full-access",
      });
      const sessionId = (session.resumeCursor as { sessionID: string }).sessionID;
      const events = yield* Queue.unbounded<ProviderRuntimeEvent>();
      yield* Effect.forkScoped(
        Stream.runForEach(adapter.streamEvents, (event) => Queue.offer(events, event)),
      );

      emit({
        type: "session.compaction.ended",
        data: { sessionID: sessionId, reason: "manual", text: "summary", recent: "msg-1" },
      });

      while (true) {
        const event = yield* Queue.take(events);
        if (event.type !== "thread.state.changed") continue;
        expect((event.payload as any).state).toBe("compacted");
        expect(event.turnId).toBeUndefined();
        return;
      }
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );

  it.effect("rejects attachments when connected to a remote OpenCode host", () =>
    Effect.gen(function* () {
      const { handle } = createMockHost();
      (handle as any).isRemote = true;
      const adapter = yield* makeOpenCodeAdapter(handle);

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

  describe("openCodeClientErrorMessage", () => {
    it("unpacks nested cause chains", () => {
      const err = new Error("Transport", {
        cause: new TypeError("fetch failed", {
          cause: new Error("connect ECONNREFUSED 127.0.0.1:49374"),
        }),
      });
      expect(openCodeClientErrorMessage(err)).toBe(
        "Transport: fetch failed: connect ECONNREFUSED 127.0.0.1:49374",
      );
    });

    it("deduplicates identical messages in the cause chain", () => {
      const err = new Error("Connection failed", {
        cause: new Error("Connection failed", {
          cause: new Error("Server offline"),
        }),
      });
      expect(openCodeClientErrorMessage(err)).toBe("Connection failed: Server offline");
    });

    it("extracts detail and reason properties from structured errors", () => {
      expect(openCodeClientErrorMessage({ detail: "Token expired" })).toBe("Token expired");
      expect(openCodeClientErrorMessage({ reason: "Rate limited" })).toBe("Rate limited");
    });

    it("returns Unknown error for null or undefined", () => {
      expect(openCodeClientErrorMessage(null)).toBe("Unknown error");
      expect(openCodeClientErrorMessage(undefined)).toBe("Unknown error");
    });
  });

  it.effect("surfaces nested transport errors cleanly when startSession fails", () =>
    Effect.gen(function* () {
      const { handle } = createMockHost();
      (handle.client.session as any).create = async () => {
        throw new Error("Transport", {
          cause: new TypeError("fetch failed", {
            cause: new Error("connect ECONNREFUSED 127.0.0.1:49374"),
          }),
        });
      };
      const adapter = yield* makeOpenCodeAdapter(handle);

      const threadId = ThreadId.make("thread-fail-create");
      const exit = yield* adapter
        .startSession({
          threadId,
          cwd: "/tmp/project",
          runtimeMode: "full-access",
        })
        .pipe(Effect.exit);

      expect(Exit.isFailure(exit)).toBe(true);
      if (Exit.isFailure(exit)) {
        const failureStr = Cause.pretty(exit.cause);
        expect(failureStr).toContain(
          "Failed to create OpenCode session: Transport: fetch failed: connect ECONNREFUSED 127.0.0.1:49374",
        );
      }
    }).pipe(Effect.scoped, Effect.provide(testLayer)),
  );
});
