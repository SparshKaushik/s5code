/**
 * Checkpoint maintenance — inspect and reclaim checkpoint storage.
 *
 * Covers hidden `refs/t3/checkpoints/<thread>/turn/<n>` commits written into
 * each project repository by the built-in checkpoint flow.
 *
 * Nothing here ever touches a user branch, tag, stash, or remote ref.
 *
 * @module checkpointMaintenance
 */
import * as Schema from "effect/Schema";

import { IsoDateTime, NonNegativeInt, ThreadId, TrimmedNonEmptyString } from "./baseSchemas.ts";

export const CHECKPOINT_MAINTENANCE_WS_METHODS = {
  getUsage: "checkpointMaintenance.getUsage",
  cleanup: "checkpointMaintenance.cleanup",
} as const;

/**
 * Ref namespace for built-in checkpoints. Everything under it is created and
 * owned by S5 Code, which is what makes bulk deletion safe: no user branch,
 * tag, stash, or remote-tracking ref can live here.
 */
export const CHECKPOINT_REFS_PREFIX = "refs/t3/checkpoints";

export const CheckpointStorageKind = Schema.Literals(["checkpoint-refs"]);
export type CheckpointStorageKind = typeof CheckpointStorageKind.Type;

/**
 * One reclaimable unit: all checkpoint refs for a thread inside one
 * repository.
 */
export const CheckpointStorageEntry = Schema.Struct({
  kind: CheckpointStorageKind,
  threadId: Schema.NullOr(ThreadId),
  /** Human label for the UI: thread title, or the repository path. */
  label: TrimmedNonEmptyString,
  /** Repository path this entry lives in. */
  location: TrimmedNonEmptyString,
  /** Refs for `checkpoint-refs`. */
  refCount: NonNegativeInt,
  bytes: NonNegativeInt,
  /** Newest checkpoint timestamp in this entry, when it can be determined. */
  updatedAt: Schema.NullOr(IsoDateTime),
  /** True when the owning thread no longer exists in the read model. */
  orphaned: Schema.Boolean,
});
export type CheckpointStorageEntry = typeof CheckpointStorageEntry.Type;

export const CheckpointStorageUsage = Schema.Struct({
  generatedAt: IsoDateTime,
  entries: Schema.Array(CheckpointStorageEntry),
  totalBytes: NonNegativeInt,
  orphanedBytes: NonNegativeInt,
});
export type CheckpointStorageUsage = typeof CheckpointStorageUsage.Type;

export const CheckpointMaintenanceGetUsageInput = Schema.Struct({});
export type CheckpointMaintenanceGetUsageInput = typeof CheckpointMaintenanceGetUsageInput.Type;

/**
 * Cleanup scope.
 *
 * - `orphaned`: only entries whose thread is gone. Always safe.
 * - `retention-policy`: apply the configured age and size limits, plus
 *   orphans. Never removes the newest entry for a live thread.
 * - `all`: every checkpoint ref, including live threads.
 *   Callers must confirm this in the UI; it disables revert for
 *   existing threads.
 */
export const CheckpointCleanupScope = Schema.Literals(["orphaned", "retention-policy", "all"]);
export type CheckpointCleanupScope = typeof CheckpointCleanupScope.Type;

export const CheckpointMaintenanceCleanupInput = Schema.Struct({
  scope: CheckpointCleanupScope,
  /** Report what would be deleted without deleting anything. */
  dryRun: Schema.optionalKey(Schema.Boolean),
});
export type CheckpointMaintenanceCleanupInput = typeof CheckpointMaintenanceCleanupInput.Type;

export const CheckpointCleanupResult = Schema.Struct({
  scope: CheckpointCleanupScope,
  dryRun: Schema.Boolean,
  removedEntries: Schema.Array(CheckpointStorageEntry),
  removedRefCount: NonNegativeInt,
  reclaimedBytes: NonNegativeInt,
  usage: CheckpointStorageUsage,
});
export type CheckpointCleanupResult = typeof CheckpointCleanupResult.Type;

export class CheckpointMaintenanceError extends Schema.TaggedErrorClass<CheckpointMaintenanceError>()(
  "CheckpointMaintenanceError",
  {
    operation: Schema.String,
    detail: Schema.String,
    cause: Schema.optional(Schema.Defect()),
  },
) {
  override get message(): string {
    return `Checkpoint maintenance failed in ${this.operation}: ${this.detail}`;
  }
}
