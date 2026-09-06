/**
 * PiAdapter — provider adapter for the pi coding agent (`pi --mode rpc`).
 *
 * One long-lived pi process per thread. pi's RPC mode is a persistent session:
 * `prompt` starts work and returns immediately on *acceptance*, while the real
 * progress arrives as agent events. So `sendTurn` opens the turn, fires
 * `prompt`, and lets the event pump close the turn on `agent_settled` — pi's
 * session-level boundary after retries, compaction, and queued continuations.
 * Treating either the `prompt` response or low-level `agent_end` as completion
 * would settle a turn while pi can still be running.
 *
 * Notable pi-specific behavior this adapter has to absorb:
 *
 *  - **Steering.** A `prompt` while streaming is a steer, not a new turn. pi
 *    exposes that as `streamingBehavior`, so a `sendTurn` during an active turn
 *    reuses the turn id and sends `streamingBehavior: "steer"`.
 *  - **Resume.** pi persists sessions to disk and addresses them by file path.
 *    Our resume cursor stores that path, and start replays it with
 *    `--session <path>` so a thread keeps its history across server restarts.
 *  - **Approvals.** pi has no built-in permission protocol. Extensions raise
 *    `extension_ui_request` (`confirm`/`select`/`input`), which is the *only*
 *    interactive channel, and it blocks the extension until answered. We map
 *    `confirm` onto our approval flow and `select`/`input` onto structured
 *    user input, then reply with `extension_ui_response`. Runtime modes below
 *    `full-access` are enforced by the bundled `t3-runtime-mode` extension,
 *    which raises exactly those confirms from inside pi's `tool_call` hook.
 *  - **Plans.** pi has no plan channel; task lists come from todo tool results
 *    (see `PiPlan`).
 *
 * @module provider/Layers/PiAdapter
 */
import {
  ApprovalRequestId,
  EventId,
  type PiSettings,
  ProviderDriverKind,
  ProviderInstanceId,
  type ProviderRuntimeEvent,
  type ProviderSendTurnInput,
  type ProviderSession,
  type ProviderUserInputAnswers,
  RuntimeItemId,
  RuntimeRequestId,
  type ThreadId,
  TurnId,
  type UserInputQuestion,
} from "@t3tools/contracts";
import * as Crypto from "effect/Crypto";
import * as DateTime from "effect/DateTime";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import * as FileSystem from "effect/FileSystem";
import * as Option from "effect/Option";
import * as Queue from "effect/Queue";
import * as Scope from "effect/Scope";
import * as Stream from "effect/Stream";
import { ChildProcessSpawner } from "effect/unstable/process";

import { resolveAttachmentPath } from "../../attachmentStore.ts";
import { ServerConfig } from "../../config.ts";
import {
  ProviderAdapterProcessError,
  ProviderAdapterRequestError,
  ProviderAdapterSessionNotFoundError,
  ProviderAdapterValidationError,
  type ProviderAdapterError,
} from "../Errors.ts";
import type { ProviderAdapterShape, ProviderThreadSnapshot } from "../Services/ProviderAdapter.ts";
import { type EventNdjsonLogger, makeEventNdjsonLogger } from "./EventNdjsonLogger.ts";
import { parsePiModelSlug, piThinkingLevelFromSelection } from "../pi/PiModelSupport.ts";
import { resolvePiLaunch } from "../pi/PiLaunch.ts";
import { resolvePiRuntimeModeExtensionPath } from "../pi/PiExtensionAssets.ts";
import { isPiPlanToolName, piPlanStepsFromToolResult } from "../pi/PiPlan.ts";
import { makePiRpcConnection, type PiRpcConnection } from "../pi/PiRpcConnection.ts";
import { piRpcErrorDetail, type PiRpcError } from "../pi/PiRpcErrors.ts";
import {
  isPiExtensionUiDialogMethod,
  PiEntries,
  PiSessionState,
  type PiAgentEvent,
  type PiExtensionUiRequest,
} from "../pi/PiRpcSchemas.ts";
import {
  piAppendAssistantDelta,
  piAssistantSegmentItemId,
  piAssistantText,
  piCloseAssistantSegment,
  piContentBlocks,
  piContentStreamKind,
  piToolItemDetail,
  piToolItemType,
  piToolOutputDelta,
  piToolOutputText,
  piTurnHasAssistantText,
  piTurnStateFromStopReason,
  piUsageSnapshot,
  type PiTurnAssistantSegments,
} from "../pi/PiTurnState.ts";

const PROVIDER = ProviderDriverKind.make("pi");

/**
 * Version tag for the resume cursor. Bump when the shape changes so cursors
 * written by an older build are ignored rather than misread.
 */
const PI_RESUME_VERSION = 1 as const;

export interface PiAdapterLiveOptions {
  readonly environment?: NodeJS.ProcessEnv | undefined;
  readonly nativeEventLogPath?: string | undefined;
  readonly nativeEventLogger?: EventNdjsonLogger | undefined;
  readonly instanceId?: ProviderInstanceId | undefined;
}

/** Pending `extension_ui_request` awaiting a user decision. */
interface PendingExtensionUi {
  readonly requestId: ApprovalRequestId;
  readonly request: PiExtensionUiRequest;
  /** Which of our two interactive surfaces owns this request. */
  readonly surface: "approval" | "user-input";
}

interface PiSessionContext {
  readonly threadId: ThreadId;
  session: ProviderSession;
  readonly scope: Scope.Closeable;
  readonly connection: PiRpcConnection;
  /** pi session file, present once known. The resume cursor's payload. */
  sessionFile: string | undefined;
  activeTurnId: TurnId | undefined;
  /**
   * Assistant messages for the active turn. pi emits a separate message per
   * text block between tool calls; each closes at its `message_end` and lands
   * on its own `assistant_message` item, so a settled turn folds down to the
   * last message instead of one bubble carrying every message's text.
   */
  assistantSegmentsByTurn: Map<string, PiTurnAssistantSegments>;
  readonly pendingExtensionUi: Map<ApprovalRequestId, PendingExtensionUi>;
  /**
   * Tools the user approved for the rest of the session. pi's `ctx.ui.confirm`
   * is boolean-only, so "accept for session" cannot be expressed on the wire;
   * we remember it here and auto-confirm later requests for the same tool.
   */
  readonly sessionApprovedTools: Set<string>;
  /**
   * Tool inputs by pi tool-call id. pi's `tool_execution_end` carries only the
   * result, so the command/path the work log shows has to be recovered from
   * the args remembered at `tool_execution_start`.
   */
  readonly pendingToolArgs: Map<string, unknown>;
  /**
   * Accumulated tool output text by pi tool-call id. pi's
   * `tool_execution_update` carries the *whole* output so far, so the delta to
   * stream is the suffix past the previously-seen text.
   */
  readonly pendingToolOutput: Map<string, string>;
  /** Latest plan steps, so a `patchtodo` result doesn't clear the plan. */
  lastPlanFingerprint: string | undefined;
  /** Turn ids already interrupted; a late `agent_end` must not resurrect them. */
  readonly interruptedTurnIds: Set<TurnId>;
  /**
   * Turns pi actually started an agent run for. Extension commands (`/repos`,
   * `/diff`) execute inside pi without starting one, so they emit no
   * `agent_end` and nothing but us can close their turn.
   */
  readonly agentRunTurnIds: Set<TurnId>;
  /** True while pi is between low-level runs (retry or compaction). */
  betweenAgentRuns: boolean;
  /** Latest low-level run messages, retained until `agent_settled`. */
  lastAgentMessages: unknown;
  stopped: boolean;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parsePiResume(raw: unknown): { readonly sessionFile: string } | undefined {
  if (!isRecord(raw)) return undefined;
  if (raw.schemaVersion !== PI_RESUME_VERSION) return undefined;
  if (typeof raw.sessionFile !== "string" || raw.sessionFile.trim().length === 0) return undefined;
  return { sessionFile: raw.sessionFile.trim() };
}

/**
 * Questions for a `select` / `input` extension dialog.
 *
 * `select` becomes a single-select question over pi's options. `input` has no
 * options, and our user-input surface always renders at least one choice, so it
 * gets a single "Continue" affordance plus the free-text field the panel
 * already offers — that free text is what pi receives.
 */
export function piExtensionUiQuestions(
  request: PiExtensionUiRequest,
): ReadonlyArray<UserInputQuestion> {
  const header = request.title?.trim() || "pi";
  const question = request.method === "input" ? header : request.title?.trim() || "Choose one";
  const options = (request.options ?? [])
    .map((option) => option.trim())
    .filter((option) => option.length > 0);

  return [
    {
      id: request.id,
      header,
      question:
        request.method === "input"
          ? request.placeholder?.trim() || question
          : request.message?.trim() || question,
      multiSelect: false,
      options:
        options.length > 0
          ? options.map((option) => ({ label: option, description: option }))
          : [{ label: "Continue", description: "Submit your answer" }],
    },
  ];
}

/**
 * Whether a turn is already over by the time pi answers its `prompt`.
 *
 * pi answers `prompt` for an extension command (`/repos`, `/diff`) only after
 * the command's handler returns, and such a command never starts an agent run,
 * so no `agent_settled` is coming to close the turn. A steer belongs to a turn
 * someone else owns, an open dialog means the handler is still waiting on the
 * user, and a live agent run closes itself.
 *
 * `agentRunActive` is a thunk so the extra `get_state` round-trip only happens
 * when the cheap local checks have not already ruled a settle out.
 */
export function piShouldSettleTurnAfterPrompt<E, R>(input: {
  readonly isSteer: boolean;
  readonly turnIsStillActive: boolean;
  readonly sawAgentStart: boolean;
  readonly hasPendingDialog: boolean;
  readonly agentRunActive: () => Effect.Effect<boolean, E, R>;
}): Effect.Effect<boolean, E, R> {
  if (input.isSteer || !input.turnIsStillActive) return Effect.succeed(false);
  if (input.sawAgentStart || input.hasPendingDialog) return Effect.succeed(false);
  return Effect.map(input.agentRunActive(), (active) => !active);
}

export function piShouldReportCompaction(
  event: Pick<PiAgentEvent, "aborted" | "errorMessage">,
): boolean {
  return event.aborted !== true && event.errorMessage === undefined;
}

/** A parsed manual `/compact` command. `instructions` is absent for `/compact`. */
export interface PiCompactCommand {
  readonly instructions: string | undefined;
}

/**
 * Parse a manual `/compact` command out of a user turn.
 *
 * pi's built-in `/compact` command never executes through `prompt` (only
 * extension and skill commands do), so the adapter must recognise it and send
 * the dedicated `compact` RPC command instead. Returns `undefined` when the
 * input is not a compact command, so it falls through to a normal prompt.
 */
export function parsePiCompactCommand(input: string | undefined): PiCompactCommand | undefined {
  const text = input?.trim();
  if (text === undefined || text.length === 0) return undefined;
  const match = /^\/compact(?:\s+([\s\S]*))?$/i.exec(text);
  if (!match) return undefined;
  const instructions = match[1]?.trim();
  return {
    instructions: instructions !== undefined && instructions.length > 0 ? instructions : undefined,
  };
}

/**
 * Canonical request type for a gated tool, so the approval card renders with
 * the right affordances instead of a generic tool row.
 */
export function piApprovalRequestType(
  toolName: string | undefined,
): "exec_command_approval" | "file_change_approval" | "dynamic_tool_call" {
  switch (toolName) {
    case "bash":
      return "exec_command_approval";
    case "edit":
    case "write":
      return "file_change_approval";
    default:
      return "dynamic_tool_call";
  }
}

/**
 * Tool name from a `t3-runtime-mode` confirm title.
 *
 * The extension titles its dialogs `Allow <tool>?`. pi's `confirm` carries only
 * a title and message, so that string is the whole channel — matching it is how
 * "accept for session" is scoped to one tool rather than to everything.
 * Returns `undefined` for confirms raised by any other extension, which are
 * then never auto-confirmed.
 */
export function piApprovalToolName(request: PiExtensionUiRequest): string | undefined {
  const match = /^Allow (.+)\?$/.exec(request.title?.trim() ?? "");
  const toolName = match?.[1]?.trim();
  return toolName !== undefined && toolName.length > 0 ? toolName : undefined;
}

/**
 * Translate our answers back into a pi `extension_ui_response`.
 *
 * pi discriminates on the *shape* of the response, not a method field:
 * `confirm` wants `confirmed`, the text/selection dialogs want `value`, and an
 * unanswered dialog wants `cancelled: true`.
 */
export function piExtensionUiResponsePayload(
  request: PiExtensionUiRequest,
  answers: ProviderUserInputAnswers,
): Record<string, unknown> {
  const answer = answers[request.id];
  const value = Array.isArray(answer) ? answer[0] : answer;
  const text = typeof value === "string" ? value.trim() : undefined;

  if (request.method === "confirm") {
    return { confirmed: text !== undefined && text.length > 0 };
  }
  if (text === undefined || text.length === 0) {
    return { cancelled: true };
  }
  return { value: text };
}

export function makePiAdapter(piSettings: PiSettings, options?: PiAdapterLiveOptions) {
  return Effect.gen(function* () {
    const boundInstanceId = options?.instanceId ?? ProviderInstanceId.make("pi");
    const serverConfig = yield* ServerConfig;
    const fileSystem = yield* FileSystem.FileSystem;
    const childProcessSpawner = yield* ChildProcessSpawner.ChildProcessSpawner;
    const crypto = yield* Crypto.Crypto;
    const nativeEventLogger =
      options?.nativeEventLogger ??
      (options?.nativeEventLogPath !== undefined
        ? yield* makeEventNdjsonLogger(options.nativeEventLogPath, { stream: "native" })
        : undefined);
    // Only close a logger we created; an injected one belongs to the caller.
    const managedNativeEventLogger =
      options?.nativeEventLogger === undefined ? nativeEventLogger : undefined;

    const runtimeEvents = yield* Queue.unbounded<ProviderRuntimeEvent>();
    const sessions = new Map<ThreadId, PiSessionContext>();
    const nowIso = Effect.map(DateTime.now, DateTime.formatIso);
    // Resolved once: the path is process-static, and `startSession` must not
    // require FileSystem/Path in its own signature.
    const runtimeModeExtensionPath = yield* resolvePiRuntimeModeExtensionPath();

    const randomUUIDv4 = crypto.randomUUIDv4.pipe(
      Effect.mapError(
        (cause) =>
          new ProviderAdapterRequestError({
            provider: PROVIDER,
            method: "crypto/randomUUIDv4",
            detail: "Failed to generate pi runtime identifier.",
            cause,
          }),
      ),
    );

    const emit = (event: ProviderRuntimeEvent) =>
      Queue.offer(runtimeEvents, event).pipe(Effect.asVoid);

    const buildEventBase = (input: {
      readonly threadId: ThreadId;
      readonly turnId?: TurnId | undefined;
      readonly itemId?: string | undefined;
      readonly requestId?: string | undefined;
      readonly raw?: unknown;
      readonly rawSource?: "pi.rpc.event" | "pi.rpc.extension-ui";
    }) =>
      Effect.all({
        eventId: randomUUIDv4.pipe(Effect.map(EventId.make)),
        createdAt: nowIso,
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
                  source: input.rawSource ?? ("pi.rpc.event" as const),
                  payload: input.raw,
                },
              }
            : {}),
        })),
      );

    const writeNativeEvent = (threadId: ThreadId, payload: unknown) =>
      nativeEventLogger
        ? nowIso.pipe(
            Effect.flatMap((observedAt) =>
              nativeEventLogger.write(
                { observedAt, event: isRecord(payload) ? payload : { payload } },
                threadId,
              ),
            ),
            Effect.catchCause(() => Effect.void),
          )
        : Effect.void;

    const requireContext = (threadId: ThreadId) =>
      Effect.suspend(() => {
        const context = sessions.get(threadId);
        return context === undefined
          ? Effect.fail(new ProviderAdapterSessionNotFoundError({ provider: PROVIDER, threadId }))
          : Effect.succeed(context);
      });

    const toRequestError = (method: string) => (cause: PiRpcError) =>
      new ProviderAdapterRequestError({
        provider: PROVIDER,
        method,
        detail: piRpcErrorDetail(cause),
        cause,
      });

    const updateSession = Effect.fn("updateSession")(function* (
      context: PiSessionContext,
      patch: {
        readonly status?: ProviderSession["status"];
        readonly model?: string | undefined;
        readonly activeTurnId?: TurnId | undefined;
        readonly lastError?: string | undefined;
      },
      flags?: { readonly clearActiveTurnId?: boolean; readonly clearLastError?: boolean },
    ) {
      const updatedAt = yield* nowIso;
      const {
        activeTurnId: currentActiveTurnId,
        lastError: currentLastError,
        ...rest
      } = context.session;
      const nextActiveTurnId = flags?.clearActiveTurnId
        ? undefined
        : (patch.activeTurnId ?? currentActiveTurnId);
      const nextLastError = flags?.clearLastError
        ? undefined
        : (patch.lastError ?? currentLastError);
      context.session = {
        ...rest,
        ...(patch.status ? { status: patch.status } : { status: context.session.status }),
        ...(patch.model !== undefined
          ? { model: patch.model }
          : context.session.model !== undefined
            ? { model: context.session.model }
            : {}),
        ...(nextActiveTurnId ? { activeTurnId: nextActiveTurnId } : {}),
        ...(nextLastError ? { lastError: nextLastError } : {}),
        updatedAt,
      };
    });

    const resumeCursorFor = (context: PiSessionContext) =>
      context.sessionFile === undefined
        ? undefined
        : { schemaVersion: PI_RESUME_VERSION, sessionFile: context.sessionFile };

    /** Cancel every pending dialog so a torn-down session leaves nothing blocked. */
    const cancelPendingExtensionUi = Effect.fn("cancelPendingExtensionUi")(function* (
      context: PiSessionContext,
    ) {
      const pending = [...context.pendingExtensionUi.values()];
      context.pendingExtensionUi.clear();
      yield* Effect.forEach(
        pending,
        (entry) =>
          Effect.gen(function* () {
            yield* context.connection
              .respondToExtensionUi({ id: entry.request.id, cancelled: true })
              .pipe(Effect.ignore);
            yield* emit({
              ...(yield* buildEventBase({
                threadId: context.threadId,
                turnId: context.activeTurnId,
                requestId: entry.requestId,
              })),
              ...(entry.surface === "approval"
                ? {
                    type: "request.resolved" as const,
                    payload: {
                      requestType: piApprovalRequestType(piApprovalToolName(entry.request)),
                      decision: "cancel",
                    },
                  }
                : {
                    type: "user-input.resolved" as const,
                    payload: { answers: {} },
                  }),
            }).pipe(Effect.ignore);
          }),
        { discard: true },
      );
    });

    /**
     * Cancel every pending dialog for a turn and report the requests closed.
     *
     * Needed on interrupt: pi's `abort` stops the agent but leaves an
     * extension's blocking `ctx.ui.*` promise pending, so without this the
     * extension never returns and the turn cannot be closed.
     */
    const cancelPendingExtensionUiForInterrupt = Effect.fn("cancelPendingExtensionUiForInterrupt")(
      function* (context: PiSessionContext) {
        const pending = [...context.pendingExtensionUi.values()];
        if (pending.length === 0) return;
        context.pendingExtensionUi.clear();
        yield* Effect.forEach(
          pending,
          (entry) =>
            Effect.gen(function* () {
              yield* context.connection
                .respondToExtensionUi({ id: entry.request.id, cancelled: true })
                .pipe(Effect.ignore);
              yield* emit({
                ...(yield* buildEventBase({
                  threadId: context.threadId,
                  turnId: context.activeTurnId,
                  requestId: entry.requestId,
                })),
                ...(entry.surface === "approval"
                  ? {
                      type: "request.resolved" as const,
                      payload: {
                        requestType: piApprovalRequestType(piApprovalToolName(entry.request)),
                        decision: "cancel",
                      },
                    }
                  : { type: "user-input.resolved" as const, payload: { answers: {} } }),
              }).pipe(Effect.ignore);
            }),
          { discard: true },
        );
      },
    );

    /**
     * Whether pi has an agent run in flight.
     *
     * `isStreaming` is pi's own `_isAgentRunActive`, set in the same tick as the
     * `prompt` response for a real prompt. An extension command answers `prompt`
     * only after its handler returns and never starts a run, so a `false` here
     * right after `prompt` means the turn is already over.
     */
    const isAgentRunActive = Effect.fn("isAgentRunActive")(function* (context: PiSessionContext) {
      const state = yield* context.connection
        .requestAs("get_state", PiSessionState)
        .pipe(Effect.provideService(Scope.Scope, context.scope), Effect.option);
      // An unreadable state must not strand the turn; assume no run is active
      // so the caller settles rather than waiting forever.
      return Option.isSome(state) ? state.value.isStreaming === true : false;
    });

    const closeOpenAssistantSegment = Effect.fn("closeOpenAssistantSegment")(function* (
      context: PiSessionContext,
      turnId: TurnId,
      raw?: unknown,
    ) {
      const segment = piCloseAssistantSegment(context.assistantSegmentsByTurn, String(turnId));
      if (segment === undefined || segment.text.trim().length === 0) {
        return;
      }
      yield* emit({
        ...(yield* buildEventBase({
          threadId: context.threadId,
          turnId,
          itemId: piAssistantSegmentItemId(String(turnId), segment.segmentIndex),
          raw,
        })),
        type: "item.completed",
        payload: {
          itemType: "assistant_message",
          status: "completed",
          detail: segment.text,
        },
      });
    });

    const settleTurn = Effect.fn("settleTurn")(function* (
      context: PiSessionContext,
      turnId: TurnId,
      outcome:
        | { readonly kind: "completed"; readonly stopReason: string | undefined }
        | { readonly kind: "failed"; readonly errorMessage: string }
        | { readonly kind: "cancelled" },
      raw?: unknown,
    ) {
      if (context.activeTurnId !== turnId) {
        // A late settle for a turn we already closed (interrupt raced the
        // event pump). Emitting again would double-count the turn.
        return;
      }
      context.activeTurnId = undefined;
      context.interruptedTurnIds.delete(turnId);
      context.agentRunTurnIds.delete(turnId);
      context.betweenAgentRuns = false;
      context.lastAgentMessages = undefined;

      yield* updateSession(context, { status: "ready" }, { clearActiveTurnId: true });

      if (outcome.kind === "completed") {
        // The final run's last message may not have reached its `message_end`
        // (interrupt race, dropped event); close it here so the UI does not
        // keep a spinner over an open assistant message.
        yield* closeOpenAssistantSegment(context, turnId, raw);
      }
      context.assistantSegmentsByTurn.delete(String(turnId));

      yield* emit({
        ...(yield* buildEventBase({ threadId: context.threadId, turnId, raw })),
        type: "turn.completed",
        payload:
          outcome.kind === "failed"
            ? { state: "failed", errorMessage: outcome.errorMessage }
            : outcome.kind === "cancelled"
              ? { state: "cancelled", stopReason: "aborted" }
              : {
                  state: piTurnStateFromStopReason(outcome.stopReason),
                  stopReason: outcome.stopReason ?? null,
                },
      });
    });

    const handleExtensionUiRequest = Effect.fn("handleExtensionUiRequest")(function* (
      context: PiSessionContext,
      request: PiExtensionUiRequest,
    ) {
      yield* writeNativeEvent(context.threadId, request);

      if (!isPiExtensionUiDialogMethod(request.method)) {
        // notify / setStatus / setWidget / setTitle / set_editor_text are
        // fire-and-forget: pi does not block on them and sending a response
        // would desynchronize its pending-dialog table.
        if (request.method === "notify") {
          const message = request.message?.trim();
          if (message && message.length > 0) {
            const base = yield* buildEventBase({
              threadId: context.threadId,
              turnId: context.activeTurnId,
              raw: request,
              rawSource: "pi.rpc.extension-ui",
            });
            yield* emit(
              request.notifyType === "error"
                ? { ...base, type: "runtime.error", payload: { message, class: "provider_error" } }
                : { ...base, type: "runtime.warning", payload: { message } },
            );
          }
        }
        return;
      }

      const requestId = ApprovalRequestId.make(request.id);
      // `confirm` is a yes/no decision, which is exactly our approval surface.
      // `select` and `input` need a value, so they go to structured user input.
      const surface = request.method === "confirm" ? "approval" : "user-input";
      const approvalToolName = surface === "approval" ? piApprovalToolName(request) : undefined;

      // A tool the user already approved for the session must not re-prompt;
      // answer pi directly instead of opening a request the UI would show.
      if (approvalToolName !== undefined && context.sessionApprovedTools.has(approvalToolName)) {
        yield* context.connection
          .respondToExtensionUi({ id: request.id, confirmed: true })
          .pipe(Effect.ignore);
        return;
      }

      context.pendingExtensionUi.set(requestId, { requestId, request, surface });

      const base = yield* buildEventBase({
        threadId: context.threadId,
        turnId: context.activeTurnId,
        requestId,
        raw: request,
        rawSource: "pi.rpc.extension-ui",
      });

      if (surface === "approval") {
        const detail = [request.title?.trim(), request.message?.trim()]
          .filter((part): part is string => part !== undefined && part.length > 0)
          .join("\n");
        yield* emit({
          ...base,
          type: "request.opened",
          payload: {
            requestType: piApprovalRequestType(approvalToolName),
            ...(detail.length > 0 ? { detail } : {}),
            args: request,
          },
        });
        return;
      }

      yield* emit({
        ...base,
        type: "user-input.requested",
        payload: { questions: piExtensionUiQuestions(request) },
      });
    });

    const handleAgentEvent = Effect.fn("handleAgentEvent")(function* (
      context: PiSessionContext,
      event: PiAgentEvent,
      raw: unknown,
    ) {
      yield* writeNativeEvent(context.threadId, raw);
      const turnId = context.activeTurnId;

      switch (event.type) {
        case "message_update": {
          const delta = event.assistantMessageEvent?.delta;
          const streamKind = piContentStreamKind(event.assistantMessageEvent?.type ?? "");
          if (delta === undefined || delta.length === 0 || streamKind === undefined) {
            return;
          }
          // Track the open message (and its segment id) only for visible text;
          // thinking deltas stream but never become their own message.
          let segmentItemId: string | undefined;
          if (turnId !== undefined && streamKind === "assistant_text") {
            const segment = piAppendAssistantDelta(
              context.assistantSegmentsByTurn,
              String(turnId),
              delta,
            );
            segmentItemId = piAssistantSegmentItemId(String(turnId), segment.segmentIndex);
          }
          yield* emit({
            ...(yield* buildEventBase({
              threadId: context.threadId,
              turnId,
              ...(segmentItemId !== undefined ? { itemId: segmentItemId } : {}),
              raw,
            })),
            type: "content.delta",
            payload: { streamKind, delta },
          });
          return;
        }

        case "message_end": {
          // pi's message boundary is where the UI needs a turn separator: each
          // assistant message closes here as its own item, so the tool rows
          // that follow land between messages instead of inside one bubble.
          if (turnId !== undefined) {
            yield* closeOpenAssistantSegment(context, turnId, raw);
          }
          return;
        }

        case "tool_execution_start": {
          if (event.toolCallId === undefined) return;
          // Defensive boundary: pi normally closes the assistant message with
          // `message_end` before executing its tool call, but a missing event
          // must not bleed the tool row into the message bubble.
          if (turnId !== undefined) {
            yield* closeOpenAssistantSegment(context, turnId, raw);
          }
          context.pendingToolArgs.set(event.toolCallId, event.args);
          const detail = piToolItemDetail(event.toolName, event.args);
          yield* emit({
            ...(yield* buildEventBase({
              threadId: context.threadId,
              turnId,
              itemId: event.toolCallId,
              raw,
            })),
            type: "item.started",
            payload: {
              itemType: piToolItemType(event.toolName),
              status: "inProgress",
              ...(event.toolName ? { title: event.toolName } : {}),
              ...(detail !== undefined && detail.length > 0 ? { detail } : {}),
              ...(event.args !== undefined ? { data: event.args } : {}),
            },
          });
          return;
        }

        case "tool_execution_update": {
          if (event.toolCallId === undefined) return;
          const accumulated = piToolOutputText(event.partialResult);
          const previous = context.pendingToolOutput.get(event.toolCallId) ?? "";
          const delta = piToolOutputDelta(previous, accumulated);
          context.pendingToolOutput.set(event.toolCallId, accumulated);
          if (delta.length === 0) return;
          yield* emit({
            ...(yield* buildEventBase({
              threadId: context.threadId,
              turnId,
              itemId: event.toolCallId,
              raw,
            })),
            type: "content.delta",
            payload: { streamKind: "command_output", delta },
          });
          return;
        }

        case "tool_execution_end": {
          if (event.toolCallId === undefined) return;
          const args = context.pendingToolArgs.get(event.toolCallId);
          context.pendingToolArgs.delete(event.toolCallId);
          context.pendingToolOutput.delete(event.toolCallId);
          const detail = piToolItemDetail(event.toolName, args);
          yield* emit({
            ...(yield* buildEventBase({
              threadId: context.threadId,
              turnId,
              itemId: event.toolCallId,
              raw,
            })),
            type: "item.completed",
            payload: {
              itemType: piToolItemType(event.toolName),
              status: event.isError === true ? "failed" : "completed",
              ...(event.toolName ? { title: event.toolName } : {}),
              ...(detail !== undefined && detail.length > 0 ? { detail } : {}),
              ...(event.result !== undefined ? { data: event.result } : {}),
            },
          });

          // Todo tools are pi's only structured task list. Their *result*
          // carries the whole reconciled list, so a `patchtodo` still yields a
          // complete plan.
          if (isPiPlanToolName(event.toolName)) {
            const steps = piPlanStepsFromToolResult(event.result);
            if (steps !== undefined && steps.length > 0) {
              const fingerprint = steps.map((step) => `${step.status}:${step.step}`).join("\u0000");
              if (fingerprint !== context.lastPlanFingerprint) {
                context.lastPlanFingerprint = fingerprint;
                yield* emit({
                  ...(yield* buildEventBase({ threadId: context.threadId, turnId, raw })),
                  type: "turn.plan.updated",
                  payload: { plan: steps },
                });
              }
            }
          }
          return;
        }

        case "turn_end": {
          // pi's "turn" is one model round trip inside our turn. Usage on the
          // message is per-request, and the context window is the latest
          // request's totals, not a sum across requests.
          const message = isRecord(event.message) ? event.message : undefined;
          const usage = piUsageSnapshot(
            isRecord(message?.usage) ? (message.usage as never) : undefined,
          );
          if (usage !== undefined) {
            yield* emit({
              ...(yield* buildEventBase({ threadId: context.threadId, turnId, raw })),
              type: "thread.token-usage.updated",
              payload: { usage },
            });
          }
          return;
        }

        case "agent_start": {
          // Records that this turn has a real agent run behind it, so an
          // extension command that never starts one can be settled instead of
          // waiting for an `agent_settled` that will never arrive.
          if (turnId !== undefined) {
            context.agentRunTurnIds.add(turnId);
            // A continuation's fresh run is now executing; it is no longer
            // between low-level runs.
            context.betweenAgentRuns = false;
          }
          return;
        }

        case "agent_end": {
          if (turnId === undefined) return;
          // `agent_end` closes one low-level run, not the session-level turn.
          // Pi may still auto-retry, compact and continue, or drain messages
          // queued by extensions. Keep the latest payload for final text and
          // stop-reason recovery, then settle only at `agent_settled`.
          context.lastAgentMessages = event.messages;
          context.betweenAgentRuns = true;
          return;
        }

        case "agent_settled": {
          if (turnId === undefined) return;
          if (context.interruptedTurnIds.has(turnId)) {
            context.interruptedTurnIds.delete(turnId);
            yield* settleTurn(context, turnId, { kind: "cancelled" }, raw);
            return;
          }

          const messages = Array.isArray(context.lastAgentMessages)
            ? context.lastAgentMessages
            : [];
          const last = messages[messages.length - 1];
          const stopReason =
            isRecord(last) && typeof last.stopReason === "string" ? last.stopReason : undefined;
          // Recover final text when streaming was disabled or a delta was
          // dropped; otherwise the accumulated deltas already hold it.
          if (
            isRecord(last) &&
            !piTurnHasAssistantText(context.assistantSegmentsByTurn, String(turnId))
          ) {
            const text = piAssistantText(piContentBlocks(last.content));
            if (text.length > 0) {
              piAppendAssistantDelta(context.assistantSegmentsByTurn, String(turnId), text);
            }
          }
          yield* settleTurn(context, turnId, { kind: "completed", stopReason }, raw);
          return;
        }

        case "compaction_start": {
          // No canonical "compacting" thread state exists, and compaction is
          // short. Only the completed transition is reported, below.
          return;
        }

        case "compaction_end": {
          // pi also emits compaction_end when summarization fails. Those events
          // have an errorMessage despite aborted=false and did not compact the
          // context, so reporting them as successes creates a row per retry.
          if (!piShouldReportCompaction(event)) {
            return;
          }
          yield* emit({
            ...(yield* buildEventBase({ threadId: context.threadId, turnId, raw })),
            type: "thread.state.changed",
            payload: {
              state: "compacted",
              ...(event.reason !== undefined ? { detail: { reason: event.reason } } : {}),
            },
          });
          return;
        }

        case "auto_retry_start": {
          const message =
            event.errorMessage?.trim() ||
            `Retrying after a provider error (attempt ${event.attempt ?? 1}).`;
          yield* emit({
            ...(yield* buildEventBase({ threadId: context.threadId, turnId, raw })),
            type: "runtime.warning",
            payload: {
              message,
              detail: {
                ...(event.attempt !== undefined ? { attempt: event.attempt } : {}),
                ...(event.maxAttempts !== undefined ? { maxAttempts: event.maxAttempts } : {}),
              },
            },
          });
          return;
        }

        default:
          // pi adds event types between releases; an unknown one is inert.
          return;
      }
    });

    const startEventPump = Effect.fn("startEventPump")(function* (context: PiSessionContext) {
      yield* context.connection.incoming.pipe(
        Stream.runForEach((incoming) => {
          switch (incoming._tag) {
            case "Event":
              return handleAgentEvent(context, incoming.event, incoming.raw);
            case "ExtensionUiRequest":
              return handleExtensionUiRequest(context, incoming.request);
            case "Response":
              // Command responses settle inside the connection; nothing here.
              return Effect.void;
          }
        }),
        Effect.catchCause(() => Effect.void),
        Effect.forkIn(context.scope),
      );

      // pi's exit is terminal for the thread's session. Report it once, then
      // tear down so a follow-up turn starts a fresh process.
      yield* context.connection.awaitExit.pipe(
        Effect.flatMap((error) =>
          Effect.gen(function* () {
            if (context.stopped) return;
            context.stopped = true;
            const activeTurnId = context.activeTurnId;
            sessions.delete(context.threadId);
            if (activeTurnId !== undefined) {
              yield* settleTurn(context, activeTurnId, {
                kind: "failed",
                errorMessage: error.message,
              }).pipe(Effect.ignore);
            }
            yield* emit({
              ...(yield* buildEventBase({ threadId: context.threadId })),
              type: "session.exited",
              payload: {
                reason: error.message,
                recoverable: false,
                exitKind: "error",
              },
            }).pipe(Effect.ignore);
            yield* cancelPendingExtensionUi(context).pipe(Effect.ignore);
            yield* Scope.close(context.scope, Exit.void).pipe(Effect.ignore);
          }),
        ),
        Effect.forkIn(context.scope),
      );
    });

    const stopContext = Effect.fn("stopContext")(function* (context: PiSessionContext) {
      if (context.stopped) return false;
      context.stopped = true;
      yield* cancelPendingExtensionUi(context).pipe(Effect.ignore);
      yield* context.connection.close.pipe(Effect.ignore);
      yield* Scope.close(context.scope, Exit.void).pipe(Effect.ignore);
      return true;
    });

    yield* Effect.addFinalizer(() =>
      Effect.gen(function* () {
        const contexts = [...sessions.values()];
        sessions.clear();
        yield* Effect.forEach(contexts, (context) => Effect.ignoreCause(stopContext(context)), {
          concurrency: "unbounded",
          discard: true,
        });
        if (managedNativeEventLogger !== undefined) {
          yield* managedNativeEventLogger.close();
        }
      }).pipe(Effect.ensuring(Queue.shutdown(runtimeEvents))),
    );

    const startSession: ProviderAdapterShape<ProviderAdapterError>["startSession"] = Effect.fn(
      "startSession",
    )(function* (input) {
      const existing = sessions.get(input.threadId);
      if (existing) {
        yield* stopContext(existing);
        sessions.delete(input.threadId);
      }

      const cwd = input.cwd ?? serverConfig.cwd;
      const resumeSessionFile = parsePiResume(input.resumeCursor)?.sessionFile;
      // Only resume a session file that still exists: pi treats a missing
      // `--session` path as a hard startup error, which would make the thread
      // permanently unstartable after the user cleaned their pi sessions.
      const resumableSessionFile =
        resumeSessionFile !== undefined &&
        (yield* fileSystem.exists(resumeSessionFile).pipe(Effect.orElseSucceed(() => false)))
          ? resumeSessionFile
          : undefined;
      if (resumeSessionFile !== undefined && resumableSessionFile === undefined) {
        yield* Effect.logWarning(
          `pi session file '${resumeSessionFile}' no longer exists; starting a fresh pi session.`,
        );
      }

      // Runtime modes below full-access need the bundled extension: pi's tools
      // are ungated otherwise. A missing file downgrades to full-access with a
      // warning rather than failing the session, since the alternative is a
      // thread that cannot start at all.
      if (input.runtimeMode !== "full-access" && runtimeModeExtensionPath === undefined) {
        yield* Effect.logWarning(
          `pi runtime-mode extension not found; '${input.runtimeMode}' cannot be enforced and pi tools will run without approval prompts.`,
        );
      }

      const launch = resolvePiLaunch({
        piSettings,
        ...(options?.environment ? { environment: options.environment } : {}),
        extraArgs: resumableSessionFile ? ["--session", resumableSessionFile] : [],
        ...(input.runtimeMode !== "full-access" && runtimeModeExtensionPath !== undefined
          ? { extensionPaths: [runtimeModeExtensionPath], runtimeMode: input.runtimeMode }
          : {}),
      });

      const sessionScope = yield* Scope.make();
      const connectionExit = yield* Effect.exit(
        makePiRpcConnection({
          spawn: { command: launch.command, args: launch.args, cwd, env: launch.env },
        }).pipe(
          Effect.provideService(Scope.Scope, sessionScope),
          Effect.provideService(ChildProcessSpawner.ChildProcessSpawner, childProcessSpawner),
        ),
      );
      if (Exit.isFailure(connectionExit)) {
        yield* Scope.close(sessionScope, Exit.void).pipe(Effect.ignore);
        return yield* new ProviderAdapterProcessError({
          provider: PROVIDER,
          threadId: input.threadId,
          detail: "Failed to start pi RPC process.",
          cause: connectionExit.cause,
        });
      }
      const connection = connectionExit.value;

      const stateExit = yield* Effect.exit(
        connection
          .requestAs("get_state", PiSessionState)
          .pipe(Effect.provideService(Scope.Scope, sessionScope)),
      );
      if (Exit.isFailure(stateExit)) {
        yield* connection.close.pipe(Effect.ignore);
        yield* Scope.close(sessionScope, Exit.void).pipe(Effect.ignore);
        return yield* new ProviderAdapterProcessError({
          provider: PROVIDER,
          threadId: input.threadId,
          detail: "pi RPC process started but did not report its session state.",
          cause: stateExit.cause,
        });
      }
      const state = stateExit.value;

      const createdAt = yield* nowIso;
      const session: ProviderSession = {
        provider: PROVIDER,
        providerInstanceId: boundInstanceId,
        status: "ready",
        runtimeMode: input.runtimeMode,
        cwd,
        ...(input.modelSelection?.instanceId === boundInstanceId
          ? { model: input.modelSelection.model }
          : {}),
        threadId: input.threadId,
        ...(state.sessionFile
          ? {
              resumeCursor: {
                schemaVersion: PI_RESUME_VERSION,
                sessionFile: state.sessionFile,
              },
            }
          : {}),
        createdAt,
        updatedAt: createdAt,
      };

      const context: PiSessionContext = {
        threadId: input.threadId,
        session,
        scope: sessionScope,
        connection,
        sessionFile: state.sessionFile,
        activeTurnId: undefined,
        assistantSegmentsByTurn: new Map(),
        pendingExtensionUi: new Map(),
        sessionApprovedTools: new Set(),
        pendingToolArgs: new Map(),
        pendingToolOutput: new Map(),
        lastPlanFingerprint: undefined,
        interruptedTurnIds: new Set(),
        agentRunTurnIds: new Set(),
        betweenAgentRuns: false,
        lastAgentMessages: undefined,
        stopped: false,
      };
      sessions.set(input.threadId, context);
      yield* startEventPump(context);

      yield* emit({
        ...(yield* buildEventBase({ threadId: input.threadId })),
        type: "session.started",
        payload: {
          message: "pi session started",
          ...(resumableSessionFile ? { resume: { sessionFile: resumableSessionFile } } : {}),
        },
      });
      yield* emit({
        ...(yield* buildEventBase({ threadId: input.threadId })),
        type: "thread.started",
        payload: { providerThreadId: state.sessionId },
      });

      return session;
    });

    /**
     * Apply model and thinking level before prompting.
     *
     * Both are session-scoped in pi, so they must be (re)asserted per turn: the
     * user can change either between turns without restarting the session.
     */
    const applySelection = Effect.fn("applySelection")(function* (
      context: PiSessionContext,
      input: ProviderSendTurnInput,
    ) {
      const modelSelection =
        input.modelSelection ??
        (context.session.model
          ? { instanceId: boundInstanceId, model: context.session.model }
          : undefined);
      if (modelSelection !== undefined && modelSelection.instanceId !== boundInstanceId) {
        return yield* new ProviderAdapterValidationError({
          provider: PROVIDER,
          operation: "sendTurn",
          issue: `pi model selection is bound to instance '${modelSelection.instanceId}', expected '${boundInstanceId}'.`,
        });
      }

      const parsedModel = parsePiModelSlug(modelSelection?.model);
      if (modelSelection !== undefined && parsedModel === undefined) {
        return yield* new ProviderAdapterValidationError({
          provider: PROVIDER,
          operation: "sendTurn",
          issue: "pi model selection must use the 'provider/model' format.",
        });
      }

      if (parsedModel !== undefined) {
        yield* context.connection
          .request("set_model", { provider: parsedModel.provider, modelId: parsedModel.modelId })
          .pipe(Effect.mapError(toRequestError("set_model")));
      }

      const thinkingLevel = piThinkingLevelFromSelection(modelSelection);
      if (thinkingLevel !== undefined) {
        yield* context.connection
          .request("set_thinking_level", { level: thinkingLevel })
          .pipe(Effect.mapError(toRequestError("set_thinking_level")));
      }

      return modelSelection?.model;
    });

    /**
     * Run pi's built-in `/compact` command as its dedicated RPC command.
     *
     * `/compact` never executes through `prompt`, and it does not start an
     * agent run, so this opens a turn, awaits the compaction result, and
     * settles the turn itself rather than waiting for an `agent_settled` that
     * will never arrive. The event pump still reports the compaction via the
     * `compaction_end` event's `thread.state.changed` transition.
     */
    const sendCompactTurn = Effect.fn("sendCompactTurn")(function* (
      context: PiSessionContext,
      input: ProviderSendTurnInput,
      instructions: string | undefined,
    ) {
      if (context.activeTurnId !== undefined) {
        return yield* new ProviderAdapterValidationError({
          provider: PROVIDER,
          operation: "sendTurn",
          issue: "Cannot compact while a turn is running.",
        });
      }

      const turnId = TurnId.make(`pi-turn-${yield* randomUUIDv4}`);
      context.activeTurnId = turnId;
      yield* updateSession(
        context,
        { status: "running", activeTurnId: turnId },
        { clearLastError: true },
      );
      yield* emit({
        ...(yield* buildEventBase({ threadId: input.threadId, turnId })),
        type: "turn.started",
        payload: {},
      });

      yield* context.connection
        .request("compact", instructions !== undefined ? { customInstructions: instructions } : {})
        .pipe(
          Effect.mapError(toRequestError("compact")),
          Effect.tapError((requestError) =>
            Effect.gen(function* () {
              context.activeTurnId = undefined;
              yield* updateSession(
                context,
                { status: "ready", lastError: requestError.detail },
                { clearActiveTurnId: true },
              );
              yield* emit({
                ...(yield* buildEventBase({ threadId: input.threadId, turnId })),
                type: "turn.aborted",
                payload: { reason: requestError.detail },
              });
            }),
          ),
        );

      yield* settleTurn(context, turnId, { kind: "completed", stopReason: undefined });

      const resumeCursor = resumeCursorFor(context);
      return {
        threadId: input.threadId,
        turnId,
        ...(resumeCursor ? { resumeCursor } : {}),
      };
    });

    const sendTurn: ProviderAdapterShape<ProviderAdapterError>["sendTurn"] = Effect.fn("sendTurn")(
      function* (input) {
        const context = yield* requireContext(input.threadId);
        const text = input.input?.trim();

        const compactCommand = parsePiCompactCommand(text);
        if (compactCommand !== undefined) {
          return yield* sendCompactTurn(context, input, compactCommand.instructions);
        }

        const images = yield* readAttachmentImages(input);

        if ((text === undefined || text.length === 0) && images.length === 0) {
          return yield* new ProviderAdapterValidationError({
            provider: PROVIDER,
            operation: "sendTurn",
            issue: "pi turns require text input or at least one attachment.",
          });
        }

        const model = yield* applySelection(context, input);

        // A prompt during an active turn is a steer: pi folds it into the
        // running turn, so reusing the turn id keeps the UI on one turn.
        const steeringTurnId = context.activeTurnId;
        const turnId = steeringTurnId ?? TurnId.make(`pi-turn-${yield* randomUUIDv4}`);
        context.activeTurnId = turnId;
        yield* updateSession(
          context,
          { status: "running", activeTurnId: turnId, ...(model ? { model } : {}) },
          { clearLastError: true },
        );

        if (steeringTurnId === undefined) {
          yield* emit({
            ...(yield* buildEventBase({ threadId: input.threadId, turnId })),
            type: "turn.started",
            payload: model ? { model } : {},
          });
        }

        yield* context.connection
          .request("prompt", {
            message: text ?? "",
            ...(images.length > 0 ? { images } : {}),
            ...(steeringTurnId === undefined ? {} : { streamingBehavior: "steer" }),
          })
          .pipe(
            Effect.mapError(toRequestError("prompt")),
            // A failed *fresh* prompt closes the turn it opened. A failed steer
            // leaves the still-running original turn alone.
            Effect.tapError((requestError) =>
              steeringTurnId !== undefined
                ? Effect.void
                : Effect.gen(function* () {
                    context.activeTurnId = undefined;
                    yield* updateSession(
                      context,
                      { status: "ready", lastError: requestError.detail },
                      { clearActiveTurnId: true },
                    );
                    yield* emit({
                      ...(yield* buildEventBase({ threadId: input.threadId, turnId })),
                      type: "turn.aborted",
                      payload: { reason: requestError.detail },
                    });
                  }),
            ),
          );

        // pi answers `prompt` for an extension command (`/repos`, `/diff`) only
        // after the command's handler returns, and such a command never starts
        // an agent run. With no `agent_settled` coming, close the turn here or
        // the UI spins forever.
        if (
          yield* piShouldSettleTurnAfterPrompt({
            isSteer: steeringTurnId !== undefined,
            turnIsStillActive: context.activeTurnId === turnId,
            sawAgentStart: context.agentRunTurnIds.has(turnId),
            hasPendingDialog: context.pendingExtensionUi.size > 0,
            agentRunActive: () => isAgentRunActive(context),
          })
        ) {
          yield* settleTurn(context, turnId, { kind: "completed", stopReason: undefined });
        }

        const resumeCursor = resumeCursorFor(context);
        return {
          threadId: input.threadId,
          turnId,
          ...(resumeCursor ? { resumeCursor } : {}),
        };
      },
    );

    /**
     * Read image attachments into pi's inline `ImageContent` shape.
     *
     * pi's RPC `prompt` takes base64 image data inline; it has no file-path
     * form, so attachments must be read here rather than referenced.
     */
    const readAttachmentImages = Effect.fn("readAttachmentImages")(function* (
      input: ProviderSendTurnInput,
    ): Effect.fn.Return<
      ReadonlyArray<{ readonly type: "image"; readonly data: string; readonly mimeType: string }>,
      ProviderAdapterRequestError
    > {
      const attachments = input.attachments ?? [];
      const images: Array<{
        readonly type: "image";
        readonly data: string;
        readonly mimeType: string;
      }> = [];
      for (const attachment of attachments) {
        const attachmentPath = resolveAttachmentPath({
          attachmentsDir: serverConfig.attachmentsDir,
          attachment,
        });
        if (attachmentPath === null) {
          return yield* new ProviderAdapterRequestError({
            provider: PROVIDER,
            method: "prompt",
            detail: `Invalid attachment id '${attachment.id}'.`,
          });
        }
        const bytes = yield* fileSystem.readFile(attachmentPath).pipe(
          Effect.mapError(
            (cause) =>
              new ProviderAdapterRequestError({
                provider: PROVIDER,
                method: "prompt",
                detail: `Failed to read attachment file: ${cause.message}.`,
                cause,
              }),
          ),
        );
        images.push({
          type: "image",
          data: Buffer.from(bytes).toString("base64"),
          mimeType: attachment.mimeType,
        });
      }
      return images;
    });

    const interruptTurn: ProviderAdapterShape<ProviderAdapterError>["interruptTurn"] = Effect.fn(
      "interruptTurn",
    )(function* (threadId, turnId) {
      const context = yield* requireContext(threadId);
      const targetTurnId = turnId ?? context.activeTurnId;
      if (targetTurnId !== undefined) {
        // Remember the interrupt so the `agent_settled` that follows reports a
        // cancellation instead of a normal completion.
        context.interruptedTurnIds.add(targetTurnId);
      }

      // Cancel dialogs first: pi's `abort` awaits idle, and an extension
      // blocked on `ctx.ui.confirm` keeps the session busy until it is answered.
      yield* cancelPendingExtensionUiForInterrupt(context);

      yield* context.connection.request("abort").pipe(Effect.mapError(toRequestError("abort")));

      if (targetTurnId === undefined) return;

      yield* emit({
        ...(yield* buildEventBase({ threadId, turnId: targetTurnId })),
        type: "turn.aborted",
        payload: { reason: "Interrupted by user." },
      });

      // `turn.aborted` is informational; only `turn.completed` clears the
      // thread's active turn. A turn with an agent run behind it gets that from
      // the `agent_settled` the abort triggers. An extension command has no run,
      // so it would hang here — close it now.
      // The same holds between low-level runs: abort can cancel retry backoff
      // or compaction before another run exists to emit `agent_settled`, so
      // close it directly.
      if (!context.agentRunTurnIds.has(targetTurnId) || context.betweenAgentRuns) {
        yield* settleTurn(context, targetTurnId, { kind: "cancelled" });
      }
    });

    const respondToRequest: ProviderAdapterShape<ProviderAdapterError>["respondToRequest"] =
      Effect.fn("respondToRequest")(function* (threadId, requestId, decision) {
        const context = yield* requireContext(threadId);
        const pending = context.pendingExtensionUi.get(requestId);
        if (pending === undefined) {
          return yield* new ProviderAdapterRequestError({
            provider: PROVIDER,
            method: "extension_ui_response",
            detail: `Unknown pending pi request: ${requestId}`,
          });
        }
        context.pendingExtensionUi.delete(requestId);

        const approvalToolName = piApprovalToolName(pending.request);
        if (decision === "acceptForSession" && approvalToolName !== undefined) {
          // pi's confirm channel has no "always" reply, so the session-wide
          // decision is kept here and applied to later confirms for this tool.
          context.sessionApprovedTools.add(approvalToolName);
        }

        const confirmed = decision === "accept" || decision === "acceptForSession";
        yield* context.connection
          .respondToExtensionUi(
            decision === "cancel"
              ? { id: pending.request.id, cancelled: true }
              : { id: pending.request.id, confirmed },
          )
          .pipe(Effect.mapError(toRequestError("extension_ui_response")));

        yield* emit({
          ...(yield* buildEventBase({
            threadId,
            turnId: context.activeTurnId,
            requestId,
          })),
          type: "request.resolved",
          payload: { requestType: piApprovalRequestType(approvalToolName), decision },
        });
      });

    const respondToUserInput: ProviderAdapterShape<ProviderAdapterError>["respondToUserInput"] =
      Effect.fn("respondToUserInput")(function* (threadId, requestId, answers) {
        const context = yield* requireContext(threadId);
        const pending = context.pendingExtensionUi.get(requestId);
        if (pending === undefined) {
          return yield* new ProviderAdapterRequestError({
            provider: PROVIDER,
            method: "extension_ui_response",
            detail: `Unknown pending pi user-input request: ${requestId}`,
          });
        }
        context.pendingExtensionUi.delete(requestId);

        yield* context.connection
          .respondToExtensionUi({
            id: pending.request.id,
            ...piExtensionUiResponsePayload(pending.request, answers),
          })
          .pipe(Effect.mapError(toRequestError("extension_ui_response")));

        yield* emit({
          ...(yield* buildEventBase({
            threadId,
            turnId: context.activeTurnId,
            requestId,
          })),
          type: "user-input.resolved",
          payload: { answers },
        });
      });

    const stopSession: ProviderAdapterShape<ProviderAdapterError>["stopSession"] = Effect.fn(
      "stopSession",
    )(function* (threadId) {
      const context = sessions.get(threadId);
      if (context === undefined) {
        return yield* new ProviderAdapterSessionNotFoundError({ provider: PROVIDER, threadId });
      }
      sessions.delete(threadId);
      const stopped = yield* stopContext(context);
      if (!stopped) return;
      yield* emit({
        ...(yield* buildEventBase({ threadId })),
        type: "session.exited",
        payload: { reason: "Session stopped.", recoverable: false, exitKind: "graceful" },
      });
    });

    const listSessions: ProviderAdapterShape<ProviderAdapterError>["listSessions"] = () =>
      Effect.sync(() => [...sessions.values()].map((context) => context.session));

    const hasSession: ProviderAdapterShape<ProviderAdapterError>["hasSession"] = (threadId) =>
      Effect.sync(() => sessions.has(threadId));

    const readThread: ProviderAdapterShape<ProviderAdapterError>["readThread"] = Effect.fn(
      "readThread",
    )(function* (threadId) {
      const context = yield* requireContext(threadId);
      const entries = yield* context.connection
        .requestAs("get_entries", PiEntries)
        .pipe(Effect.mapError(toRequestError("get_entries")));

      // pi's history is a flat entry list, not turns. Group by user message:
      // each user entry opens a turn that runs until the next user entry, which
      // is the same boundary the UI shows.
      const turns: Array<{ readonly id: TurnId; readonly items: ReadonlyArray<unknown> }> = [];
      let current: { id: TurnId; items: Array<unknown> } | undefined;
      for (const entry of entries.entries) {
        const message = isRecord(entry.message) ? entry.message : undefined;
        const role = typeof message?.role === "string" ? message.role : undefined;
        if (role === "user") {
          if (current) turns.push(current);
          current = { id: TurnId.make(entry.id), items: [entry] };
          continue;
        }
        if (current === undefined) {
          // Pre-turn entries (system prompt, context files) belong to no turn.
          continue;
        }
        current.items.push(entry);
      }
      if (current) turns.push(current);

      return { threadId, turns } satisfies ProviderThreadSnapshot;
    });

    const rollbackThread: ProviderAdapterShape<ProviderAdapterError>["rollbackThread"] = Effect.fn(
      "rollbackThread",
    )(function* (threadId, numTurns) {
      const context = yield* requireContext(threadId);
      const snapshot = yield* readThread(threadId);
      const targetIndex = snapshot.turns.length - numTurns;
      const target = targetIndex >= 0 ? snapshot.turns[targetIndex] : undefined;
      if (target === undefined) {
        return snapshot;
      }
      // pi forks rather than truncating: `fork` re-roots the session before the
      // named user entry, which is exactly "roll back to just before this turn".
      yield* context.connection
        .request("fork", { entryId: target.id })
        .pipe(Effect.mapError(toRequestError("fork")));

      // The fork replaces the session file, so refresh the cursor or the next
      // resume would reopen the pre-rollback history.
      const state = yield* context.connection
        .requestAs("get_state", PiSessionState)
        .pipe(Effect.mapError(toRequestError("get_state")));
      context.sessionFile = state.sessionFile;

      return yield* readThread(threadId);
    });

    const stopAll: ProviderAdapterShape<ProviderAdapterError>["stopAll"] = () =>
      Effect.gen(function* () {
        const contexts = [...sessions.values()];
        sessions.clear();
        yield* Effect.forEach(contexts, (context) => Effect.ignoreCause(stopContext(context)), {
          concurrency: "unbounded",
          discard: true,
        });
      });

    return {
      provider: PROVIDER,
      capabilities: {
        // `set_model` applies to the live session, so no new thread is needed.
        sessionModelSwitch: "in-session",
      },
      startSession,
      sendTurn,
      interruptTurn,
      respondToRequest,
      respondToUserInput,
      stopSession,
      listSessions,
      hasSession,
      readThread,
      rollbackThread,
      stopAll,
      get streamEvents() {
        return Stream.fromQueue(runtimeEvents);
      },
    } satisfies ProviderAdapterShape<ProviderAdapterError>;
  });
}
