/**
 * PiTurnState — pure mapping helpers shared by `PiAdapter` and its tests.
 *
 * pi's event stream is flat and untyped-by-design; turning it into canonical
 * runtime events needs a handful of decisions that are easier to reason about
 * (and to test) as pure functions than as branches inside the adapter's
 * event pump.
 *
 * @module provider/pi/PiTurnState
 */
import type {
  RuntimeContentStreamKind,
  RuntimeTurnState,
  ToolLifecycleItemType,
} from "@t3tools/contracts";

import type { PiAssistantContentBlock, PiTokenUsage } from "./PiRpcSchemas.ts";

/**
 * Map a pi tool name onto a canonical item type. pi ships a small fixed set of
 * built-ins; anything else is an extension tool and lands on
 * `dynamic_tool_call` so the UI still renders it as a tool row.
 */
export function piToolItemType(toolName: string | undefined): ToolLifecycleItemType {
  switch (toolName?.trim().toLowerCase()) {
    case "bash":
      return "command_execution";
    case "edit":
    case "write":
      return "file_change";
    default:
      // Includes pi's read-only built-ins (read/grep/find/ls) and every
      // extension tool. They render as generic tool rows, which is accurate:
      // none of them map onto a more specific canonical lifecycle.
      return "dynamic_tool_call";
  }
}

/**
 * Which canonical stream a `message_update` delta belongs to. pi separates
 * thinking from visible text at the event level, so no heuristics are needed.
 */
/**
 * Display detail for a completed tool item.
 *
 * pi's `tool_execution_end` event carries only the result, not the input that
 * produced it, so the command (or edited path) has to be recovered from the
 * args remembered at `tool_execution_start`. Without it the work log would
 * render a bare tool-name row with nothing to preview or expand.
 */
export function piToolItemDetail(toolName: string | undefined, args: unknown): string | undefined {
  const record = piToolArgsRecord(args);
  if (record === null) {
    return undefined;
  }
  switch (toolName?.trim().toLowerCase()) {
    case "bash": {
      const command = typeof record.command === "string" ? record.command.trim() : undefined;
      return command !== undefined && command.length > 0 ? command : undefined;
    }
    case "edit":
    case "write": {
      const path = typeof record.path === "string" ? record.path.trim() : undefined;
      return path !== undefined && path.length > 0 ? path : undefined;
    }
    case "read": {
      // The whole point of the row is the file being read, so the path is the
      // detail; offset/limit are windowing on that path, not a separate row.
      const path = typeof record.path === "string" ? record.path.trim() : undefined;
      return path !== undefined && path.length > 0 ? path : undefined;
    }
    case "grep": {
      // "pattern in path" — path is optional (defaults to cwd), pattern is not.
      const pattern = typeof record.pattern === "string" ? record.pattern.trim() : undefined;
      if (pattern === undefined || pattern.length === 0) {
        return undefined;
      }
      const path = typeof record.path === "string" ? record.path.trim() : undefined;
      return path !== undefined && path.length > 0 ? `${pattern} in ${path}` : pattern;
    }
    case "find":
    case "ls": {
      const path = typeof record.path === "string" ? record.path.trim() : undefined;
      return path !== undefined && path.length > 0 ? path : undefined;
    }
    case "todowrite": {
      // The args carry the whole list, so a count is the honest single-line
      // summary; the full steps live in the plan panel via `details.todos`.
      const todos = Array.isArray(record.todos) ? record.todos : undefined;
      if (todos === undefined || todos.length === 0) {
        return undefined;
      }
      return `${todos.length} todo${todos.length === 1 ? "" : "s"}`;
    }
    case "patchtodo": {
      // Only the patched fields are in the args; name the target so the row
      // reads as a change rather than a bare tool call.
      const id = typeof record.id === "number" ? record.id : undefined;
      const status = typeof record.status === "string" ? record.status.trim() : undefined;
      const priority = typeof record.priority === "string" ? record.priority.trim() : undefined;
      const content = typeof record.content === "string" ? record.content.trim() : undefined;
      const changes = [status, priority, content].filter(
        (change): change is string => change !== undefined && change.length > 0,
      );
      const target = id !== undefined ? `#${id}` : "";
      return changes.length > 0 ? `${target} ${changes.join(", ")}`.trim() : undefined;
    }
    default:
      return undefined;
  }
}

function piToolArgsRecord(args: unknown): Record<string, unknown> | null {
  if (typeof args === "string" && args.trim().length > 0) {
    try {
      const parsed: unknown = JSON.parse(args);
      if (isPiObjectRecord(parsed)) {
        return parsed;
      }
    } catch {
      return null;
    }
    return null;
  }
  return isPiObjectRecord(args) ? args : null;
}

function isPiObjectRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function piContentStreamKind(deltaType: string): RuntimeContentStreamKind | undefined {
  switch (deltaType) {
    case "text_delta":
      return "assistant_text";
    case "thinking_delta":
      return "reasoning_text";
    default:
      return undefined;
  }
}

/**
 * Terminal turn state from the assistant message's stop reason.
 *
 * `aborted` is the only reason that maps to a user-visible cancellation; the
 * others are ordinary completions except `error`, which we surface as failed so
 * the thread shows an error affordance instead of a silently short turn.
 */
export function piTurnStateFromStopReason(stopReason: string | undefined): RuntimeTurnState {
  switch (stopReason) {
    case "aborted":
      return "cancelled";
    case "error":
      return "failed";
    default:
      return "completed";
  }
}

/**
 * One assistant message inside one of our turns.
 *
 * pi emits a separate assistant message per text block between tool calls (or
 * between turns), so each becomes its own canonical `assistant_message` item -
 * the same shape the ACP adapters (Cursor, Grok) produce. `segmentIndex` is
 * the message's 0-based position within our turn and gives it a stable item id.
 */
export interface PiAssistantSegment {
  readonly segmentIndex: number;
  text: string;
}

/**
 * Streaming state for one of our turns: already-finalized messages plus the
 * message currently accumulating text. Closed segments are only counted, not
 * kept - their text already went out as `item.completed` payloads.
 */
export interface PiTurnAssistantSegments {
  /** Messages already closed at their message boundary. */
  readonly closedCount: number;
  /** The message currently streaming text, if any. */
  open: PiAssistantSegment | undefined;
}

/**
 * Canonical item id for a pi assistant message within one of our turns.
 *
 * Segment 0 keeps the historical `assistant-<turn>` id so message ids stay
 * stable for turns already persisted; later messages extend it with a segment
 * suffix so each one lands on its own assistant message in the read model.
 */
export function piAssistantSegmentItemId(turnId: string, segmentIndex: number): string {
  return segmentIndex === 0 ? `assistant-${turnId}` : `assistant-${turnId}:seg:${segmentIndex}`;
}

/** The message currently streaming text for a turn, if any. */
export function piOpenAssistantSegment(
  segmentsByTurn: ReadonlyMap<string, PiTurnAssistantSegments>,
  turnId: string,
): PiAssistantSegment | undefined {
  return segmentsByTurn.get(turnId)?.open;
}

/** Whether any assistant text has accumulated for a turn (streamed or recovered). */
export function piTurnHasAssistantText(
  segmentsByTurn: ReadonlyMap<string, PiTurnAssistantSegments>,
  turnId: string,
): boolean {
  const state = segmentsByTurn.get(turnId);
  if (state === undefined) {
    return false;
  }
  return state.closedCount > 0 || state.open !== undefined;
}

/**
 * Append a streamed delta to a turn's open message, opening a new one when the
 * previous message was closed (or nothing has streamed yet). Returns the
 * segment the delta landed in so the caller can tag `content.delta` with it.
 */
export function piAppendAssistantDelta(
  segmentsByTurn: Map<string, PiTurnAssistantSegments>,
  turnId: string,
  delta: string,
): PiAssistantSegment {
  const state = segmentsByTurn.get(turnId);
  const nextOpen = state?.open
    ? { ...state.open, text: state.open.text + delta }
    : { segmentIndex: state?.closedCount ?? 0, text: delta };
  segmentsByTurn.set(turnId, { closedCount: state?.closedCount ?? 0, open: nextOpen });
  return nextOpen;
}

/**
 * Close a turn's open message, returning it so the caller can emit its
 * terminal item. No-op (returns `undefined`) when nothing is streaming.
 */
export function piCloseAssistantSegment(
  segmentsByTurn: Map<string, PiTurnAssistantSegments>,
  turnId: string,
): PiAssistantSegment | undefined {
  const state = segmentsByTurn.get(turnId);
  if (state === undefined || state.open === undefined) {
    return undefined;
  }
  segmentsByTurn.set(turnId, { closedCount: state.closedCount + 1, open: undefined });
  return state.open;
}

/** Concatenated visible text of an assistant message's content blocks. */
export function piAssistantText(blocks: ReadonlyArray<PiAssistantContentBlock>): string {
  return blocks
    .filter((block) => block.type === "text")
    .map((block) => block.text ?? "")
    .join("");
}

/**
 * Normalize a message's `content` into content blocks. pi allows a bare string
 * for user messages and an array of blocks for assistant messages.
 */
export function piContentBlocks(content: unknown): ReadonlyArray<PiAssistantContentBlock> {
  if (typeof content === "string") {
    return [{ type: "text", text: content }];
  }
  if (!Array.isArray(content)) {
    return [];
  }
  return content.flatMap((entry) => {
    if (entry === null || typeof entry !== "object") return [];
    const record = entry as Record<string, unknown>;
    const type = typeof record.type === "string" ? record.type : undefined;
    if (type === undefined) return [];
    return [
      {
        type,
        ...(typeof record.text === "string" ? { text: record.text } : {}),
        ...(typeof record.thinking === "string" ? { thinking: record.thinking } : {}),
        ...(typeof record.id === "string" ? { id: record.id } : {}),
        ...(typeof record.name === "string" ? { name: record.name } : {}),
        ...(record.arguments !== undefined ? { arguments: record.arguments } : {}),
      } satisfies PiAssistantContentBlock,
    ];
  });
}

/**
 * Accumulated output text of a pi tool result or partial result.
 *
 * pi's `tool_execution_update` carries `partialResult` with the *accumulated*
 * output so far (not a delta), and `tool_execution_end` carries the final
 * `result` in the same `{ content, details }` shape. Both are read through the
 * same content-block normalizer used for assistant messages, so the text is
 * the concatenation of their text blocks.
 */
export function piToolOutputText(result: unknown): string {
  if (!isPiObjectRecord(result)) return "";
  return piAssistantText(piContentBlocks(result.content));
}

/**
 * Delta to emit for a `tool_execution_update`.
 *
 * pi appends, so the delta is the suffix past the previously-seen text. A
 * non-prefix (truncated or restarted buffer) falls back to the whole
 * accumulated text so no output is silently dropped.
 */
export function piToolOutputDelta(previous: string, accumulated: string): string {
  return accumulated.startsWith(previous) ? accumulated.slice(previous.length) : accumulated;
}

export interface PiUsageSnapshot {
  readonly usedTokens: number;
  readonly inputTokens?: number;
  readonly cachedInputTokens?: number;
  readonly outputTokens?: number;
}

/**
 * Context-window snapshot from a pi usage record.
 *
 * pi reports per-request usage, and the context window is the *last* request's
 * input plus cache reads plus output — not a running sum. Summing across
 * requests would inflate the context gauge to many times the window.
 */
export function piUsageSnapshot(usage: PiTokenUsage | undefined): PiUsageSnapshot | undefined {
  if (usage === undefined) {
    return undefined;
  }
  const input = usage.input ?? 0;
  const cacheRead = usage.cacheRead ?? 0;
  const output = usage.output ?? 0;
  const usedTokens = input + cacheRead + output;
  if (usedTokens <= 0) {
    return undefined;
  }
  return {
    usedTokens,
    ...(usage.input === undefined ? {} : { inputTokens: usage.input }),
    ...(usage.cacheRead === undefined ? {} : { cachedInputTokens: usage.cacheRead }),
    ...(usage.output === undefined ? {} : { outputTokens: usage.output }),
  };
}
