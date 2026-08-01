/**
 * RewindService — per-turn undo/redo policy for session rewind.
 *
 * Capture records a before/after snapshot pair per turn plus the file list
 * that turn actually touched. Undo restores the "before" side of the newest
 * applied entry; redo restores the "after" side of the oldest undone entry.
 * Because both sides are retained, undo is always reversible — unlike the
 * built-in checkpoint revert, which truncates the transcript.
 *
 * Deliberate scoping decisions:
 *
 *   - **Files, not worktrees.** Only paths in the turn's diff are written.
 *     The user's index and unrelated untracked files survive.
 *   - **Transcript untouched.** Rewind never dispatches orchestration
 *     commands, so messages, turns, and activities are preserved. The agent's
 *     conversation is not rolled back; this is a filesystem-level undo.
 *   - **Serialized per thread.** A semaphore per thread prevents an undo and
 *     a capture from interleaving mid-restore.
 *   - **New prompts invalidate redo.** Once a new turn is captured, undone
 *     entries below it are dropped from the redo queue, matching editor
 *     undo semantics.
 *
 * @module rewind/RewindService
 */
import {
  RewindError,
  type MessageId,
  type RewindEntry,
  type RewindStatus,
  type RewindStepResult,
  type ThreadId,
  type TurnId,
} from "@t3tools/contracts";
import * as Context from "effect/Context";
import * as DateTime from "effect/DateTime";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Schema from "effect/Schema";
import * as Semaphore from "effect/Semaphore";

import { RewindEntryRepository } from "../persistence/Services/RewindEntries.ts";
import type { RewindEntryRow } from "../persistence/Services/RewindEntries.ts";
import * as RewindStore from "./RewindStore.ts";
import { ServerSettingsService } from "../serverSettings.ts";

export interface CaptureRewindTurnInput {
  readonly threadId: ThreadId;
  readonly turnId: TurnId;
  readonly cwd: string;
  readonly userMessageId: MessageId | null;
  readonly assistantMessageId: MessageId | null;
  readonly prompt: string;
}

export const unavailableStatus = (threadId: ThreadId): RewindStatus => ({
  threadId,
  available: false,
  undo: null,
  redo: null,
  appliedCount: 0,
  undoneCount: 0,
});

export function toRewindEntry(row: RewindEntryRow): RewindEntry {
  return {
    threadId: row.threadId,
    turnId: row.turnId,
    sequence: row.sequence,
    userMessageId: row.userMessageId,
    assistantMessageId: row.assistantMessageId,
    prompt: row.prompt,
    files: row.files,
    state: row.state,
    createdAt: row.createdAt,
    updatedAt: row.updatedAt,
  };
}

/**
 * Build the undo/redo affordance state from a thread's entries.
 *
 * Undo targets the newest `applied` entry; redo targets the oldest `undone`
 * entry. Pure so the ordering rules can be unit-tested without a database.
 */
export function deriveStatus(input: {
  readonly threadId: ThreadId;
  readonly available: boolean;
  readonly entries: ReadonlyArray<RewindEntryRow>;
}): RewindStatus {
  const applied = input.entries.filter((entry) => entry.state === "applied");
  const undone = input.entries.filter((entry) => entry.state === "undone");
  const undoTarget = applied.reduce<RewindEntryRow | null>(
    (newest, entry) => (newest === null || entry.sequence > newest.sequence ? entry : newest),
    null,
  );
  const redoTarget = undone.reduce<RewindEntryRow | null>(
    (oldest, entry) => (oldest === null || entry.sequence < oldest.sequence ? entry : oldest),
    null,
  );

  return {
    threadId: input.threadId,
    available: input.available,
    undo: undoTarget === null ? null : toRewindEntry(undoTarget),
    redo: redoTarget === null ? null : toRewindEntry(redoTarget),
    appliedCount: applied.length,
    undoneCount: undone.length,
  };
}

export class RewindService extends Context.Service<
  RewindService,
  {
    /** True when the session rewind experiment is enabled in server settings. */
    readonly isEnabled: Effect.Effect<boolean, RewindError>;

    /**
     * Snapshot the workspace before a turn runs, remembering it as the
     * "before" side for the next `captureTurn` on this thread. Returns the
     * tree oid, or `null` when rewind is disabled.
     */
    readonly beginTurn: (input: {
      readonly threadId: ThreadId;
      readonly cwd: string;
    }) => Effect.Effect<string | null, RewindError>;

    /**
     * Record the after-snapshot and the files the turn touched.
     *
     * Returns `None` when rewind is disabled, when the turn changed nothing,
     * or when no "before" side is recoverable (see the fallback in the
     * implementation).
     */
    readonly captureTurn: (
      input: CaptureRewindTurnInput,
    ) => Effect.Effect<Option.Option<RewindEntry>, RewindError>;

    readonly getStatus: (threadId: ThreadId) => Effect.Effect<RewindStatus, RewindError>;

    readonly undo: (threadId: ThreadId) => Effect.Effect<RewindStepResult, RewindError>;

    readonly redo: (threadId: ThreadId) => Effect.Effect<RewindStepResult, RewindError>;

    /** Drop a thread's rewind history and shadow store. Returns bytes freed. */
    readonly forgetThread: (threadId: ThreadId) => Effect.Effect<number, RewindError>;
  }
>()("t3/rewind/RewindService") {}

const isRewindError = Schema.is(RewindError);

const toRewindError = (operation: string, threadId?: ThreadId) => (cause: unknown) =>
  isRewindError(cause)
    ? cause
    : new RewindError({
        operation,
        ...(threadId === undefined ? {} : { threadId }),
        detail: cause instanceof Error ? cause.message : "Rewind operation failed.",
        cause,
      });

export const make = Effect.gen(function* () {
  const repository = yield* RewindEntryRepository;
  const store = yield* RewindStore.RewindStore;
  const serverSettings = yield* ServerSettingsService;

  // One permit per thread: undo/redo/capture must never interleave, or a
  // restore could be snapshotted as if it were the agent's own edit.
  const threadLocks = new Map<ThreadId, Semaphore.Semaphore>();
  const getThreadLock = (threadId: ThreadId) =>
    Effect.suspend(() => {
      const existing = threadLocks.get(threadId);
      if (existing) {
        return Effect.succeed(existing);
      }
      return Semaphore.make(1).pipe(
        Effect.tap((created) => Effect.sync(() => threadLocks.set(threadId, created))),
      );
    });
  const withThreadLock = <A, E, R>(threadId: ThreadId, effect: Effect.Effect<A, E, R>) =>
    Effect.flatMap(getThreadLock(threadId), (lock) => lock.withPermit(effect));

  const isEnabled = serverSettings.getSettings.pipe(
    Effect.map((settings) => settings.experimental.sessionRewindEnabled),
    Effect.mapError(toRewindError("RewindService.isEnabled")),
  );

  const nowIso = Effect.map(DateTime.now, DateTime.formatIso);

  // Pre-turn snapshot per thread, set by `beginTurn` and consumed by
  // `captureTurn`. Intentionally in-memory: a server restart mid-turn should
  // fall back to the previous entry's snapshot rather than resurrect a tree
  // that no longer reflects the workspace.
  const pendingBeforeTreeByThread = new Map<ThreadId, string>();

  const beginTurn: RewindService["Service"]["beginTurn"] = Effect.fn("RewindService.beginTurn")(
    function* (input) {
      if (!(yield* isEnabled)) {
        return null;
      }
      return yield* withThreadLock(
        input.threadId,
        Effect.gen(function* () {
          const handle = yield* store.open({ threadId: input.threadId, cwd: input.cwd });
          const tree = yield* store.snapshot(handle);
          pendingBeforeTreeByThread.set(input.threadId, tree);
          return tree;
        }),
      );
    },
  );

  const runCaptureTurn = Effect.fn("RewindService.captureTurn")(function* (
    input: CaptureRewindTurnInput,
  ) {
    if (!(yield* isEnabled)) {
      return Option.none<RewindEntry>();
    }

    return yield* withThreadLock(
      input.threadId,
      Effect.gen(function* () {
        const existing = yield* repository.listByThreadId({ threadId: input.threadId });
        // Capture is idempotent per turn: the reactor may see both a runtime
        // and a domain completion for the same turn, and re-capturing would
        // overwrite a correct before/after pair with a degenerate one.
        if (existing.some((entry) => entry.turnId === input.turnId)) {
          pendingBeforeTreeByThread.delete(input.threadId);
          return Option.none<RewindEntry>();
        }

        // Prefer the snapshot `beginTurn` took. Falling back to the previous
        // turn's after-tree keeps undo working across a server restart, at the
        // cost of folding any manual edits made between turns into this
        // entry's undo scope.
        const beforeTree =
          pendingBeforeTreeByThread.get(input.threadId) ??
          existing.reduce<{ readonly sequence: number; readonly afterTree: string } | null>(
            (newest, entry) =>
              newest === null || entry.sequence > newest.sequence
                ? { sequence: entry.sequence, afterTree: entry.afterTree }
                : newest,
            null,
          )?.afterTree;
        if (beforeTree === undefined) {
          return Option.none<RewindEntry>();
        }

        const handle = yield* store.open({ threadId: input.threadId, cwd: input.cwd });
        const afterTree = yield* store.snapshot(handle);
        const files = yield* store.changedFiles({
          handle,
          fromTree: beforeTree,
          toTree: afterTree,
        });
        pendingBeforeTreeByThread.delete(input.threadId);
        if (files.length === 0) {
          // A turn that changed nothing is not worth an undo step; recording
          // it would make undo look available and then do nothing.
          return Option.none<RewindEntry>();
        }

        const nextSequence = existing.reduce((max, entry) => Math.max(max, entry.sequence + 1), 0);
        const createdAt = yield* nowIso;
        const row: RewindEntryRow = {
          threadId: input.threadId,
          turnId: input.turnId,
          sequence: nextSequence,
          storeId: handle.storeId,
          cwd: input.cwd,
          userMessageId: input.userMessageId,
          assistantMessageId: input.assistantMessageId,
          prompt: input.prompt,
          beforeTree,
          afterTree,
          files,
          state: "applied",
          createdAt,
          updatedAt: createdAt,
        };
        yield* repository.upsert(row);

        // A fresh turn commits the branch: previously undone turns can no
        // longer be redone on top of it (redoing would clobber this turn's
        // work), so they are dropped rather than left as a wrong-but-clickable
        // redo. Same rule as an editor discarding redo after a new edit.
        yield* repository.deleteUndoneByThreadId({ threadId: input.threadId });

        return Option.some(toRewindEntry(row));
      }),
    );
  });

  const captureTurn: RewindService["Service"]["captureTurn"] = (input) =>
    runCaptureTurn(input).pipe(
      Effect.mapError(toRewindError("RewindService.captureTurn", input.threadId)),
    );

  const readStatus = Effect.fn("RewindService.readStatus")(function* (threadId: ThreadId) {
    const available = yield* isEnabled;
    if (!available) {
      return unavailableStatus(threadId);
    }
    const entries = yield* repository.listByThreadId({ threadId });
    return deriveStatus({ threadId, available: true, entries });
  });

  const getStatus: RewindService["Service"]["getStatus"] = (threadId) =>
    readStatus(threadId).pipe(Effect.mapError(toRewindError("RewindService.getStatus", threadId)));

  const step = Effect.fn("RewindService.step")(function* (input: {
    readonly threadId: ThreadId;
    readonly direction: "undo" | "redo";
  }) {
    if (!(yield* isEnabled)) {
      return {
        outcome: "unavailable" as const,
        restoredFiles: [],
        prompt: null,
        status: unavailableStatus(input.threadId),
      } satisfies RewindStepResult;
    }

    return yield* withThreadLock(
      input.threadId,
      Effect.gen(function* () {
        const candidate =
          input.direction === "undo"
            ? yield* repository.getUndoCandidate({ threadId: input.threadId })
            : yield* repository.getRedoCandidate({ threadId: input.threadId });

        if (Option.isNone(candidate)) {
          return {
            outcome: "nothing-to-do" as const,
            restoredFiles: [],
            prompt: null,
            status: yield* readStatus(input.threadId),
          } satisfies RewindStepResult;
        }

        const entry = candidate.value;
        const handle = yield* store.open({ threadId: input.threadId, cwd: entry.cwd });
        const restoredFiles = yield* store.restoreFiles({
          handle,
          tree: input.direction === "undo" ? entry.beforeTree : entry.afterTree,
          files: entry.files,
        });
        const updatedAt = yield* nowIso;
        yield* repository.setState({
          threadId: entry.threadId,
          turnId: entry.turnId,
          state: input.direction === "undo" ? "undone" : "applied",
          updatedAt,
        });

        return {
          outcome: "applied" as const,
          restoredFiles,
          prompt: entry.prompt,
          status: yield* readStatus(input.threadId),
        } satisfies RewindStepResult;
      }),
    );
  });

  const undo: RewindService["Service"]["undo"] = (threadId) =>
    step({ threadId, direction: "undo" }).pipe(
      Effect.mapError(toRewindError("RewindService.undo", threadId)),
    );

  const redo: RewindService["Service"]["redo"] = (threadId) =>
    step({ threadId, direction: "redo" }).pipe(
      Effect.mapError(toRewindError("RewindService.redo", threadId)),
    );

  const forgetThread: RewindService["Service"]["forgetThread"] = Effect.fn(
    "RewindService.forgetThread",
  )(function* (threadId) {
    pendingBeforeTreeByThread.delete(threadId);
    const storeId = yield* store.storeIdForThread(threadId);
    yield* repository
      .deleteByThreadId({ threadId })
      .pipe(Effect.mapError(toRewindError("RewindService.forgetThread", threadId)));
    return yield* store.deleteStore(storeId);
  });

  return RewindService.of({
    isEnabled,
    beginTurn,
    captureTurn,
    getStatus,
    undo,
    redo,
    forgetThread,
  });
});

export const layer = Layer.effect(RewindService, make);
