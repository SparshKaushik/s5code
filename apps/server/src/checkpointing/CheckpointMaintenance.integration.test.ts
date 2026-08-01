// @effect-diagnostics nodeBuiltinImport:off
/**
 * End-to-end checkpoint maintenance against real git repositories, the real
 * projection query, and real rewind shadow stores.
 *
 * The pure eviction rules are covered in `CheckpointMaintenance.test.ts`; this
 * file exists for the properties that only hold against real git: which refs
 * are visible, what deletion actually removes, and that user refs survive.
 */
import * as NodePath from "node:path";

import * as NodeServices from "@effect/platform-node/NodeServices";
import { it } from "@effect/vitest";
import { ThreadId, TurnId } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as SqlClient from "effect/unstable/sql/SqlClient";
import { describe, expect } from "vite-plus/test";

import { CheckpointMaintenance } from "./CheckpointMaintenance.ts";
import * as CheckpointMaintenanceLayer from "./CheckpointMaintenance.ts";
import * as CheckpointStore from "./CheckpointStore.ts";
import { checkpointRefForThreadTurn } from "./Utils.ts";
import { OrchestrationProjectionSnapshotQueryLive } from "../orchestration/Layers/ProjectionSnapshotQuery.ts";
import { RewindEntryRepositoryLive } from "../persistence/Layers/RewindEntries.ts";
import { SqlitePersistenceMemory } from "../persistence/Layers/Sqlite.ts";
import * as RepositoryIdentityResolver from "../project/RepositoryIdentityResolver.ts";
import * as RewindService from "../rewind/RewindService.ts";
import * as RewindStore from "../rewind/RewindStore.ts";
import * as ServerConfig from "../config.ts";
import * as ServerSettings from "../serverSettings.ts";
import * as VcsDriverRegistry from "../vcs/VcsDriverRegistry.ts";
import * as VcsProcess from "../vcs/VcsProcess.ts";

const VcsProcessTestLayer = VcsProcess.layer.pipe(Layer.provide(NodeServices.layer));
const VcsDriverTestLayer = VcsDriverRegistry.layer.pipe(Layer.provide(VcsProcessTestLayer));

const makeTestLayer = (
  overrides: {
    readonly sessionRewindEnabled?: boolean;
    readonly deleteOnThreadDelete?: boolean;
    readonly maxAgeDays?: number | null;
    readonly maxTotalMegabytes?: number | null;
    readonly sweepOnStartup?: boolean;
  } = {},
) =>
  CheckpointMaintenanceLayer.layer.pipe(
    Layer.provideMerge(RewindService.layer),
    Layer.provideMerge(RewindStore.layer),
    Layer.provideMerge(RewindEntryRepositoryLive),
    Layer.provideMerge(CheckpointStore.layer.pipe(Layer.provide(VcsDriverTestLayer))),
    Layer.provideMerge(
      OrchestrationProjectionSnapshotQueryLive.pipe(
        Layer.provide(RepositoryIdentityResolver.layer),
      ),
    ),
    Layer.provideMerge(SqlitePersistenceMemory),
    Layer.provideMerge(
      ServerSettings.layerTest({
        experimental: {
          sessionRewindEnabled: overrides.sessionRewindEnabled ?? true,
          checkpointRetention: {
            deleteOnThreadDelete: overrides.deleteOnThreadDelete ?? true,
            maxAgeDays: overrides.maxAgeDays === undefined ? null : overrides.maxAgeDays,
            maxTotalMegabytes:
              overrides.maxTotalMegabytes === undefined ? null : overrides.maxTotalMegabytes,
            sweepOnStartup: overrides.sweepOnStartup ?? false,
          },
        },
      }),
    ),
    Layer.provideMerge(VcsProcessTestLayer),
    Layer.provideMerge(VcsDriverTestLayer),
    Layer.provideMerge(
      ServerConfig.layerTest(process.cwd(), { prefix: "t3-checkpoint-maintenance-test-" }),
    ),
    Layer.provideMerge(NodeServices.layer),
  );

const git = (cwd: string, args: ReadonlyArray<string>) =>
  Effect.flatMap(VcsProcess.VcsProcess, (process) =>
    process
      .run({
        operation: "CheckpointMaintenance.integration.git",
        command: "git",
        cwd,
        args,
        timeoutMs: 10_000,
        allowNonZeroExit: true,
      })
      .pipe(Effect.map((result) => result)),
  );

const initRepository = (cwd: string) =>
  Effect.gen(function* () {
    const fs = yield* FileSystem.FileSystem;
    yield* git(cwd, ["init"]);
    yield* git(cwd, ["config", "user.email", "test@test.com"]);
    yield* git(cwd, ["config", "user.name", "Test"]);
    yield* fs.writeFileString(NodePath.join(cwd, "README.md"), "# test\n");
    yield* git(cwd, ["add", "."]);
    yield* git(cwd, ["commit", "-m", "initial commit"]);
  });

/** Seed a project row and one thread row directly into the read model. */
const seedThread = (input: {
  readonly projectId: string;
  readonly threadId: ThreadId;
  readonly title: string;
  readonly workspaceRoot: string;
  readonly archived: boolean;
  readonly deleted: boolean;
}) =>
  Effect.gen(function* () {
    const sql = yield* SqlClient.SqlClient;
    yield* sql`
      INSERT OR IGNORE INTO projection_projects (
        project_id, title, workspace_root, default_model_selection_json, scripts_json,
        created_at, updated_at, deleted_at
      ) VALUES (
        ${input.projectId}, ${input.title}, ${input.workspaceRoot},
        '{"instanceId":"codex","model":"gpt-5-codex"}', '[]',
        '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z', NULL
      )
    `;
    yield* sql`
      INSERT INTO projection_threads (
        thread_id, project_id, title, model_selection_json, runtime_mode, interaction_mode,
        branch, worktree_path, latest_turn_id, latest_user_message_at,
        pending_approval_count, pending_user_input_count, has_actionable_proposed_plan,
        created_at, updated_at, archived_at, deleted_at
      ) VALUES (
        ${input.threadId}, ${input.projectId}, ${input.title},
        '{"instanceId":"codex","model":"gpt-5-codex"}', 'full-access', 'default',
        NULL, NULL, NULL, NULL, 0, 0, 0,
        '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z',
        ${input.archived ? "2026-01-02T00:00:00.000Z" : null},
        ${input.deleted ? "2026-01-03T00:00:00.000Z" : null}
      )
    `;
  });

describe("CheckpointMaintenance (integration)", () => {
  it.layer(makeTestLayer())("usage reporting", (it) => {
    it.effect("reports checkpoint refs per thread and flags deleted threads as orphaned", () =>
      Effect.gen(function* () {
        const fs = yield* FileSystem.FileSystem;
        const maintenance = yield* CheckpointMaintenance;
        const checkpointStore = yield* CheckpointStore.CheckpointStore;
        const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "maintenance-usage-" });
        yield* initRepository(workspace);

        const liveThreadId = ThreadId.make("thread-live");
        const archivedThreadId = ThreadId.make("thread-archived");
        const deletedThreadId = ThreadId.make("thread-deleted");

        yield* seedThread({
          projectId: "project-usage",
          threadId: liveThreadId,
          title: "Live thread",
          workspaceRoot: workspace,
          archived: false,
          deleted: false,
        });
        yield* seedThread({
          projectId: "project-usage",
          threadId: archivedThreadId,
          title: "Archived thread",
          workspaceRoot: workspace,
          archived: true,
          deleted: false,
        });
        yield* seedThread({
          projectId: "project-usage",
          threadId: deletedThreadId,
          title: "Deleted thread",
          workspaceRoot: workspace,
          archived: false,
          deleted: true,
        });

        for (const threadId of [liveThreadId, archivedThreadId, deletedThreadId]) {
          yield* fs.writeFileString(NodePath.join(workspace, `${threadId}.txt`), "content\n");
          yield* checkpointStore.captureCheckpoint({
            cwd: workspace,
            checkpointRef: checkpointRefForThreadTurn(threadId, 0),
          });
          yield* checkpointStore.captureCheckpoint({
            cwd: workspace,
            checkpointRef: checkpointRefForThreadTurn(threadId, 1),
          });
        }
        // A user branch that must never appear in usage.
        yield* git(workspace, ["branch", "feature/keep"]);

        const usage = yield* maintenance.getUsage();
        const byThread = new Map(usage.entries.map((entry) => [entry.threadId, entry]));

        expect(byThread.get(liveThreadId)?.orphaned).toBe(false);
        expect(byThread.get(liveThreadId)?.refCount).toBe(2);
        expect(byThread.get(liveThreadId)?.label).toBe("Live thread");
        // Archiving is reversible, so an archived thread is not an orphan.
        expect(byThread.get(archivedThreadId)?.orphaned).toBe(false);
        expect(byThread.get(deletedThreadId)?.orphaned).toBe(true);
        expect(usage.totalBytes).toBeGreaterThan(0);
        expect(usage.orphanedBytes).toBeGreaterThan(0);
        expect(usage.orphanedBytes).toBeLessThanOrEqual(usage.totalBytes);
        for (const entry of usage.entries) {
          expect(entry.location).toBe(workspace);
        }
      }),
    );
  });

  it.layer(makeTestLayer())("cleanup scopes", (it) => {
    it.effect("dry run reports targets without deleting anything", () =>
      Effect.gen(function* () {
        const fs = yield* FileSystem.FileSystem;
        const maintenance = yield* CheckpointMaintenance;
        const checkpointStore = yield* CheckpointStore.CheckpointStore;
        const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "maintenance-dry-run-" });
        yield* initRepository(workspace);

        const threadId = ThreadId.make("thread-dry-run");
        yield* seedThread({
          projectId: "project-dry-run",
          threadId,
          title: "Gone",
          workspaceRoot: workspace,
          archived: false,
          deleted: true,
        });
        yield* checkpointStore.captureCheckpoint({
          cwd: workspace,
          checkpointRef: checkpointRefForThreadTurn(threadId, 0),
        });

        const result = yield* maintenance.cleanup({ scope: "orphaned", dryRun: true });
        expect(result.dryRun).toBe(true);
        expect(result.removedEntries.length).toBe(1);
        expect(result.removedRefCount).toBe(1);
        // Still present: a dry run must not touch the repository.
        expect((yield* checkpointStore.listCheckpointRefs(workspace)).length).toBe(1);
      }),
    );

    it.effect("orphaned scope removes only deleted-thread refs and leaves user refs alone", () =>
      Effect.gen(function* () {
        const fs = yield* FileSystem.FileSystem;
        const maintenance = yield* CheckpointMaintenance;
        const checkpointStore = yield* CheckpointStore.CheckpointStore;
        const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "maintenance-orphans-" });
        yield* initRepository(workspace);

        const liveThreadId = ThreadId.make("thread-orphans-live");
        const deletedThreadId = ThreadId.make("thread-orphans-deleted");
        yield* seedThread({
          projectId: "project-orphans",
          threadId: liveThreadId,
          title: "Live",
          workspaceRoot: workspace,
          archived: false,
          deleted: false,
        });
        yield* seedThread({
          projectId: "project-orphans",
          threadId: deletedThreadId,
          title: "Deleted",
          workspaceRoot: workspace,
          archived: false,
          deleted: true,
        });
        yield* checkpointStore.captureCheckpoint({
          cwd: workspace,
          checkpointRef: checkpointRefForThreadTurn(liveThreadId, 0),
        });
        yield* checkpointStore.captureCheckpoint({
          cwd: workspace,
          checkpointRef: checkpointRefForThreadTurn(deletedThreadId, 0),
        });
        yield* git(workspace, ["branch", "feature/keep"]);
        yield* git(workspace, ["tag", "v1.0.0"]);

        const result = yield* maintenance.cleanup({ scope: "orphaned" });
        expect(result.removedEntries.length).toBe(1);
        expect(result.removedEntries[0]?.threadId).toBe(deletedThreadId);

        const remaining = (yield* checkpointStore.listCheckpointRefs(workspace)).map(
          (ref) => ref.checkpointRef,
        );
        expect(remaining).toEqual([checkpointRefForThreadTurn(liveThreadId, 0)]);
        // User refs are untouched.
        expect((yield* git(workspace, ["rev-parse", "--verify", "feature/keep"])).exitCode).toBe(0);
        expect((yield* git(workspace, ["rev-parse", "--verify", "v1.0.0"])).exitCode).toBe(0);
      }),
    );

    it.effect("all scope removes live-thread refs too", () =>
      Effect.gen(function* () {
        const fs = yield* FileSystem.FileSystem;
        const maintenance = yield* CheckpointMaintenance;
        const checkpointStore = yield* CheckpointStore.CheckpointStore;
        const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "maintenance-all-" });
        yield* initRepository(workspace);

        const threadId = ThreadId.make("thread-all");
        yield* seedThread({
          projectId: "project-all",
          threadId,
          title: "Live",
          workspaceRoot: workspace,
          archived: false,
          deleted: false,
        });
        yield* checkpointStore.captureCheckpoint({
          cwd: workspace,
          checkpointRef: checkpointRefForThreadTurn(threadId, 0),
        });
        yield* git(workspace, ["branch", "feature/keep"]);

        const result = yield* maintenance.cleanup({ scope: "all" });
        expect(result.removedRefCount).toBe(1);
        expect(yield* checkpointStore.listCheckpointRefs(workspace)).toEqual([]);
        expect((yield* git(workspace, ["rev-parse", "--verify", "feature/keep"])).exitCode).toBe(0);
      }),
    );
  });

  it.layer(makeTestLayer({ maxAgeDays: 1 }))("retention policy", (it) => {
    it.effect("keeps the newest ref for a live thread and drops older ones", () =>
      Effect.gen(function* () {
        const fs = yield* FileSystem.FileSystem;
        const maintenance = yield* CheckpointMaintenance;
        const checkpointStore = yield* CheckpointStore.CheckpointStore;
        const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "maintenance-retention-" });
        yield* initRepository(workspace);

        const liveThreadId = ThreadId.make("thread-retention-live");
        const deletedThreadId = ThreadId.make("thread-retention-deleted");
        yield* seedThread({
          projectId: "project-retention",
          threadId: liveThreadId,
          title: "Live",
          workspaceRoot: workspace,
          archived: false,
          deleted: false,
        });
        yield* seedThread({
          projectId: "project-retention",
          threadId: deletedThreadId,
          title: "Deleted",
          workspaceRoot: workspace,
          archived: false,
          deleted: true,
        });
        yield* checkpointStore.captureCheckpoint({
          cwd: workspace,
          checkpointRef: checkpointRefForThreadTurn(liveThreadId, 0),
        });
        yield* checkpointStore.captureCheckpoint({
          cwd: workspace,
          checkpointRef: checkpointRefForThreadTurn(deletedThreadId, 0),
        });

        // Checkpoints were just written, so the age limit cannot evict the live
        // thread's entry; only the orphan goes.
        const result = yield* maintenance.cleanup({ scope: "retention-policy" });
        expect(result.removedEntries.map((entry) => entry.threadId)).toEqual([deletedThreadId]);
        expect((yield* checkpointStore.listCheckpointRefs(workspace)).length).toBe(1);
      }),
    );
  });

  it.layer(makeTestLayer())("thread deletion", (it) => {
    it.effect("forgetThread removes that thread's refs and rewind store only", () =>
      Effect.gen(function* () {
        const fs = yield* FileSystem.FileSystem;
        const maintenance = yield* CheckpointMaintenance;
        const checkpointStore = yield* CheckpointStore.CheckpointStore;
        const rewind = yield* RewindService.RewindService;
        const rewindStore = yield* RewindStore.RewindStore;
        const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "maintenance-forget-" });
        yield* initRepository(workspace);

        const targetThreadId = ThreadId.make("thread-forget-target");
        const otherThreadId = ThreadId.make("thread-forget-other");
        for (const [threadId, title] of [
          [targetThreadId, "Target"],
          [otherThreadId, "Other"],
        ] as const) {
          yield* seedThread({
            projectId: "project-forget",
            threadId,
            title,
            workspaceRoot: workspace,
            archived: false,
            deleted: false,
          });
          yield* checkpointStore.captureCheckpoint({
            cwd: workspace,
            checkpointRef: checkpointRefForThreadTurn(threadId, 0),
          });
        }

        // Give the target thread a rewind store with one captured turn.
        yield* rewind.beginTurn({ threadId: targetThreadId, cwd: workspace });
        yield* fs.writeFileString(NodePath.join(workspace, "rewound.txt"), "after\n");
        yield* rewind.captureTurn({
          threadId: targetThreadId,
          turnId: TurnId.make("turn-forget"),
          cwd: workspace,
          userMessageId: null,
          assistantMessageId: null,
          prompt: "edit",
        });
        const storePath = yield* rewindStore.storePath(
          yield* rewindStore.storeIdForThread(targetThreadId),
        );
        expect(yield* fs.exists(storePath)).toBe(true);

        yield* maintenance.forgetThread({ threadId: targetThreadId, cwd: workspace });

        expect(
          (yield* checkpointStore.listCheckpointRefs(workspace)).map((r) => r.checkpointRef),
        ).toEqual([checkpointRefForThreadTurn(otherThreadId, 0)]);
        expect(yield* fs.exists(storePath)).toBe(false);
        expect((yield* rewind.getStatus(targetThreadId)).undo).toBeNull();
      }),
    );

    it.effect("tolerates a thread with no workspace and no state", () =>
      Effect.gen(function* () {
        const maintenance = yield* CheckpointMaintenance;

        expect(
          yield* maintenance.forgetThread({
            threadId: ThreadId.make("thread-forget-unknown"),
            cwd: undefined,
          }),
        ).toBe(0);
      }),
    );
  });

  it.layer(makeTestLayer({ sweepOnStartup: false }))("startup sweep disabled", (it) => {
    it.effect("does nothing when sweepOnStartup is off", () =>
      Effect.gen(function* () {
        const fs = yield* FileSystem.FileSystem;
        const maintenance = yield* CheckpointMaintenance;
        const checkpointStore = yield* CheckpointStore.CheckpointStore;
        const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "maintenance-no-sweep-" });
        yield* initRepository(workspace);

        const threadId = ThreadId.make("thread-no-sweep");
        yield* seedThread({
          projectId: "project-no-sweep",
          threadId,
          title: "Deleted",
          workspaceRoot: workspace,
          archived: false,
          deleted: true,
        });
        yield* checkpointStore.captureCheckpoint({
          cwd: workspace,
          checkpointRef: checkpointRefForThreadTurn(threadId, 0),
        });

        yield* maintenance.sweepIfConfigured();
        expect((yield* checkpointStore.listCheckpointRefs(workspace)).length).toBe(1);
      }),
    );
  });

  it.layer(makeTestLayer({ sweepOnStartup: true }))("startup sweep enabled", (it) => {
    it.effect("applies the retention policy on demand", () =>
      Effect.gen(function* () {
        const fs = yield* FileSystem.FileSystem;
        const maintenance = yield* CheckpointMaintenance;
        const checkpointStore = yield* CheckpointStore.CheckpointStore;
        const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "maintenance-sweep-" });
        yield* initRepository(workspace);

        const threadId = ThreadId.make("thread-sweep");
        yield* seedThread({
          projectId: "project-sweep",
          threadId,
          title: "Deleted",
          workspaceRoot: workspace,
          archived: false,
          deleted: true,
        });
        yield* checkpointStore.captureCheckpoint({
          cwd: workspace,
          checkpointRef: checkpointRefForThreadTurn(threadId, 0),
        });

        yield* maintenance.sweepIfConfigured();
        expect(yield* checkpointStore.listCheckpointRefs(workspace)).toEqual([]);
      }),
    );
  });
});
