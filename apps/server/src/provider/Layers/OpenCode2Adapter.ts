/**
 * OpenCode2Adapter — provider adapter for OpenCode 2.
 *
 * Implements the ProviderAdapterShape SPI using OpenCode 2's event stream,
 * durable inbox, child session subagents, and session forks.
 *
 * @module provider/Layers/OpenCode2Adapter
 */
import {
  ApprovalRequestId,
  ModelSelection,
  ProviderApprovalDecision,
  ProviderDriverKind,
  ProviderInstanceId,
  ProviderInteractionMode,
  ProviderRuntimeEvent,
  ProviderSendTurnInput,
  ProviderSession,
  ProviderSessionStartInput,
  ProviderTurnStartResult,
  ProviderUserInputAnswers,
  RuntimeTaskId,
  ThreadId,
  TurnId,
} from "@t3tools/contracts";
import { getModelSelectionStringOptionValue } from "@t3tools/shared/model";
import * as Deferred from "effect/Deferred";
import * as Effect from "effect/Effect";
import * as Fiber from "effect/Fiber";
import * as Queue from "effect/Queue";
import * as Stream from "effect/Stream";

import {
  ProviderAdapterError,
  ProviderAdapterRequestError,
  ProviderAdapterSessionClosedError,
  ProviderAdapterSessionNotFoundError,
} from "../Errors.ts";
import { type OpenCode2HostHandle } from "../OpenCode2Host.ts";
import {
  type ProviderAdapterCapabilities,
  type ProviderAdapterShape,
  type ProviderThreadSnapshot,
} from "../Services/ProviderAdapter.ts";

const PROVIDER = ProviderDriverKind.make("opencode2");

interface SubagentState {
  readonly childSessionId: string;
  readonly taskId: RuntimeTaskId;
  readonly name: string;
  cumulativeInputTokens: number;
  cumulativeOutputTokens: number;
  cumulativeReasoningTokens: number;
  lastToolName?: string;
}

interface OpenCode2SessionContext {
  readonly threadId: ThreadId;
  readonly sessionId: string;
  readonly directory: string;
  activeTurnId: TurnId | null;
  activeTurnStatus: "idle" | "running" | "interrupted" | "failed";
  hasSubagents: boolean;
  subagents: Map<string, SubagentState>;
  pendingInboxItems: Set<string>;
}

export interface OpenCode2AdapterLiveOptions {
  readonly instanceId?: ProviderInstanceId;
}

export function makeOpenCode2Adapter(
  hostHandle: OpenCode2HostHandle,
  options?: OpenCode2AdapterLiveOptions,
): Effect.Effect<ProviderAdapterShape<ProviderAdapterError>, never, never> {
  return Effect.gen(function* () {
    const boundInstanceId = options?.instanceId ?? ProviderInstanceId.make("opencode2");
    const runtimeEventQueue = yield* Queue.unbounded<ProviderRuntimeEvent>();
    const sessionsByThreadId = new Map<ThreadId, OpenCode2SessionContext>();
    const threadIdBySessionId = new Map<string, ThreadId>();

    // Start background event pump from hostHandle.client.event.subscribe()
    const pumpFiber = yield* Effect.gen(function* () {
      const asyncIterable = yield* Effect.tryPromise({
        try: () => hostHandle.client.event.subscribe(),
        catch: () => null,
      }).pipe(Effect.orElseSucceed(() => null));

      if (!asyncIterable) return;

      yield* Stream.fromAsyncIterable(
        asyncIterable as AsyncIterable<{ type: string; data?: any }>,
        (err) => err,
      ).pipe(
        Stream.runForEach((rawEvent) =>
          Effect.gen(function* () {
            const eventType = rawEvent.type;
            const data = rawEvent.data ?? {};
            const sessionId: string | undefined = data.sessionID;

            if (!sessionId) return;

            // Check if this event belongs to a root session or a child session
            let threadId = threadIdBySessionId.get(sessionId);
            let isChildSession = false;
            let parentContext: OpenCode2SessionContext | undefined;

            if (!threadId) {
              // Check if parentID matches any active session
              const parentSessionId = data.parentID;
              if (parentSessionId && threadIdBySessionId.has(parentSessionId)) {
                threadId = threadIdBySessionId.get(parentSessionId)!;
                parentContext = sessionsByThreadId.get(threadId);
                isChildSession = true;
              }
            } else {
              parentContext = sessionsByThreadId.get(threadId);
            }

            if (!threadId || !parentContext) return;

            const turnId = parentContext.activeTurnId ?? TurnId.make("turn-initial");

            // Handle Subagent Lifecycle (Capability 2)
            if (isChildSession) {
              let subagent = parentContext.subagents.get(sessionId);
              if (!subagent && eventType === "session.created") {
                const taskId = RuntimeTaskId.make(`task-subagent-${sessionId}`);
                subagent = {
                  childSessionId: sessionId,
                  taskId,
                  name: data.agent ?? "assistant",
                  cumulativeInputTokens: 0,
                  cumulativeOutputTokens: 0,
                  cumulativeReasoningTokens: 0,
                };
                parentContext.subagents.set(sessionId, subagent);
                parentContext.hasSubagents = true;

                yield* Queue.offer(runtimeEventQueue, {
                  type: "task.started",
                  threadId,
                  turnId,
                  payload: {
                    taskId,
                    description: `Subagent: ${subagent.name}`,
                    taskType: "subagent",
                    agentKind: "agent",
                    agentId: sessionId,
                  },
                } as ProviderRuntimeEvent);
              }

              if (subagent) {
                if (eventType === "session.tool.called") {
                  subagent.lastToolName = data.tool;
                  yield* Queue.offer(runtimeEventQueue, {
                    type: "task.progress",
                    threadId,
                    turnId,
                    payload: {
                      taskId: subagent.taskId,
                      description: `Subagent ${subagent.name} running`,
                      lastToolName: data.tool,
                      status: "running",
                      taskType: "subagent",
                      agentKind: "agent",
                      agentId: sessionId,
                    },
                  } as ProviderRuntimeEvent);
                } else if (eventType === "session.step.ended") {
                  const tokens = data.tokens ?? {};
                  subagent.cumulativeInputTokens += tokens.input ?? 0;
                  subagent.cumulativeOutputTokens += tokens.output ?? 0;
                  subagent.cumulativeReasoningTokens += tokens.reasoning ?? 0;

                  yield* Queue.offer(runtimeEventQueue, {
                    type: "task.progress",
                    threadId,
                    turnId,
                    payload: {
                      taskId: subagent.taskId,
                      description: `Subagent ${subagent.name} step ended`,
                      lastToolName: subagent.lastToolName,
                      status: "running",
                      typedUsage: {
                        totalTokens:
                          subagent.cumulativeInputTokens + subagent.cumulativeOutputTokens,
                        inputTokens: subagent.cumulativeInputTokens,
                        outputTokens: subagent.cumulativeOutputTokens,
                        reasoningOutputTokens: subagent.cumulativeReasoningTokens,
                      },
                      taskType: "subagent",
                      agentKind: "agent",
                      agentId: sessionId,
                    },
                  } as ProviderRuntimeEvent);
                } else if (
                  eventType === "session.execution.succeeded" ||
                  eventType === "session.execution.failed"
                ) {
                  const succeeded = eventType === "session.execution.succeeded";
                  yield* Queue.offer(runtimeEventQueue, {
                    type: "task.completed",
                    threadId,
                    turnId,
                    payload: {
                      taskId: subagent.taskId,
                      status: succeeded ? "completed" : "failed",
                      typedUsage: {
                        totalTokens:
                          subagent.cumulativeInputTokens + subagent.cumulativeOutputTokens,
                        inputTokens: subagent.cumulativeInputTokens,
                        outputTokens: subagent.cumulativeOutputTokens,
                        reasoningOutputTokens: subagent.cumulativeReasoningTokens,
                      },
                      taskType: "subagent",
                      agentKind: "agent",
                      agentId: sessionId,
                    },
                  } as ProviderRuntimeEvent);
                  parentContext.subagents.delete(sessionId);
                }
              }
              return;
            }

            // Root Session Event Handlers
            switch (eventType) {
              case "session.execution.started": {
                parentContext.activeTurnStatus = "running";
                break;
              }

              case "session.text.delta": {
                if (typeof data.delta === "string" && data.delta.length > 0) {
                  yield* Queue.offer(runtimeEventQueue, {
                    type: "thread.token.delta",
                    threadId,
                    turnId,
                    payload: {
                      delta: data.delta,
                    },
                  } as ProviderRuntimeEvent);
                }
                break;
              }

              case "session.reasoning.delta": {
                if (typeof data.delta === "string" && data.delta.length > 0) {
                  yield* Queue.offer(runtimeEventQueue, {
                    type: "thread.reasoning.delta",
                    threadId,
                    turnId,
                    payload: {
                      delta: data.delta,
                    },
                  } as ProviderRuntimeEvent);
                }
                break;
              }

              case "session.tool.called": {
                yield* Queue.offer(runtimeEventQueue, {
                  type: "tool.called",
                  threadId,
                  turnId,
                  payload: {
                    callId: data.callID ?? data.id ?? `call-${Date.now()}`,
                    toolName: data.tool ?? "unknown_tool",
                    input: data.input ?? {},
                  },
                } as ProviderRuntimeEvent);
                break;
              }

              case "session.tool.success": {
                yield* Queue.offer(runtimeEventQueue, {
                  type: "tool.completed",
                  threadId,
                  turnId,
                  payload: {
                    callId: data.callID ?? data.id ?? `call-${Date.now()}`,
                    toolName: data.tool ?? "unknown_tool",
                    output: data.output ?? {},
                    status: "success",
                  },
                } as ProviderRuntimeEvent);
                break;
              }

              case "session.tool.failed": {
                yield* Queue.offer(runtimeEventQueue, {
                  type: "tool.completed",
                  threadId,
                  turnId,
                  payload: {
                    callId: data.callID ?? data.id ?? `call-${Date.now()}`,
                    toolName: data.tool ?? "unknown_tool",
                    error: data.error?.message ?? "Tool execution failed",
                    status: "failed",
                  },
                } as ProviderRuntimeEvent);
                break;
              }

              case "permission.asked": {
                yield* Queue.offer(runtimeEventQueue, {
                  type: "request.opened",
                  threadId,
                  turnId,
                  payload: {
                    requestId: ApprovalRequestId.make(data.permissionID ?? data.id),
                    action: "permission",
                    description: data.description ?? `Permission requested: ${data.pattern ?? "*"}`,
                  },
                } as ProviderRuntimeEvent);
                break;
              }

              case "form.created": {
                yield* Queue.offer(runtimeEventQueue, {
                  type: "request.opened",
                  threadId,
                  turnId,
                  payload: {
                    requestId: ApprovalRequestId.make(data.formID ?? data.id),
                    action: "user-input",
                    description: data.title ?? "Input requested",
                    fields: data.fields ?? [],
                  },
                } as ProviderRuntimeEvent);
                break;
              }

              case "session.inbox.delivered": {
                if (data.inboxID) {
                  parentContext.pendingInboxItems.delete(data.inboxID);
                }
                break;
              }

              case "session.inbox.cancelled": {
                if (data.inboxID) {
                  parentContext.pendingInboxItems.delete(data.inboxID);
                }
                break;
              }

              case "session.execution.succeeded": {
                parentContext.activeTurnStatus = "idle";
                yield* Queue.offer(runtimeEventQueue, {
                  type: "turn.completed",
                  threadId,
                  turnId,
                  payload: {
                    status: "completed",
                    hasSubagents: parentContext.hasSubagents,
                  },
                } as ProviderRuntimeEvent);
                parentContext.activeTurnId = null;
                break;
              }

              case "session.execution.failed": {
                parentContext.activeTurnStatus = "failed";
                yield* Queue.offer(runtimeEventQueue, {
                  type: "turn.failed",
                  threadId,
                  turnId,
                  payload: {
                    error: data.error?.message ?? "Turn execution failed",
                  },
                } as ProviderRuntimeEvent);
                parentContext.activeTurnId = null;
                break;
              }

              case "session.execution.interrupted": {
                parentContext.activeTurnStatus = "interrupted";
                yield* Queue.offer(runtimeEventQueue, {
                  type: "turn.interrupted",
                  threadId,
                  turnId,
                  payload: {},
                } as ProviderRuntimeEvent);
                parentContext.activeTurnId = null;
                break;
              }
            }
          }),
        ),
      );
    }).pipe(
      Effect.orElseSucceed(() => undefined),
      Effect.forkDetach,
    );

    const capabilities: ProviderAdapterCapabilities = {
      sessionModelSwitch: "in-session",
      promptlessTurnContinuation: false,
      supportsConversationRollback: true,
      supportsConversationFork: true,
      supportsInboxSteering: true,
      supportsInboxQueueing: true,
    };

    const startSession = (
      input: ProviderSessionStartInput,
    ): Effect.Effect<ProviderSession, ProviderAdapterError> =>
      Effect.gen(function* () {
        const existing = sessionsByThreadId.get(input.threadId);
        if (existing) {
          return {
            threadId: input.threadId,
            sessionId: existing.sessionId,
            resumeCursor: { sessionID: existing.sessionId, durableSeq: 0 },
          };
        }

        const resumeSessionId = (input.resumeCursor as { sessionID?: string })?.sessionID;
        let sessionId = resumeSessionId;

        if (!sessionId) {
          const session = yield* Effect.tryPromise({
            try: () =>
              hostHandle.client.session.create({
                directory: input.directory,
                title: `T3 Session: ${input.threadId}`,
              }),
            catch: (cause) =>
              new ProviderAdapterRequestError({
                provider: PROVIDER,
                method: "session.create",
                detail: `Failed to create OpenCode 2 session: ${String(cause)}`,
                cause,
              }),
          });
          sessionId = session.id;
        }

        const context: OpenCode2SessionContext = {
          threadId: input.threadId,
          sessionId,
          directory: input.directory,
          activeTurnId: null,
          activeTurnStatus: "idle",
          hasSubagents: false,
          subagents: new Map(),
          pendingInboxItems: new Set(),
        };

        sessionsByThreadId.set(input.threadId, context);
        threadIdBySessionId.set(sessionId, input.threadId);

        return {
          threadId: input.threadId,
          sessionId,
          resumeCursor: { sessionID: sessionId, durableSeq: 0 },
        };
      });

    const sendTurn = (
      input: ProviderSendTurnInput,
    ): Effect.Effect<ProviderTurnStartResult, ProviderAdapterError> =>
      Effect.gen(function* () {
        const context = sessionsByThreadId.get(input.threadId);
        if (!context) {
          return yield* new ProviderAdapterSessionNotFoundError({
            provider: PROVIDER,
            threadId: input.threadId,
          });
        }

        const turnId = TurnId.make(`turn-${Date.now()}`);
        context.activeTurnId = turnId;
        context.activeTurnStatus = "running";

        const selectedVariant = input.modelSelection
          ? getModelSelectionStringOptionValue(input.modelSelection, "variant")
          : undefined;
        const selectedAgent = input.modelSelection
          ? getModelSelectionStringOptionValue(input.modelSelection, "agent")
          : undefined;

        const delivery = input.delivery ?? "steer";

        yield* Effect.tryPromise({
          try: async () => {
            await hostHandle.client.session.prompt({
              sessionID: context.sessionId,
              text: input.input ?? "",
              delivery,
              ...(selectedVariant ? { variant: selectedVariant } : {}),
              ...(selectedAgent ? { agent: selectedAgent } : {}),
            });
          },
          catch: (cause) =>
            new ProviderAdapterRequestError({
              provider: PROVIDER,
              method: "session.prompt",
              detail: `Failed to send turn to OpenCode 2: ${String(cause)}`,
              cause,
            }),
        });

        return {
          threadId: input.threadId,
          turnId,
          resumeCursor: { sessionID: context.sessionId, durableSeq: 0 },
        };
      });

    const interruptTurn = (
      threadId: ThreadId,
      _turnId?: TurnId,
    ): Effect.Effect<void, ProviderAdapterError> =>
      Effect.gen(function* () {
        const context = sessionsByThreadId.get(threadId);
        if (!context) return;

        yield* Effect.tryPromise({
          try: () => hostHandle.client.session.interrupt({ sessionID: context.sessionId }),
          catch: (cause) =>
            new ProviderAdapterRequestError({
              provider: PROVIDER,
              method: "session.interrupt",
              detail: `Failed to interrupt turn: ${String(cause)}`,
              cause,
            }),
        });
      });

    const compactThread = (threadId: ThreadId): Effect.Effect<void, ProviderAdapterError> =>
      Effect.gen(function* () {
        const context = sessionsByThreadId.get(threadId);
        if (!context) return;

        yield* Effect.tryPromise({
          try: () =>
            hostHandle.client.session.compact({
              sessionID: context.sessionId,
              delivery: "steer",
            }),
          catch: (cause) =>
            new ProviderAdapterRequestError({
              provider: PROVIDER,
              method: "session.compact",
              detail: `Failed to compact thread: ${String(cause)}`,
              cause,
            }),
        });
      });

    const forkThread = (
      sourceThreadId: ThreadId,
      targetThreadId: ThreadId,
      _boundaryTurnId: TurnId,
    ): Effect.Effect<{ resumeCursor?: unknown }, ProviderAdapterError> =>
      Effect.gen(function* () {
        const sourceContext = sessionsByThreadId.get(sourceThreadId);
        if (!sourceContext) {
          return yield* new ProviderAdapterSessionNotFoundError({
            provider: PROVIDER,
            threadId: sourceThreadId,
          });
        }

        const forked = yield* Effect.tryPromise({
          try: () =>
            hostHandle.client.session.fork({
              sessionID: sourceContext.sessionId,
              boundary: { type: "before", messageID: "latest" },
            }),
          catch: (cause) =>
            new ProviderAdapterRequestError({
              provider: PROVIDER,
              method: "session.fork",
              detail: `Failed to fork session: ${String(cause)}`,
              cause,
            }),
        });

        const targetContext: OpenCode2SessionContext = {
          threadId: targetThreadId,
          sessionId: forked.id,
          directory: sourceContext.directory,
          activeTurnId: null,
          activeTurnStatus: "idle",
          hasSubagents: false,
          subagents: new Map(),
          pendingInboxItems: new Set(),
        };

        sessionsByThreadId.set(targetThreadId, targetContext);
        threadIdBySessionId.set(forked.id, targetThreadId);

        return {
          resumeCursor: { sessionID: forked.id, durableSeq: 0 },
        };
      });

    const cancelInboxItem = (
      threadId: ThreadId,
      inboxId: string,
    ): Effect.Effect<void, ProviderAdapterError> =>
      Effect.gen(function* () {
        const context = sessionsByThreadId.get(threadId);
        if (!context) return;

        yield* Effect.tryPromise({
          try: () =>
            hostHandle.client.session.inbox.cancel({
              sessionID: context.sessionId,
              inboxID: inboxId,
            }),
          catch: (cause) =>
            new ProviderAdapterRequestError({
              provider: PROVIDER,
              method: "session.inbox.cancel",
              detail: `Failed to cancel inbox item: ${String(cause)}`,
              cause,
            }),
        });
      });

    const changeInboxDelivery = (
      threadId: ThreadId,
      inboxId: string,
      delivery: "steer" | "queue",
    ): Effect.Effect<void, ProviderAdapterError> =>
      Effect.gen(function* () {
        const context = sessionsByThreadId.get(threadId);
        if (!context) return;

        yield* Effect.tryPromise({
          try: () =>
            delivery === "steer"
              ? hostHandle.client.session.inbox.steer({
                  sessionID: context.sessionId,
                  inboxID: inboxId,
                })
              : hostHandle.client.session.inbox.queue({
                  sessionID: context.sessionId,
                  inboxID: inboxId,
                }),
          catch: (cause) =>
            new ProviderAdapterRequestError({
              provider: PROVIDER,
              method: `session.inbox.${delivery}`,
              detail: `Failed to change inbox delivery: ${String(cause)}`,
              cause,
            }),
        });
      });

    const rollbackThread = (
      threadId: ThreadId,
      _numTurns: number,
    ): Effect.Effect<ProviderThreadSnapshot, ProviderAdapterError> =>
      Effect.gen(function* () {
        const context = sessionsByThreadId.get(threadId);
        if (!context) {
          return yield* new ProviderAdapterSessionNotFoundError({
            provider: PROVIDER,
            threadId,
          });
        }

        yield* Effect.tryPromise({
          try: async () => {
            await hostHandle.client.session.revert.stage({
              sessionID: context.sessionId,
              messageID: "latest",
              files: false,
            });
            await hostHandle.client.session.revert.commit({
              sessionID: context.sessionId,
            });
          },
          catch: (cause) =>
            new ProviderAdapterRequestError({
              provider: PROVIDER,
              method: "session.revert",
              detail: `Failed to rollback session: ${String(cause)}`,
              cause,
            }),
        });

        return {
          threadId,
          turns: [],
        };
      });

    const respondToRequest = (
      threadId: ThreadId,
      requestId: ApprovalRequestId,
      decision: ProviderApprovalDecision,
    ): Effect.Effect<void, ProviderAdapterError> =>
      Effect.gen(function* () {
        const context = sessionsByThreadId.get(threadId);
        if (!context) return;

        const response =
          decision === "accept" ? "once" : decision === "acceptAlways" ? "always" : "reject";

        yield* Effect.tryPromise({
          try: () =>
            hostHandle.client.permission.reply({
              sessionID: context.sessionId,
              permissionID: requestId,
              response,
            }),
          catch: (cause) =>
            new ProviderAdapterRequestError({
              provider: PROVIDER,
              method: "permission.reply",
              detail: `Failed to reply to permission: ${String(cause)}`,
              cause,
            }),
        });
      });

    const respondToUserInput = (
      threadId: ThreadId,
      requestId: ApprovalRequestId,
      answers: ProviderUserInputAnswers,
    ): Effect.Effect<void, ProviderAdapterError> =>
      Effect.gen(function* () {
        const context = sessionsByThreadId.get(threadId);
        if (!context) return;

        yield* Effect.tryPromise({
          try: () =>
            hostHandle.client.form.reply({
              sessionID: context.sessionId,
              formID: requestId,
              answers: answers as Record<string, any>,
            }),
          catch: (cause) =>
            new ProviderAdapterRequestError({
              provider: PROVIDER,
              method: "form.reply",
              detail: `Failed to reply to form: ${String(cause)}`,
              cause,
            }),
        });
      });

    const stopSession = (threadId: ThreadId): Effect.Effect<void, ProviderAdapterError> =>
      Effect.gen(function* () {
        const context = sessionsByThreadId.get(threadId);
        if (!context) return;

        sessionsByThreadId.delete(threadId);
        threadIdBySessionId.delete(context.sessionId);
      });

    const listSessions = (): Effect.Effect<ReadonlyArray<ProviderSession>> =>
      Effect.succeed(
        Array.from(sessionsByThreadId.values()).map((ctx) => ({
          threadId: ctx.threadId,
          sessionId: ctx.sessionId,
          resumeCursor: { sessionID: ctx.sessionId, durableSeq: 0 },
        })),
      );

    const hasSession = (threadId: ThreadId): Effect.Effect<boolean> =>
      Effect.succeed(sessionsByThreadId.has(threadId));

    const readThread = (
      threadId: ThreadId,
    ): Effect.Effect<ProviderThreadSnapshot, ProviderAdapterError> =>
      Effect.gen(function* () {
        const context = sessionsByThreadId.get(threadId);
        if (!context) {
          return yield* new ProviderAdapterSessionNotFoundError({
            provider: PROVIDER,
            threadId,
          });
        }
        return {
          threadId,
          turns: [],
        };
      });

    const stopAll = (): Effect.Effect<void, ProviderAdapterError> =>
      Effect.gen(function* () {
        sessionsByThreadId.clear();
        threadIdBySessionId.clear();
        yield* Fiber.interrupt(pumpFiber);
      });

    return {
      provider: PROVIDER,
      capabilities,
      startSession,
      sendTurn,
      interruptTurn,
      compactThread,
      forkThread,
      cancelInboxItem,
      changeInboxDelivery,
      rollbackThread,
      respondToRequest,
      respondToUserInput,
      stopSession,
      listSessions,
      hasSession,
      readThread,
      stopAll,
      streamEvents: Stream.fromQueue(runtimeEventQueue),
    } satisfies ProviderAdapterShape<ProviderAdapterError>;
  });
}
