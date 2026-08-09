/**
 * RewindEntryRepository — persistence interface for session rewind history.
 *
 * One row per captured turn, keyed by `{threadId, turnId}`. Tree oids point
 * into the thread's shadow git store (`storeId`), never into the user's
 * repository, so nothing here can resurrect objects the user deleted.
 *
 * @module RewindEntryRepository
 */
import {
  IsoDateTime,
  MessageId,
  NonNegativeInt,
  RewindEntryState,
  RewindFilePath,
  ThreadId,
  TrimmedNonEmptyString,
  TurnId,
} from "@t3tools/contracts";
import * as Context from "effect/Context";
import type * as Effect from "effect/Effect";
import * as Option from "effect/Option";
import * as Schema from "effect/Schema";

import type { ProjectionRepositoryError } from "../Errors.ts";

export const RewindEntryRow = Schema.Struct({
  threadId: ThreadId,
  turnId: TurnId,
  sequence: NonNegativeInt,
  /** Stable id of the shadow git store holding `beforeTree` / `afterTree`. */
  storeId: TrimmedNonEmptyString,
  /** Workspace the snapshot was taken from, used to restore into. */
  cwd: TrimmedNonEmptyString,
  userMessageId: Schema.NullOr(MessageId),
  assistantMessageId: Schema.NullOr(MessageId),
  prompt: Schema.String,
  beforeTree: TrimmedNonEmptyString,
  afterTree: TrimmedNonEmptyString,
  files: Schema.Array(RewindFilePath),
  state: RewindEntryState,
  createdAt: IsoDateTime,
  updatedAt: IsoDateTime,
});
export type RewindEntryRow = typeof RewindEntryRow.Type;

export const ThreadRewindInput = Schema.Struct({
  threadId: ThreadId,
});
export type ThreadRewindInput = typeof ThreadRewindInput.Type;

export const SetRewindEntryStateInput = Schema.Struct({
  threadId: ThreadId,
  turnId: TurnId,
  state: RewindEntryState,
  updatedAt: IsoDateTime,
});
export type SetRewindEntryStateInput = typeof SetRewindEntryStateInput.Type;

export const DeleteRewindEntriesByStoreInput = Schema.Struct({
  storeId: TrimmedNonEmptyString,
});
export type DeleteRewindEntriesByStoreInput = typeof DeleteRewindEntriesByStoreInput.Type;

export const RewindStoreSummary = Schema.Struct({
  storeId: TrimmedNonEmptyString,
  threadId: ThreadId,
  cwd: TrimmedNonEmptyString,
  entryCount: NonNegativeInt,
  updatedAt: IsoDateTime,
});
export type RewindStoreSummary = typeof RewindStoreSummary.Type;

export interface RewindEntryRepositoryShape {
  /** Insert or replace the capture record for one turn. */
  readonly upsert: (row: RewindEntryRow) => Effect.Effect<void, ProjectionRepositoryError>;

  /** All entries for a thread, oldest first. */
  readonly listByThreadId: (
    input: ThreadRewindInput,
  ) => Effect.Effect<ReadonlyArray<RewindEntryRow>, ProjectionRepositoryError>;

  /** Highest-sequence entry that is currently `applied`, i.e. the undo target. */
  readonly getUndoCandidate: (
    input: ThreadRewindInput,
  ) => Effect.Effect<Option.Option<RewindEntryRow>, ProjectionRepositoryError>;

  /** Lowest-sequence entry that is currently `undone`, i.e. the redo target. */
  readonly getRedoCandidate: (
    input: ThreadRewindInput,
  ) => Effect.Effect<Option.Option<RewindEntryRow>, ProjectionRepositoryError>;

  readonly setState: (
    input: SetRewindEntryStateInput,
  ) => Effect.Effect<void, ProjectionRepositoryError>;

  /**
   * Drop every `undone` entry for a thread.
   *
   * Called when a new turn is captured: once the agent has worked on top of
   * an undone state, redoing those turns would clobber the newer work, so the
   * redo path must disappear rather than stay available and be wrong.
   */
  readonly deleteUndoneByThreadId: (
    input: ThreadRewindInput,
  ) => Effect.Effect<void, ProjectionRepositoryError>;

  readonly deleteByThreadId: (
    input: ThreadRewindInput,
  ) => Effect.Effect<void, ProjectionRepositoryError>;

  readonly deleteByStoreId: (
    input: DeleteRewindEntriesByStoreInput,
  ) => Effect.Effect<void, ProjectionRepositoryError>;

  /** One row per shadow store, for storage reporting and cleanup. */
  readonly listStoreSummaries: () => Effect.Effect<
    ReadonlyArray<RewindStoreSummary>,
    ProjectionRepositoryError
  >;
}

export class RewindEntryRepository extends Context.Service<
  RewindEntryRepository,
  RewindEntryRepositoryShape
>()("t3/persistence/Services/RewindEntries/RewindEntryRepository") {}
