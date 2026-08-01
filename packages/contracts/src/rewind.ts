/**
 * Session rewind — opencode-style per-turn undo/redo backed by shadow-git
 * snapshots kept outside the user's repository.
 *
 * This is deliberately separate from the built-in checkpoint/revert flow:
 *
 *   - Checkpoint revert (`thread.checkpoint.revert`) is destructive and
 *     forward-only. It restores the whole worktree from a hidden ref inside
 *     the user's repo and drops newer messages, turns, and activities.
 *   - Rewind is reversible. Each turn records a before/after tree pair in a
 *     per-thread shadow git store, and undo/redo restore **only the files
 *     that turn touched**. The transcript is never truncated, so undo can
 *     always be redone.
 *
 * @module rewind
 */
import * as Schema from "effect/Schema";

import { IsoDateTime, MessageId, NonNegativeInt, ThreadId, TurnId } from "./baseSchemas.ts";

export const REWIND_WS_METHODS = {
  getStatus: "rewind.getStatus",
  undo: "rewind.undo",
  redo: "rewind.redo",
} as const;

/**
 * Relative POSIX path of a file captured in a rewind snapshot. Absolute
 * paths, `..` traversal, and the `.t3` state directory are rejected at
 * capture time, so consumers can treat these as repo-relative.
 */
export const RewindFilePath = Schema.String.check(Schema.isNonEmpty());
export type RewindFilePath = typeof RewindFilePath.Type;

export const RewindEntryState = Schema.Literals(["applied", "undone"]);
export type RewindEntryState = typeof RewindEntryState.Type;

/** One captured turn in a thread's rewind history. */
export const RewindEntry = Schema.Struct({
  threadId: ThreadId,
  turnId: TurnId,
  /** Monotonic per-thread capture order. Undo pops the highest applied. */
  sequence: NonNegativeInt,
  userMessageId: Schema.NullOr(MessageId),
  assistantMessageId: Schema.NullOr(MessageId),
  /** Prompt text that opened the turn, used for undo/redo affordance labels. */
  prompt: Schema.String,
  files: Schema.Array(RewindFilePath),
  state: RewindEntryState,
  createdAt: IsoDateTime,
  updatedAt: IsoDateTime,
});
export type RewindEntry = typeof RewindEntry.Type;

/**
 * Undo/redo affordance state for one thread.
 *
 * `available` is false when the experiment is off, when the thread has no
 * workspace, or when the workspace could not be snapshotted — the clients
 * hide the controls entirely in that case rather than showing dead buttons.
 */
export const RewindStatus = Schema.Struct({
  threadId: ThreadId,
  available: Schema.Boolean,
  undo: Schema.NullOr(RewindEntry),
  redo: Schema.NullOr(RewindEntry),
  appliedCount: NonNegativeInt,
  undoneCount: NonNegativeInt,
});
export type RewindStatus = typeof RewindStatus.Type;

export const RewindGetStatusInput = Schema.Struct({
  threadId: ThreadId,
});
export type RewindGetStatusInput = typeof RewindGetStatusInput.Type;

export const RewindStepInput = Schema.Struct({
  threadId: ThreadId,
});
export type RewindStepInput = typeof RewindStepInput.Type;

export const RewindStepOutcome = Schema.Literals([
  "applied",
  "nothing-to-do",
  "unavailable",
  "busy",
]);
export type RewindStepOutcome = typeof RewindStepOutcome.Type;

export const RewindStepResult = Schema.Struct({
  outcome: RewindStepOutcome,
  /** Files written or removed while restoring. Empty for no-op outcomes. */
  restoredFiles: Schema.Array(RewindFilePath),
  /** Prompt of the turn that moved, for the confirmation copy. */
  prompt: Schema.NullOr(Schema.String),
  status: RewindStatus,
});
export type RewindStepResult = typeof RewindStepResult.Type;

export class RewindError extends Schema.TaggedErrorClass<RewindError>()("RewindError", {
  operation: Schema.String,
  threadId: Schema.optional(ThreadId),
  detail: Schema.String,
  cause: Schema.optional(Schema.Defect()),
}) {
  override get message(): string {
    return `Rewind failed in ${this.operation}: ${this.detail}`;
  }
}
