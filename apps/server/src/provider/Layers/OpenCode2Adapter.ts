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
  EventId,
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
  RuntimeItemId,
  RuntimeRequestId,
  RuntimeTaskId,
  ThreadId,
  TurnId,
} from "@t3tools/contracts";
import { getModelSelectionStringOptionValue } from "@t3tools/shared/model";
import * as Clock from "effect/Clock";
import * as Crypto from "effect/Crypto";
import * as DateTime from "effect/DateTime";
import * as Deferred from "effect/Deferred";
import * as Effect from "effect/Effect";
import * as Fiber from "effect/Fiber";
import * as Queue from "effect/Queue";
import * as Stream from "effect/Stream";

import { resolveAttachmentPath } from "../../attachmentStore.ts";
import { ServerConfig } from "../../config.ts";

import type { ProviderAdapterError } from "../Errors.ts";
import {
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
  readonly createdAt: string;
  activeTurnId: TurnId | null;
  activeTurnStatus: "idle" | "running" | "interrupted" | "failed";
  hasSubagents: boolean;
  subagents: Map<string, SubagentState>;
  pendingInboxItems: Set<string>;
  turnToMessageId: Map<TurnId, string>;
  messageIds: Array<string>;
}

export interface OpenCode2AdapterLiveOptions {
  readonly instanceId?: ProviderInstanceId;
}

export function makeOpenCode2Adapter(
  hostHandle: OpenCode2HostHandle,
  options?: OpenCode2AdapterLiveOptions,
): Effect.Effect<ProviderAdapterShape<ProviderAdapterError>, never, Crypto.Crypto | ServerConfig> {
  return Effect.gen(function* () {
    const crypto = yield* Crypto.Crypto;
    const serverConfig = yield* ServerConfig;
    const boundInstanceId = options?.instanceId ?? ProviderInstanceId.make("opencode2");
    const runtimeEventQueue = yield* Queue.unbounded<ProviderRuntimeEvent>();
    const sessionsByThreadId = new Map<ThreadId, OpenCode2SessionContext>();
    const threadIdBySessionId = new Map<string, ThreadId>();

    const randomUUIDv4 = crypto.randomUUIDv4.pipe(
      Effect.mapError(
        (cause) =>
          new ProviderAdapterRequestError({
            provider: PROVIDER,
            method: "crypto/randomUUIDv4",
            detail: "Failed to generate runtime identifier.",
            cause,
          }),
      ),
    );

    const buildEventBase = (input: {
      readonly threadId: ThreadId;
      readonly turnId?: TurnId | undefined;
      readonly itemId?: string | undefined;
      readonly requestId?: string | undefined;
      readonly raw?: unknown;
    }) =>
      Effect.all({
        eventId: randomUUIDv4.pipe(Effect.map(EventId.make)),
        createdAt: DateTime.now.pipe(Effect.map(DateTime.formatIso)),
      }).pipe(
        Effect.map(({ eventId, createdAt }) => ({
          eventId,
          provider: PROVIDER,
          providerInstanceId: boundInstanceId,
          threadId: input.threadId,
          createdAt,
          ...(input.turnId ? { turnId: input.turnId } : {}),
          ...(input.itemId ? { itemId: RuntimeItemId.make(input.itemId) } : {}),
          ...(input.requestId ? { requestId: RuntimeRequestId.make(input.requestId) } : {}),
          ...(input.raw !== undefined
            ? {
                raw: {
                  source: "opencode2.sdk.event" as const,
                  payload: input.raw,
                },
              }
            : {}),
        })),
      );

    const emit = (event: ProviderRuntimeEvent) =>
      Queue.offer(runtimeEventQueue, event).pipe(Effect.asVoid);

    // Start background event pump from hostHandle.client.event.subscribe()
    const pumpFiber = yield* Effect.gen(function* () {
      const asyncIterable = yield* Effect.try({
        try: () => hostHandle.client.event.subscribe(),
        catch: (cause) =>
          new ProviderAdapterRequestError({
            provider: PROVIDER,
            method: "event.subscribe",
            detail: `Failed to subscribe: ${String(cause)}`,
            cause,
          }),
      }).pipe(Effect.orElseSucceed(() => null));

      if (!asyncIterable) return;

      yield* Stream.fromAsyncIterable(
        asyncIterable as AsyncIterable<{ type: string; data?: any }>,
        (cause) =>
          new ProviderAdapterRequestError({
            provider: PROVIDER,
            method: "event.stream",
            detail: String(cause),
            cause,
          }),
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

            const observedMessageId = data.assistantMessageID ?? data.messageID;
            if (typeof observedMessageId === "string" && observedMessageId.length > 0) {
              parentContext.turnToMessageId.set(turnId, observedMessageId);
              if (!parentContext.messageIds.includes(observedMessageId)) {
                parentContext.messageIds.push(observedMessageId);
              }
            }

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

                yield* emit({
                  ...(yield* buildEventBase({ threadId, turnId, raw: rawEvent })),
                  type: "task.started",
                  payload: {
                    taskId,
                    description: `Subagent: ${subagent.name}`,
                    taskType: "subagent",
                    agentKind: "agent",
                    agentId: sessionId,
                  },
                });
              }

              if (subagent) {
                if (eventType === "session.tool.called") {
                  subagent.lastToolName = data.tool;
                  yield* emit({
                    ...(yield* buildEventBase({ threadId, turnId, raw: rawEvent })),
                    type: "task.progress",
                    payload: {
                      taskId: subagent.taskId,
                      description: `Subagent ${subagent.name} running`,
                      lastToolName: data.tool,
                      status: "running",
                      taskType: "subagent",
                      agentKind: "agent",
                      agentId: sessionId,
                    },
                  });
                } else if (eventType === "session.step.ended") {
                  const tokens = data.tokens ?? {};
                  subagent.cumulativeInputTokens += tokens.input ?? 0;
                  subagent.cumulativeOutputTokens += tokens.output ?? 0;
                  subagent.cumulativeReasoningTokens += tokens.reasoning ?? 0;

                  yield* emit({
                    ...(yield* buildEventBase({ threadId, turnId, raw: rawEvent })),
                    type: "task.progress",
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
                  });
                } else if (
                  eventType === "session.execution.succeeded" ||
                  eventType === "session.execution.failed"
                ) {
                  const succeeded = eventType === "session.execution.succeeded";
                  yield* emit({
                    ...(yield* buildEventBase({ threadId, turnId, raw: rawEvent })),
                    type: "task.completed",
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
                  });
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
                  yield* emit({
                    ...(yield* buildEventBase({ threadId, turnId, raw: rawEvent })),
                    type: "content.delta",
                    payload: {
                      streamKind: "assistant_text",
                      delta: data.delta,
                    },
                  });
                }
                break;
              }

              case "session.reasoning.delta": {
                if (typeof data.delta === "string" && data.delta.length > 0) {
                  yield* emit({
                    ...(yield* buildEventBase({ threadId, turnId, raw: rawEvent })),
                    type: "content.delta",
                    payload: {
                      streamKind: "reasoning_text",
                      delta: data.delta,
                    },
                  });
                }
                break;
              }

              case "session.tool.called": {
                const callId = data.callID ?? data.id ?? `call-${yield* Clock.currentTimeMillis}`;
                yield* emit({
                  ...(yield* buildEventBase({ threadId, turnId, itemId: callId, raw: rawEvent })),
                  type: "item.started",
                  payload: {
                    itemType: "command_execution",
                    title: data.tool ?? "tool",
                    ...(typeof data.input === "string" ? { detail: data.input } : {}),
                  },
                });
                break;
              }

              case "session.tool.success": {
                const callId = data.callID ?? data.id ?? `call-${yield* Clock.currentTimeMillis}`;
                yield* emit({
                  ...(yield* buildEventBase({ threadId, turnId, itemId: callId, raw: rawEvent })),
                  type: "item.completed",
                  payload: {
                    itemType: "command_execution",
                    status: "completed",
                    title: data.tool ?? "tool",
                    ...(typeof data.output === "string" ? { detail: data.output } : {}),
                  },
                });
                break;
              }

              case "session.tool.failed": {
                const callId = data.callID ?? data.id ?? `call-${yield* Clock.currentTimeMillis}`;
                yield* emit({
                  ...(yield* buildEventBase({ threadId, turnId, itemId: callId, raw: rawEvent })),
                  type: "item.completed",
                  payload: {
                    itemType: "command_execution",
                    status: "failed",
                    title: data.tool ?? "tool",
                    detail: data.error?.message ?? "Tool failed",
                  },
                });
                break;
              }

              case "permission.asked": {
                const requestId =
                  data.permissionID ?? data.id ?? `perm-${yield* Clock.currentTimeMillis}`;
                yield* emit({
                  ...(yield* buildEventBase({ threadId, turnId, requestId, raw: rawEvent })),
                  type: "request.opened",
                  payload: {
                    requestType: "command_execution_approval",
                    options: [
                      { decision: "accept", label: "Allow once" },
                      { decision: "acceptAlways", label: "Allow always" },
                      { decision: "decline", label: "Deny" },
                    ],
                    detail: data.description ?? "Permission requested",
                  },
                });
                break;
              }

              case "form.created": {
                const requestId =
                  data.formID ?? data.id ?? `form-${yield* Clock.currentTimeMillis}`;
                yield* emit({
                  ...(yield* buildEventBase({ threadId, turnId, requestId, raw: rawEvent })),
                  type: "user-input.requested",
                  payload: {
                    questions: (data.fields ?? []).map((f: any) => ({
                      id: f.name,
                      prompt: f.title ?? f.name,
                      allowCustomAnswer: true,
                      options: [],
                    })),
                  },
                });
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
                yield* emit({
                  ...(yield* buildEventBase({ threadId, turnId, raw: rawEvent })),
                  type: "turn.completed",
                  payload: {
                    state: "completed",
                  },
                });
                parentContext.activeTurnId = null;
                break;
              }

              case "session.execution.failed": {
                parentContext.activeTurnStatus = "failed";
                yield* emit({
                  ...(yield* buildEventBase({ threadId, turnId, raw: rawEvent })),
                  type: "turn.completed",
                  payload: {
                    state: "failed",
                    errorMessage: data.error?.message ?? "Turn execution failed",
                  },
                });
                parentContext.activeTurnId = null;
                break;
              }

              case "session.execution.interrupted": {
                parentContext.activeTurnStatus = "interrupted";
                yield* emit({
                  ...(yield* buildEventBase({ threadId, turnId, raw: rawEvent })),
                  type: "turn.completed",
                  payload: {
                    state: "cancelled",
                  },
                });
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
        const now = yield* DateTime.now.pipe(Effect.map(DateTime.formatIso));
        const cwd = input.cwd ?? process.cwd();
        const existing = sessionsByThreadId.get(input.threadId);
        if (existing) {
          return {
            provider: PROVIDER,
            providerInstanceId: boundInstanceId,
            status: "ready" as const,
            runtimeMode: input.runtimeMode ?? "full-access",
            threadId: input.threadId,
            cwd,
            resumeCursor: { sessionID: existing.sessionId, durableSeq: 0 },
            createdAt: existing.createdAt,
            updatedAt: now,
          };
        }

        const resumeSessionId = (input.resumeCursor as { sessionID?: string })?.sessionID;
        let sessionId = resumeSessionId;
        let createdAt = now;

        if (!sessionId) {
          const session = yield* Effect.tryPromise({
            try: () =>
              hostHandle.client.session.create({
                location: { directory: cwd },
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
          if ((session as any).time?.created) {
            createdAt = DateTime.makeUnsafe((session as any).time.created).pipe(DateTime.formatIso);
          }
        }

        const context: OpenCode2SessionContext = {
          threadId: input.threadId,
          sessionId,
          directory: cwd,
          createdAt,
          activeTurnId: null,
          activeTurnStatus: "idle",
          hasSubagents: false,
          subagents: new Map(),
          pendingInboxItems: new Set(),
          turnToMessageId: new Map(),
          messageIds: [],
        };

        sessionsByThreadId.set(input.threadId, context);
        threadIdBySessionId.set(sessionId, input.threadId);

        return {
          provider: PROVIDER,
          providerInstanceId: boundInstanceId,
          status: "ready" as const,
          runtimeMode: input.runtimeMode ?? "full-access",
          threadId: input.threadId,
          cwd,
          resumeCursor: { sessionID: sessionId, durableSeq: 0 },
          createdAt,
          updatedAt: now,
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

        const nowMillis = yield* Clock.currentTimeMillis;
        const turnId = TurnId.make(`turn-${nowMillis}`);
        context.activeTurnId = turnId;
        context.activeTurnStatus = "running";

        const selectedVariant = input.modelSelection
          ? getModelSelectionStringOptionValue(input.modelSelection, "variant")
          : undefined;
        const selectedAgent = input.modelSelection
          ? getModelSelectionStringOptionValue(input.modelSelection, "agent")
          : undefined;

        const delivery = input.delivery ?? "steer";

        if (hostHandle.isRemote && input.attachments && input.attachments.length > 0) {
          return yield* new ProviderAdapterRequestError({
            provider: PROVIDER,
            method: "session.prompt",
            detail: "Attachments are not supported when connecting to a remote OpenCode server.",
          });
        }

        const files = input.attachments
          ? input.attachments
              .filter((a) => a.type === "file" || a.type === "image")
              .map((attachment) => {
                const filePath = resolveAttachmentPath({
                  attachmentsDir: serverConfig.attachmentsDir,
                  attachment,
                });
                return {
                  uri: `file://${filePath}`,
                  name: attachment.name,
                };
              })
          : undefined;

        yield* Effect.tryPromise({
          try: async () => {
            await hostHandle.client.session.prompt({
              sessionID: context.sessionId,
              text: input.input ?? "",
              delivery,
              ...(selectedVariant ? { variant: selectedVariant } : {}),
              ...(selectedAgent ? { agent: selectedAgent } : {}),
              ...(files && files.length > 0 ? { files } : {}),
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
      boundaryTurnId: TurnId,
    ): Effect.Effect<{ resumeCursor?: unknown }, ProviderAdapterError> =>
      Effect.gen(function* () {
        const sourceContext = sessionsByThreadId.get(sourceThreadId);
        if (!sourceContext) {
          return yield* new ProviderAdapterSessionNotFoundError({
            provider: PROVIDER,
            threadId: sourceThreadId,
          });
        }

        const boundaryMessageId = boundaryTurnId
          ? sourceContext.turnToMessageId.get(boundaryTurnId)
          : undefined;

        if (boundaryTurnId && !boundaryMessageId) {
          return yield* new ProviderAdapterRequestError({
            provider: PROVIDER,
            method: "session.fork",
            detail: `Cannot fork at turn '${boundaryTurnId}': message mapping is not available for this turn.`,
          });
        }

        const boundary = boundaryMessageId
          ? { type: "through" as const, messageID: boundaryMessageId }
          : { type: "before" as const, messageID: "latest" };

        const forked = yield* Effect.tryPromise({
          try: () =>
            hostHandle.client.session.fork({
              sessionID: sourceContext.sessionId,
              boundary,
            }),
          catch: (cause) =>
            new ProviderAdapterRequestError({
              provider: PROVIDER,
              method: "session.fork",
              detail: `Failed to fork session: ${String(cause)}`,
              cause,
            }),
        });

        const now = yield* DateTime.now.pipe(Effect.map(DateTime.formatIso));
        let forkedCreatedAt = now;
        if ((forked as any).time?.created) {
          forkedCreatedAt = DateTime.makeUnsafe((forked as any).time.created).pipe(
            DateTime.formatIso,
          );
        }

        const forkedTurnToMessageId = new Map<TurnId, string>();
        const forkedMessageIds: Array<string> = [];
        for (const msgId of sourceContext.messageIds) {
          forkedMessageIds.push(msgId);
          if (boundaryMessageId && msgId === boundaryMessageId) {
            break;
          }
        }
        for (const [tId, mId] of sourceContext.turnToMessageId.entries()) {
          if (forkedMessageIds.includes(mId)) {
            forkedTurnToMessageId.set(tId, mId);
          }
        }

        const targetContext: OpenCode2SessionContext = {
          threadId: targetThreadId,
          sessionId: forked.id,
          directory: sourceContext.directory,
          createdAt: forkedCreatedAt,
          activeTurnId: null,
          activeTurnStatus: "idle",
          hasSubagents: false,
          subagents: new Map(),
          pendingInboxItems: new Set(),
          turnToMessageId: forkedTurnToMessageId,
          messageIds: forkedMessageIds,
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
      numTurns: number,
    ): Effect.Effect<ProviderThreadSnapshot, ProviderAdapterError> =>
      Effect.gen(function* () {
        const context = sessionsByThreadId.get(threadId);
        if (!context) {
          return yield* new ProviderAdapterSessionNotFoundError({
            provider: PROVIDER,
            threadId,
          });
        }

        if (numTurns > context.messageIds.length) {
          return yield* new ProviderAdapterRequestError({
            provider: PROVIDER,
            method: "session.revert",
            detail: `Cannot rollback ${numTurns} turn(s): only ${context.messageIds.length} message(s) are recorded.`,
          });
        }

        const targetMessageId =
          context.messageIds.length >= numTurns
            ? context.messageIds[context.messageIds.length - numTurns]
            : "latest";

        yield* Effect.tryPromise({
          try: async () => {
            await hostHandle.client.session.revert.stage({
              sessionID: context.sessionId,
              messageID: targetMessageId ?? "latest",
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
              requestID: requestId,
              reply: response,
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
              answer: answers as Record<string, string | number | boolean | ReadonlyArray<string>>,
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

        if (context.activeTurnStatus === "running") {
          yield* interruptTurn(threadId).pipe(Effect.ignore);
        }
        sessionsByThreadId.delete(threadId);
        threadIdBySessionId.delete(context.sessionId);
      });

    const listSessions = (): Effect.Effect<ReadonlyArray<ProviderSession>> =>
      Effect.gen(function* () {
        const now = yield* DateTime.now.pipe(Effect.map(DateTime.formatIso));
        return Array.from(sessionsByThreadId.values()).map((ctx) => ({
          provider: PROVIDER,
          providerInstanceId: boundInstanceId,
          status: ctx.activeTurnStatus === "running" ? ("running" as const) : ("ready" as const),
          runtimeMode: "full-access" as const,
          threadId: ctx.threadId,
          cwd: ctx.directory,
          resumeCursor: { sessionID: ctx.sessionId, durableSeq: 0 },
          createdAt: ctx.createdAt,
          updatedAt: now,
        }));
      });

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
        for (const [threadId, ctx] of sessionsByThreadId.entries()) {
          if (ctx.activeTurnStatus === "running") {
            yield* interruptTurn(threadId).pipe(Effect.ignore);
          }
        }
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
