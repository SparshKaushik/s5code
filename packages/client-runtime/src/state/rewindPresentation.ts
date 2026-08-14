/**
 * Copy and enablement rules for the session rewind undo/redo affordances.
 *
 * Pure functions so every client surface (web toolbar buttons, mobile header
 * menu) labels the same turn identically. Keep platform primitives out of this
 * module — callers map the returned strings onto their own controls.
 *
 * @module state/rewindPresentation
 */
import type { RewindStatus, RewindStepOutcome, RewindStepResult } from "@t3tools/contracts";

const MAX_PROMPT_LABEL_LENGTH = 60;

export type RewindDirection = "undo" | "redo";

export interface RewindActionDescription {
  readonly enabled: boolean;
  /** Long form, for tooltips and menu subtitles. */
  readonly tooltip: string;
  /** Short form, for `aria-label` / `accessibilityLabel`. */
  readonly ariaLabel: string;
  /**
   * Verb-free description of the target turn, for surfaces that already show
   * "Undo"/"Redo" as the control label (native menu rows). Doubles as the
   * reason a direction is disabled.
   */
  readonly detail: string;
}

export interface RewindStepNotification {
  readonly type: "success" | "info";
  readonly title: string;
  readonly description: string;
}

/** Single-line, length-capped prompt text for the undo/redo label. */
export function formatRewindPromptLabel(prompt: string): string {
  const collapsed = prompt.replaceAll(/\s+/g, " ").trim();
  if (collapsed.length === 0) return "";
  return collapsed.length <= MAX_PROMPT_LABEL_LENGTH
    ? collapsed
    : `${collapsed.slice(0, MAX_PROMPT_LABEL_LENGTH - 1)}…`;
}

function formatFileCount(count: number): string {
  return `${count} ${count === 1 ? "file" : "files"}`;
}

export function describeRewindAction(input: {
  readonly direction: RewindDirection;
  readonly status: RewindStatus | null;
}): RewindActionDescription {
  const verb = input.direction === "undo" ? "Undo" : "Redo";
  if (input.status === null || !input.status.available) {
    return {
      enabled: false,
      tooltip: `${verb} is unavailable for this thread`,
      ariaLabel: `${verb} unavailable`,
      detail: "Unavailable for this thread",
    };
  }
  const entry = input.direction === "undo" ? input.status.undo : input.status.redo;
  if (entry === null) {
    const nothing = input.direction === "undo" ? "Nothing to undo" : "Nothing to redo";
    return { enabled: false, tooltip: nothing, ariaLabel: nothing, detail: nothing };
  }
  const label = formatRewindPromptLabel(entry.prompt);
  const files = formatFileCount(entry.files.length);
  return {
    enabled: true,
    tooltip: label.length > 0 ? `${verb} "${label}" (${files})` : `${verb} last turn (${files})`,
    ariaLabel: label.length > 0 ? `${verb} ${label}` : `${verb} last turn`,
    detail: label.length > 0 ? `"${label}" · ${files}` : `Last turn · ${files}`,
  };
}

/** Notification copy for a completed step. `null` means stay silent. */
export function describeRewindStepResult(input: {
  readonly direction: RewindDirection;
  readonly result: RewindStepResult;
}): RewindStepNotification | null {
  const verb = input.direction === "undo" ? "Undo" : "Redo";
  const outcomeCopy: Record<RewindStepOutcome, string | null> = {
    applied: null,
    "nothing-to-do": input.direction === "undo" ? "Nothing to undo." : "Nothing to redo.",
    unavailable: "Session rewind is turned off for this server.",
    busy: "Another rewind step is still running.",
  };

  if (input.result.outcome !== "applied") {
    const description = outcomeCopy[input.result.outcome];
    return description === null ? null : { type: "info", title: `${verb} skipped`, description };
  }

  const files = formatFileCount(input.result.restoredFiles.length);
  const label = formatRewindPromptLabel(input.result.prompt ?? "");
  return {
    type: "success",
    title: input.direction === "undo" ? "Turn undone" : "Turn redone",
    description: label.length > 0 ? `Restored ${files} from "${label}".` : `Restored ${files}.`,
  };
}

/** Failure copy for a rejected step, shared by every client surface. */
export function describeRewindStepFailure(input: {
  readonly direction: RewindDirection;
  readonly failure: unknown;
}): { readonly title: string; readonly description: string } {
  const message =
    input.failure instanceof Error && input.failure.message.trim().length > 0
      ? input.failure.message
      : "The workspace was not changed.";
  return {
    title: input.direction === "undo" ? "Undo failed" : "Redo failed",
    description: message,
  };
}

/**
 * The prompt to hand back to the composer after an undo step, or `null` when
 * the composer should be left untouched.
 *
 * Undo restores files; returning the prompt lets the user edit and resend the
 * turn without retyping it. Redo and non-applied outcomes never repopulate
 * the composer, and empty prompts are treated as "nothing to restore".
 */
export function undonePromptForComposer(input: {
  readonly direction: RewindDirection;
  readonly result: RewindStepResult;
}): string | null {
  if (input.direction !== "undo" || input.result.outcome !== "applied") {
    return null;
  }
  const prompt = input.result.prompt;
  return prompt !== null && prompt.trim().length > 0 ? prompt : null;
}
