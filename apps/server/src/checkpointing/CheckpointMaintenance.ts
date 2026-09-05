/**
 * CheckpointMaintenance — report and reclaim checkpoint storage.
 *
 * Covers stores S5 Code creates and nothing else:
 *
 *   - hidden `refs/t3/checkpoints/**` commits inside each project repository
 *     (built-in checkpoint/revert).
 *
 * Safety rules that are enforced here rather than left to callers:
 *
 *   1. Only refs under the S5 Code namespace are ever listed or deleted. A user
 *      branch, tag, stash, or remote-tracking ref cannot be selected.
 *   2. Reported bytes are "reachable only from these refs", so a number can
 *      never imply that deleting checkpoints would drop objects a branch
 *      still needs.
 *   3. For threads that still exist, the newest checkpoint entry is retained
 *      by the age and size policies, so an active thread never silently loses
 *      its ability to revert. Only the explicit `all` scope removes it.
 *
 * @module checkpointing/CheckpointMaintenance
 */
import {
  CheckpointMaintenanceError,
  type CheckpointCleanupResult,
  type CheckpointMaintenanceCleanupInput,
  type CheckpointRef,
  type CheckpointStorageEntry,
  type CheckpointStorageUsage,
  type ThreadId,
} from "@t3tools/contracts";
import * as Clock from "effect/Clock";
import * as Context from "effect/Context";
import * as DateTime from "effect/DateTime";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";

import * as CheckpointStore from "./CheckpointStore.ts";
import { parseCheckpointRef } from "./Utils.ts";
import { ProjectionSnapshotQuery } from "../orchestration/Services/ProjectionSnapshotQuery.ts";
import { ServerSettingsService } from "../serverSettings.ts";

const BYTES_PER_MEGABYTE = 1024 * 1024;

interface LiveThread {
  readonly threadId: ThreadId;
  readonly title: string;
  readonly cwd: string | undefined;
}

/** Entry ordering for eviction: oldest first, unknown timestamps first. */
export function compareByAgeAscending(
  left: CheckpointStorageEntry,
  right: CheckpointStorageEntry,
): number {
  if (left.updatedAt === right.updatedAt) {
    return left.label.localeCompare(right.label);
  }
  if (left.updatedAt === null) {
    return -1;
  }
  if (right.updatedAt === null) {
    return 1;
  }
  return left.updatedAt.localeCompare(right.updatedAt);
}

export interface RetentionPolicy {
  readonly maxAgeMs: number | null;
  readonly maxTotalBytes: number | null;
}

/**
 * Select which entries the retention policy removes.
 *
 * Orphans always go. Live-thread entries are eligible only when they exceed
 * the age limit or when total size is still over budget after older entries
 * were dropped — and the newest live entry per thread is always kept so an
 * active thread retains at least one restore point.
 *
 * Pure so the eviction ordering can be unit-tested without git or a database.
 */
export function selectRetentionEvictions(input: {
  readonly entries: ReadonlyArray<CheckpointStorageEntry>;
  readonly policy: RetentionPolicy;
  readonly nowMs: number;
}): ReadonlyArray<CheckpointStorageEntry> {
  const evictions = new Set<CheckpointStorageEntry>();
  for (const entry of input.entries) {
    if (entry.orphaned) {
      evictions.add(entry);
    }
  }

  const liveEntries = input.entries.filter((entry) => !entry.orphaned);
  const newestLiveEntryByThread = new Map<string, CheckpointStorageEntry>();
  for (const entry of liveEntries) {
    const key = `${entry.kind}:${entry.threadId ?? entry.location}`;
    const current = newestLiveEntryByThread.get(key);
    if (current === undefined || compareByAgeAscending(current, entry) < 0) {
      newestLiveEntryByThread.set(key, entry);
    }
  }
  const isProtected = (entry: CheckpointStorageEntry) =>
    newestLiveEntryByThread.get(`${entry.kind}:${entry.threadId ?? entry.location}`) === entry;

  if (input.policy.maxAgeMs !== null) {
    const cutoffMs = input.nowMs - input.policy.maxAgeMs;
    for (const entry of liveEntries) {
      if (entry.updatedAt === null || isProtected(entry)) {
        continue;
      }
      const updatedMs = Date.parse(entry.updatedAt);
      if (Number.isFinite(updatedMs) && updatedMs < cutoffMs) {
        evictions.add(entry);
      }
    }
  }

  if (input.policy.maxTotalBytes !== null) {
    let remaining = input.entries.reduce(
      (total, entry) => (evictions.has(entry) ? total : total + entry.bytes),
      0,
    );
    if (remaining > input.policy.maxTotalBytes) {
      const candidates = liveEntries
        .filter((entry) => !evictions.has(entry) && !isProtected(entry))
        .toSorted(compareByAgeAscending);
      for (const entry of candidates) {
        if (remaining <= input.policy.maxTotalBytes) {
          break;
        }
        evictions.add(entry);
        remaining -= entry.bytes;
      }
    }
  }

  return input.entries.filter((entry) => evictions.has(entry));
}

export class CheckpointMaintenance extends Context.Service<
  CheckpointMaintenance,
  {
    /** Current checkpoint storage footprint across projects. */
    readonly getUsage: () => Effect.Effect<CheckpointStorageUsage, CheckpointMaintenanceError>;

    readonly cleanup: (
      input: CheckpointMaintenanceCleanupInput,
    ) => Effect.Effect<CheckpointCleanupResult, CheckpointMaintenanceError>;

    /**
     * Drop all checkpoint state for one thread. Called on thread deletion so
     * hidden refs do not outlive the thread that produced them.
     */
    readonly forgetThread: (input: {
      readonly threadId: ThreadId;
      readonly cwd: string | undefined;
    }) => Effect.Effect<number, CheckpointMaintenanceError>;

    /** Apply the retention policy if `sweepOnStartup` is enabled. */
    readonly sweepIfConfigured: () => Effect.Effect<void, CheckpointMaintenanceError>;
  }
>()("t3/checkpointing/CheckpointMaintenance") {}

const isCheckpointMaintenanceError = Schema.is(CheckpointMaintenanceError);

const toMaintenanceError = (operation: string) => (cause: unknown) =>
  isCheckpointMaintenanceError(cause)
    ? cause
    : new CheckpointMaintenanceError({
        operation,
        detail: cause instanceof Error ? cause.message : "Checkpoint maintenance operation failed.",
        cause,
      });

export const make = Effect.gen(function* () {
  const checkpointStore = yield* CheckpointStore.CheckpointStore;
  const projectionSnapshotQuery = yield* ProjectionSnapshotQuery;
  const serverSettings = yield* ServerSettingsService;

  const nowIso = Effect.map(DateTime.now, DateTime.formatIso);

  const listLiveThreads = Effect.fn("CheckpointMaintenance.listLiveThreads")(function* () {
    const owners = yield* projectionSnapshotQuery.listThreadCheckpointOwners();
    return owners.map(
      (owner): LiveThread => ({
        threadId: owner.threadId,
        title: owner.title,
        cwd: owner.worktreePath ?? owner.workspaceRoot,
      }),
    );
  });

  /**
   * Collect checkpoint-ref entries, one per `{repository, thread}` pair.
   *
   * Repositories come from the read model: every known project workspace root
   * plus every thread worktree, including deleted threads. That is what makes
   * orphan cleanup possible after a thread is gone. Workspaces S5 Code has no
   * record of are never scanned; the alternative would be walking the user's
   * filesystem looking for repositories.
   */
  const collectCheckpointRefEntries = Effect.fn(
    "CheckpointMaintenance.collectCheckpointRefEntries",
  )(function* (liveThreads: ReadonlyArray<LiveThread>) {
    const threadById = new Map(liveThreads.map((thread) => [thread.threadId, thread]));
    const workspaces = [...new Set(yield* projectionSnapshotQuery.listCheckpointWorkspacePaths())];

    const entries: Array<CheckpointStorageEntry> = [];
    for (const cwd of workspaces) {
      const refs = yield* checkpointStore
        .listCheckpointRefs(cwd)
        .pipe(Effect.orElseSucceed(() => []));
      if (refs.length === 0) {
        continue;
      }

      const byThread = new Map<
        string,
        { readonly refs: Array<CheckpointRef>; updatedAt: string | null }
      >();
      for (const ref of refs) {
        const parsed = parseCheckpointRef(ref.checkpointRef);
        if (parsed === undefined) {
          // Unrecognized ref shape under our namespace: leave it alone rather
          // than attributing it to an arbitrary thread.
          continue;
        }
        const bucket = byThread.get(parsed.threadId) ?? { refs: [], updatedAt: null };
        bucket.refs.push(ref.checkpointRef);
        if (
          ref.updatedAt.length > 0 &&
          (bucket.updatedAt === null || ref.updatedAt > bucket.updatedAt)
        ) {
          bucket.updatedAt = ref.updatedAt;
        }
        byThread.set(parsed.threadId, bucket);
      }

      for (const [threadId, bucket] of byThread) {
        const thread = threadById.get(threadId as ThreadId);
        const bytes = yield* checkpointStore
          .measureCheckpointRefs({ cwd, checkpointRefs: bucket.refs })
          .pipe(Effect.orElseSucceed(() => 0));
        entries.push({
          kind: "checkpoint-refs",
          threadId: threadId as ThreadId,
          label: thread?.title ?? `Deleted thread (${threadId})`,
          location: cwd,
          refCount: bucket.refs.length,
          bytes,
          updatedAt: bucket.updatedAt,
          orphaned: thread === undefined,
        });
      }
    }
    return entries;
  });

  const buildUsage = Effect.fn("CheckpointMaintenance.buildUsage")(function* () {
    const liveThreads = yield* listLiveThreads();
    const [entries, generatedAt] = yield* Effect.all([
      collectCheckpointRefEntries(liveThreads),
      nowIso,
    ]);
    return {
      generatedAt,
      entries,
      totalBytes: entries.reduce((total, entry) => total + entry.bytes, 0),
      orphanedBytes: entries.reduce(
        (total, entry) => (entry.orphaned ? total + entry.bytes : total),
        0,
      ),
    } satisfies CheckpointStorageUsage;
  });

  const getUsage: CheckpointMaintenance["Service"]["getUsage"] = () =>
    buildUsage().pipe(Effect.mapError(toMaintenanceError("CheckpointMaintenance.getUsage")));

  const removeEntry = Effect.fn("CheckpointMaintenance.removeEntry")(function* (
    entry: CheckpointStorageEntry,
  ) {
    const refs = yield* checkpointStore
      .listCheckpointRefs(entry.location)
      .pipe(Effect.orElseSucceed(() => []));
    const targets = refs
      .filter((ref) => parseCheckpointRef(ref.checkpointRef)?.threadId === entry.threadId)
      .map((ref) => ref.checkpointRef);
    if (targets.length === 0) {
      return;
    }
    yield* checkpointStore
      .deleteCheckpointRefs({ cwd: entry.location, checkpointRefs: targets })
      .pipe(Effect.ignore);
  });

  const resolvePolicy = Effect.fn("CheckpointMaintenance.resolvePolicy")(function* () {
    const settings = yield* serverSettings.getSettings;
    const retention = settings.experimental.checkpointRetention;
    return {
      maxAgeMs: retention.maxAgeDays === null ? null : retention.maxAgeDays * 24 * 60 * 60 * 1000,
      maxTotalBytes:
        retention.maxTotalMegabytes === null
          ? null
          : retention.maxTotalMegabytes * BYTES_PER_MEGABYTE,
    } satisfies RetentionPolicy;
  });

  const runCleanup = Effect.fn("CheckpointMaintenance.runCleanup")(function* (
    input: CheckpointMaintenanceCleanupInput,
  ) {
    const dryRun = input.dryRun === true;
    const usage = yield* buildUsage();

    const targets =
      input.scope === "all"
        ? usage.entries
        : input.scope === "orphaned"
          ? usage.entries.filter((entry) => entry.orphaned)
          : selectRetentionEvictions({
              entries: usage.entries,
              policy: yield* resolvePolicy(),
              nowMs: yield* Clock.currentTimeMillis,
            });

    if (!dryRun) {
      yield* Effect.forEach(targets, removeEntry, { discard: true });
    }

    return {
      scope: input.scope,
      dryRun,
      removedEntries: targets,
      removedRefCount: targets.reduce((total, entry) => total + entry.refCount, 0),
      reclaimedBytes: targets.reduce((total, entry) => total + entry.bytes, 0),
      usage: dryRun ? usage : yield* buildUsage(),
    } satisfies CheckpointCleanupResult;
  });

  const cleanup: CheckpointMaintenance["Service"]["cleanup"] = (input) =>
    runCleanup(input).pipe(Effect.mapError(toMaintenanceError("CheckpointMaintenance.cleanup")));

  const forgetThread: CheckpointMaintenance["Service"]["forgetThread"] = Effect.fn(
    "CheckpointMaintenance.forgetThread",
  )(function* (input) {
    let reclaimedBytes = 0;

    if (input.cwd !== undefined) {
      const refs = yield* checkpointStore
        .listCheckpointRefs(input.cwd)
        .pipe(Effect.orElseSucceed(() => []));
      const targets = refs
        .filter((ref) => parseCheckpointRef(ref.checkpointRef)?.threadId === input.threadId)
        .map((ref) => ref.checkpointRef);
      if (targets.length > 0) {
        reclaimedBytes += yield* checkpointStore
          .measureCheckpointRefs({ cwd: input.cwd, checkpointRefs: targets })
          .pipe(Effect.orElseSucceed(() => 0));
        yield* checkpointStore
          .deleteCheckpointRefs({ cwd: input.cwd, checkpointRefs: targets })
          .pipe(Effect.ignore);
      }
    }

    return reclaimedBytes;
  });

  const sweepIfConfigured: CheckpointMaintenance["Service"]["sweepIfConfigured"] = () =>
    serverSettings.getSettings.pipe(
      Effect.flatMap((settings) =>
        settings.experimental.checkpointRetention.sweepOnStartup
          ? runCleanup({ scope: "retention-policy" }).pipe(
              Effect.tap((result) =>
                Effect.logDebug("checkpoint retention sweep complete", {
                  removedEntries: result.removedEntries.length,
                  reclaimedBytes: result.reclaimedBytes,
                }),
              ),
              Effect.asVoid,
            )
          : Effect.void,
      ),
      Effect.mapError(toMaintenanceError("CheckpointMaintenance.sweepIfConfigured")),
    );

  return CheckpointMaintenance.of({
    getUsage,
    cleanup,
    forgetThread,
    sweepIfConfigured,
  });
});

export const layer = Layer.effect(CheckpointMaintenance, make);
