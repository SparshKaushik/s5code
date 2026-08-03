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
 * Whether a terminal `agent_end` should settle the turn now rather than defer.
 *
 * pi retries transient provider errors (e.g. 5xx) by continuing the same turn
 * with a fresh agent run. The `agent_end` for the failed attempt carries
 * `willRetry: true`; settling then would mark the turn failed before the retry
 * runs, even when the retry completes. So a `willRetry` end defers settling
 * until the later run's `agent_end` (willRetry unset/false).
 */
export function piShouldSettleTurnOnAgentEnd(willRetry: boolean | undefined): boolean {
  return willRetry !== true;
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
