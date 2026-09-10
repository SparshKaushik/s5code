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
  /** Most recent turn seen on this session; late events after a turn closes
   * still belong to it and must not spawn a phantom turn. */
  lastTurnId: TurnId | null;
  activeTurnStatus: "idle" | "running" | "interrupted" | "failed";
  hasSubagents: boolean;
  subagents: Map<string, SubagentState>;
  pendingInboxItems: Set<string>;
  /** Permission/form requests remember the session that owns them on the
   * OpenCode side: subagents ask through their own child session, and the
   * reply endpoint rejects a session mismatch with "Permission request not
   * found". */
  requestSessionIdByRequestId: Map<string, string>;
  /** v2 tool events carry no tool name; `session.tool.input.started` does. */
  toolCallByCallId: Map<string, { name: string; input?: unknown }>;
  turnToMessageId: Map<TurnId, string>;
  messageIds: Array<string>;
}

export interface OpenCode2AdapterLiveOptions {
  readonly instanceId?: ProviderInstanceId;
}

const eventCallId = (data: Record<string, unknown>): string | undefined =>
  typeof data.callID === "string" ? data.callID : typeof data.id === "string" ? data.id : undefined;

function toToolLifecycleItemType(
  toolName: string,
): "command_execution" | "file_change" | "web_search" | "mcp_tool_call" | "dynamic_tool_call" {
  const normalized = toolName.toLowerCase();
  if (normalized === "todowrite" || normalized === "todoread") {
    return "dynamic_tool_call";
  }
  if (
    normalized.includes("bash") ||
    normalized.includes("shell") ||
    normalized.includes("command")
  ) {
    return "command_execution";
  }
  if (normalized.includes("edit") || normalized.includes("write") || normalized.includes("patch")) {
    return "file_change";
  }
  if (normalized.includes("web")) {
    return "web_search";
  }
  if (normalized.includes("mcp")) {
    return "mcp_tool_call";
  }
  return "dynamic_tool_call";
}

/** Human summary for a tool call title, from the tool's input object. */
function summarizeToolCallInput(toolName: string, input: unknown): string | undefined {
  const record =
    input !== null && typeof input === "object" && !Array.isArray(input)
      ? (input as Record<string, unknown>)
      : {};
  const firstString = (...keys: string[]): string | undefined => {
    for (const key of keys) {
      const value = record[key];
      if (typeof value === "string" && value.trim().length > 0) return value.trim();
    }
    return undefined;
  };
  const normalized = toolName.toLowerCase();
  if (normalized.includes("bash") || normalized.includes("shell")) {
    return firstString("command", "script", "cmd");
  }
  if (normalized.includes("edit") || normalized.includes("write") || normalized.includes("patch")) {
    return firstString("filePath", "file_path", "path", "file");
  }
  if (normalized.includes("read")) {
    return firstString("filePath", "file_path", "path", "file");
  }
  if (normalized.includes("grep") || normalized.includes("glob")) {
    return firstString("pattern", "query");
  }
  if (normalized.includes("webfetch")) {
    return firstString("url");
  }
  if (normalized.includes("task") || normalized.includes("agent")) {
    return firstString("description", "prompt");
  }
  return (
    firstString(
      "description",
      "command",
      "url",
      "filePath",
      "file_path",
      "path",
      "pattern",
      "query",
    ) ?? undefined
  );
}

/** Flattens a v2 tool success `content` array into its text output. */
function toolContentText(content: unknown): string | undefined {
  if (!Array.isArray(content)) return undefined;
  const text = content
    .map((entry) =>
      entry &&
      typeof entry === "object" &&
      (entry as any).type === "text" &&
      typeof (entry as any).text === "string"
        ? ((entry as any).text as string)
        : null,
    )
    .filter((entry): entry is string => entry !== null)
    .join("\n")
    .trim();
  return text.length > 0 ? text : undefined;
}

function structuredErrorMessage(error: unknown): string | undefined {
  if (error && typeof error === "object") {
    const message = (error as any).message;
    if (typeof message === "string" && message.trim().length > 0) return message.trim();
  }
  if (typeof error === "string" && error.trim().length > 0) return error.trim();
  return undefined;
}

/** OpenCode client errors often stringify as "[object Object]"; dig for the message. */
function openCodeClientErrorMessage(cause: unknown): string {
  return (
    structuredErrorMessage(cause) ?? structuredErrorMessage((cause as any)?.cause) ?? String(cause)
  );
}

function parseModelSlug(slug: string): { providerID: string; modelID: string } | null {
  const slash = slug.indexOf("/");
  if (slash <= 0 || slash === slug.length - 1) {
    return null;
  }
  return {
    providerID: slug.slice(0, slash),
    modelID: slug.slice(slash + 1),
  };
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
    /** Child (subagent) sessions; their events fold into the parent thread. */
    const childSessionIds = new Set<string>();
    const supportedVariantsByModelKey = new Map<string, Set<string>>();

    const getSupportedVariantsForModel = (
      directory: string,
      providerID: string,
      modelID: string,
    ): Effect.Effect<Set<string>> =>
      Effect.gen(function* () {
        if (typeof hostHandle.client.model?.list !== "function") {
          return new Set<string>();
        }

        const key = `${providerID}/${modelID}`.toLowerCase();
        const cached = supportedVariantsByModelKey.get(key);
        if (cached !== undefined) return cached;

        const modelsRes = yield* Effect.tryPromise({
          try: () => hostHandle.client.model.list({ location: { directory } }),
          catch: () => ({ data: [] as unknown[] }),
        }).pipe(Effect.orElseSucceed(() => ({ data: [] as unknown[] })));

        for (const item of (modelsRes as { data?: any[] }).data ?? []) {
          if (item?.id) {
            const pId = String(
              item.providerID || item.id.split("/")[0] || "opencode",
            ).toLowerCase();
            const mId = String(item.modelID || item.id.split("/").pop() || item.id).toLowerCase();
            const fullSlug = `${pId}/${mId}`;
            const rawId = String(item.id).toLowerCase();
            const variants = new Set<string>();
            if (Array.isArray(item.variants)) {
              for (const v of item.variants) {
                const vid = typeof v === "string" ? v : v?.id;
                if (typeof vid === "string" && vid.trim().length > 0) {
                  variants.add(vid.trim());
                }
              }
            }
            supportedVariantsByModelKey.set(fullSlug, variants);
            supportedVariantsByModelKey.set(mId, variants);
            supportedVariantsByModelKey.set(rawId, variants);
          }
        }

        return (
          supportedVariantsByModelKey.get(key) ??
          supportedVariantsByModelKey.get(modelID.toLowerCase()) ??
          new Set<string>()
        );
      });

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
      readonly turnId?: TurnId | null | undefined;
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

    const buildPermissionRequestEvent = Effect.fn("buildPermissionRequestEvent")(function* (
      context: OpenCode2SessionContext,
      data: Record<string, any>,
      rawEvent: unknown,
      threadId: ThreadId,
      turnId: TurnId | null,
    ) {
      const requestId =
        (typeof data.id === "string" && data.id) ||
        (typeof data.permissionID === "string" && data.permissionID) ||
        `perm-${yield* Clock.currentTimeMillis}`;
      // The owning session replies, not necessarily the root one: subagents
      // ask through their own child session and OpenCode rejects a reply
      // whose session does not own the request.
      if (typeof data.sessionID === "string" && data.sessionID.length > 0) {
        context.requestSessionIdByRequestId.set(requestId, data.sessionID);
      }
      const action = typeof data.action === "string" ? data.action : "action";
      const resources = Array.isArray(data.resources)
        ? data.resources.filter((r: unknown): r is string => typeof r === "string")
        : [];
      return {
        ...(yield* buildEventBase({ threadId, turnId, requestId, raw: rawEvent })),
        type: "request.opened" as const,
        payload: {
          requestType: "command_execution_approval" as const,
          options: [
            { decision: "accept" as const, label: "Allow once" },
            { decision: "acceptAlways" as const, label: "Allow always" },
            { decision: "decline" as const, label: "Deny" },
          ],
          detail:
            (typeof data.message === "string" && data.message) || [action, ...resources].join(" "),
        },
      } satisfies ProviderRuntimeEvent;
    });

    const buildFormRequestEvent = Effect.fn("buildFormRequestEvent")(function* (
      context: OpenCode2SessionContext,
      data: Record<string, any>,
      rawEvent: unknown,
      threadId: ThreadId,
      turnId: TurnId | null,
    ) {
      const form = (data.form ?? {}) as Record<string, any>;
      const requestId =
        (typeof form.id === "string" && form.id) ||
        (typeof data.formID === "string" && data.formID) ||
        `form-${yield* Clock.currentTimeMillis}`;
      if (typeof form.sessionID === "string" && form.sessionID.length > 0) {
        context.requestSessionIdByRequestId.set(requestId, form.sessionID);
      }
      const fields = Array.isArray(form.fields) ? form.fields : [];
      return {
        ...(yield* buildEventBase({ threadId, turnId, requestId, raw: rawEvent })),
        type: "user-input.requested" as const,
        payload: {
          questions: fields.map((f: any) => ({
            id: String(f.key ?? f.name ?? "field"),
            header: String(f.title ?? f.key ?? f.name ?? "Input"),
            question: String(f.description ?? f.title ?? f.key ?? f.name ?? "Please answer"),
            allowCustomAnswer: f.custom !== false,
            options: (Array.isArray(f.options) ? f.options : []).map((o: any) => ({
              label: String(o.label ?? o.value ?? ""),
              description: String(o.description ?? ""),
              value: String(o.value ?? o.label ?? ""),
            })),
          })),
        },
      } satisfies ProviderRuntimeEvent;
    });

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
            // `form.created` nests everything under `data.form`; permission
            // and session events carry a top-level sessionID.
            const sessionId: string | undefined =
              data.sessionID ?? data.form?.sessionID ?? data.info?.id;

            if (!sessionId) return;

            // Check if this event belongs to a root session or a child session
            let threadId = threadIdBySessionId.get(sessionId);
            let isChildSession = threadId !== undefined && childSessionIds.has(sessionId);
            let parentContext: OpenCode2SessionContext | undefined;

            if (!threadId) {
              // Check if parentID matches any active session
              const parentSessionId = data.parentID;
              if (parentSessionId && threadIdBySessionId.has(parentSessionId)) {
                threadId = threadIdBySessionId.get(parentSessionId)!;
                parentContext = sessionsByThreadId.get(threadId);
                isChildSession = true;
                // Route the child session directly from now on: later events
                // it owns (e.g. permission.asked) carry no parentID.
                childSessionIds.add(sessionId);
                threadIdBySessionId.set(sessionId, threadId);
              }
            } else {
              parentContext = sessionsByThreadId.get(threadId);
            }

            if (!threadId || !parentContext) return;

            const turnId =
              parentContext.activeTurnId ??
              // Events that trail a closed turn (out-of-order tool results,
              // steers) belong to that turn. Attributing them to a synthetic
              // turn makes the UI fold them into a bogus second response.
              parentContext.lastTurnId ??
              null;

            // v2 tool events carry no tool name; input.started does.
            if (eventType === "session.tool.input.started") {
              const callId = eventCallId(data);
              if (callId && typeof data.name === "string") {
                parentContext.toolCallByCallId.set(callId, { name: data.name });
              }
            }

            // Approval requests must surface even when no turn is associated
            // (e.g. replying to a pending request recovered after a server
            // restart), so they skip the turn guard below.
            if (eventType === "permission.asked" || eventType === "form.created") {
              const requested =
                eventType === "permission.asked"
                  ? yield* buildPermissionRequestEvent(
                      parentContext,
                      data,
                      rawEvent,
                      threadId,
                      turnId,
                    )
                  : yield* buildFormRequestEvent(parentContext, data, rawEvent, threadId, turnId);
              yield* emit(requested);
              return;
            }

            if (!turnId) return;

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
                  name: data.agent ?? data.info?.agent ?? "assistant",
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
                  const callId = eventCallId(data);
                  const tracked = callId ? parentContext.toolCallByCallId.get(callId) : undefined;
                  subagent.lastToolName =
                    (typeof data.tool === "string" ? data.tool : undefined) ??
                    tracked?.name ??
                    subagent.lastToolName;
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
                const callId = eventCallId(data) ?? `call-${yield* Clock.currentTimeMillis}`;
                const tracked = parentContext.toolCallByCallId.get(callId);
                const toolName =
                  (typeof data.tool === "string" && data.tool) || tracked?.name || "tool";
                const input = data.input ?? tracked?.input;
                if (tracked && data.input !== undefined) {
                  tracked.input = data.input;
                }
                const inputSummary = summarizeToolCallInput(toolName, input);
                const itemType = toToolLifecycleItemType(toolName);
                yield* emit({
                  ...(yield* buildEventBase({ threadId, turnId, itemId: callId, raw: rawEvent })),
                  type: "item.started",
                  payload: {
                    itemType,
                    title: inputSummary ?? toolName,
                    data: {
                      tool: toolName,
                      toolName,
                      ...(itemType === "command_execution" && inputSummary !== undefined
                        ? { command: inputSummary }
                        : {}),
                    },
                  },
                });
                break;
              }

              case "session.tool.success": {
                const callId = eventCallId(data) ?? `call-${yield* Clock.currentTimeMillis}`;
                const tracked = parentContext.toolCallByCallId.get(callId);
                const toolName =
                  (typeof data.tool === "string" && data.tool) || tracked?.name || "tool";
                const inputSummary = summarizeToolCallInput(toolName, tracked?.input);
                parentContext.toolCallByCallId.delete(callId);
                const output = toolContentText(data.content);
                yield* emit({
                  ...(yield* buildEventBase({ threadId, turnId, itemId: callId, raw: rawEvent })),
                  type: "item.completed",
                  payload: {
                    itemType: toToolLifecycleItemType(toolName),
                    status: "completed",
                    title: inputSummary ?? toolName,
                    ...(output ? { detail: output } : {}),
                    data: { tool: toolName, toolName },
                  },
                });
                break;
              }

              case "session.tool.failed": {
                const callId = eventCallId(data) ?? `call-${yield* Clock.currentTimeMillis}`;
                const tracked = parentContext.toolCallByCallId.get(callId);
                const toolName =
                  (typeof data.tool === "string" && data.tool) || tracked?.name || "tool";
                parentContext.toolCallByCallId.delete(callId);
                yield* emit({
                  ...(yield* buildEventBase({ threadId, turnId, itemId: callId, raw: rawEvent })),
                  type: "item.completed",
                  payload: {
                    itemType: toToolLifecycleItemType(toolName),
                    status: "failed",
                    title: summarizeToolCallInput(toolName, tracked?.input) ?? toolName,
                    detail: structuredErrorMessage(data.error) ?? "Tool failed",
                    data: { tool: toolName, toolName },
                  },
                });
                break;
              }

              case "permission.asked":
              case "form.created":
                // Handled above: approval requests skip the turn guard.
                break;

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
                parentContext.lastTurnId = turnId;
                parentContext.toolCallByCallId.clear();
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
                parentContext.lastTurnId = turnId;
                parentContext.toolCallByCallId.clear();
                yield* emit({
                  ...(yield* buildEventBase({ threadId, turnId, raw: rawEvent })),
                  type: "turn.completed",
                  payload: {
                    state: "failed",
                    errorMessage: structuredErrorMessage(data.error) ?? "Turn execution failed",
                  },
                });
                parentContext.activeTurnId = null;
                break;
              }

              case "session.execution.interrupted": {
                parentContext.activeTurnStatus = "interrupted";
                parentContext.lastTurnId = turnId;
                parentContext.toolCallByCallId.clear();
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
          lastTurnId: null,
          activeTurnStatus: "idle",
          hasSubagents: false,
          subagents: new Map(),
          pendingInboxItems: new Set(),
          requestSessionIdByRequestId: new Map(),
          toolCallByCallId: new Map(),
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
        context.lastTurnId = turnId;
        context.activeTurnStatus = "running";

        const selectedVariant = input.modelSelection
          ? getModelSelectionStringOptionValue(input.modelSelection, "variant")
          : undefined;
        const selectedAgent = input.modelSelection
          ? getModelSelectionStringOptionValue(input.modelSelection, "agent")
          : undefined;

        // `session.prompt` carries no model field; the session's model (and
        // agent) must be switched explicitly before the turn is admitted.
        const modelSlug = input.modelSelection?.model;
        const parsedModel = modelSlug ? parseModelSlug(modelSlug) : null;
        if (parsedModel) {
          const supportedVariants = yield* getSupportedVariantsForModel(
            context.directory,
            parsedModel.providerID,
            parsedModel.modelID,
          );
          // Only pass a variant if the target model actually supports it in its catalog.
          // Passing an unsupported variant (e.g. "high" on Qwen3.8-27B or models without
          // variants) makes OpenCode 2 fail with 'Variant unavailable'.
          const validVariant =
            selectedVariant && supportedVariants.has(selectedVariant) ? selectedVariant : undefined;

          const switchModelWith = (variant?: string) =>
            Effect.tryPromise({
              try: () =>
                hostHandle.client.session.switchModel({
                  sessionID: context.sessionId,
                  model: {
                    id: parsedModel.modelID,
                    providerID: parsedModel.providerID,
                    ...(variant ? { variant } : {}),
                  },
                }),
              catch: (cause) =>
                new ProviderAdapterRequestError({
                  provider: PROVIDER,
                  method: "session.switchModel",
                  detail: `Failed to switch OpenCode 2 model to '${modelSlug}': ${openCodeClientErrorMessage(cause)}`,
                  cause,
                }),
            });

          if (validVariant) {
            yield* switchModelWith(validVariant).pipe(
              Effect.catchIf(
                (err) => err.detail.toLowerCase().includes("variant unavailable"),
                () => switchModelWith(undefined),
              ),
            );
          } else {
            yield* switchModelWith(undefined);
          }
        }
        if (selectedAgent) {
          // OpenCode 2 agent IDs are lowercase (e.g. "build", "plan"). If a UI selection
          // or persisted model option passes a title-cased name like "Build", normalize it.
          const agentId = selectedAgent.toLowerCase();
          yield* Effect.tryPromise({
            try: () =>
              hostHandle.client.session.switchAgent({
                sessionID: context.sessionId,
                agent: agentId,
              }),
            catch: (cause) =>
              new ProviderAdapterRequestError({
                provider: PROVIDER,
                method: "session.switchAgent",
                detail: `Failed to switch OpenCode 2 agent to '${agentId}': ${openCodeClientErrorMessage(cause)}`,
                cause,
              }),
          });
        }

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
          lastTurnId: null,
          activeTurnStatus: "idle",
          hasSubagents: false,
          subagents: new Map(),
          pendingInboxItems: new Set(),
          requestSessionIdByRequestId: new Map(),
          toolCallByCallId: new Map(),
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

        // Subagents ask through their own child session; replying with the
        // root session's id makes OpenCode reject with "Permission request
        // not found".
        const askingSessionId =
          context.requestSessionIdByRequestId.get(requestId) ?? context.sessionId;
        context.requestSessionIdByRequestId.delete(requestId);

        yield* Effect.tryPromise({
          try: () =>
            hostHandle.client.permission.reply({
              sessionID: askingSessionId,
              requestID: requestId,
              reply: response,
            }),
          catch: (cause) =>
            new ProviderAdapterRequestError({
              provider: PROVIDER,
              method: "permission.reply",
              detail: `Failed to reply to permission: ${openCodeClientErrorMessage(cause)}`,
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

        const askingSessionId =
          context.requestSessionIdByRequestId.get(requestId) ?? context.sessionId;
        context.requestSessionIdByRequestId.delete(requestId);

        yield* Effect.tryPromise({
          try: () =>
            hostHandle.client.form.reply({
              sessionID: askingSessionId,
              formID: requestId,
              answer: answers as Record<string, string | number | boolean | ReadonlyArray<string>>,
            }),
          catch: (cause) =>
            new ProviderAdapterRequestError({
              provider: PROVIDER,
              method: "form.reply",
              detail: `Failed to reply to form: ${openCodeClientErrorMessage(cause)}`,
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
