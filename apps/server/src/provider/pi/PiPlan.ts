/**
 * PiPlan — derive plan steps from pi todo-tool results.
 *
 * pi has no first-party plan channel. Structured task lists come from todo
 * tools (`todowrite` / `patchtodo`, shipped by the bundled S5 Code extension
 * and by pi's own todo packages), which return the full list on every call as
 * `result.details.todos`. Reading the tool result rather than the tool
 * *arguments* matters for `patchtodo`: the arguments only carry the single
 * patched field, while the result carries the whole reconciled list.
 *
 * Statuses are the ACP plan vocabulary already used by the Grok and Cursor
 * adapters, so the existing plan UI renders these with no client change.
 *
 * @module provider/pi/PiPlan
 */
import type { RuntimePlanStepStatus } from "@t3tools/contracts";

/** Tool names whose results carry a full todo list. */
const PLAN_TOOL_NAMES = new Set(["todowrite", "patchtodo", "todoread", "read_todo"]);

export function isPiPlanToolName(toolName: string | undefined): boolean {
  return toolName !== undefined && PLAN_TOOL_NAMES.has(toolName.trim().toLowerCase());
}

export interface PiPlanStep {
  readonly step: string;
  readonly status: RuntimePlanStepStatus;
}

function planStatus(value: unknown): RuntimePlanStepStatus {
  switch (typeof value === "string" ? value.trim().toLowerCase() : "") {
    case "in_progress":
    case "inprogress":
      return "inProgress";
    case "completed":
    case "done":
      return "completed";
    default:
      return "pending";
  }
}

function readTodoArray(value: unknown): ReadonlyArray<unknown> | undefined {
  if (value === null || typeof value !== "object") {
    return undefined;
  }
  const todos = (value as { readonly todos?: unknown }).todos;
  return Array.isArray(todos) ? todos : undefined;
}

/**
 * Extract plan steps from a pi tool result.
 *
 * Looks for `details.todos` first (the structured channel), then falls back to
 * a JSON array in the result's text content, which is what a todo tool without
 * a `details` payload emits. Returns `undefined` when neither is present so the
 * caller can leave the plan untouched instead of clearing it.
 */
export function piPlanStepsFromToolResult(result: unknown): ReadonlyArray<PiPlanStep> | undefined {
  if (result === null || typeof result !== "object") {
    return undefined;
  }
  const record = result as { readonly details?: unknown; readonly content?: unknown };
  const todos = readTodoArray(record.details) ?? todosFromContent(record.content);
  if (todos === undefined) {
    return undefined;
  }

  const steps: Array<PiPlanStep> = [];
  for (const entry of todos) {
    if (entry === null || typeof entry !== "object") continue;
    const todo = entry as { readonly content?: unknown; readonly status?: unknown };
    const step = typeof todo.content === "string" ? todo.content.trim() : "";
    if (step.length === 0) continue;
    steps.push({ step, status: planStatus(todo.status) });
  }
  return steps;
}

function todosFromContent(content: unknown): ReadonlyArray<unknown> | undefined {
  if (!Array.isArray(content)) {
    return undefined;
  }
  for (const block of content) {
    if (block === null || typeof block !== "object") continue;
    const text = (block as { readonly text?: unknown }).text;
    if (typeof text !== "string") continue;
    const trimmed = text.trim();
    if (!trimmed.startsWith("[") && !trimmed.startsWith("{")) continue;
    try {
      const parsed: unknown = JSON.parse(trimmed);
      if (Array.isArray(parsed)) {
        return parsed;
      }
      const nested = readTodoArray(parsed);
      if (nested) {
        return nested;
      }
    } catch {
      // Not JSON; the tool printed prose. Keep scanning the other blocks.
    }
  }
  return undefined;
}
