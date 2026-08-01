// @effect-diagnostics nodeBuiltinImport:off
import * as NodePath from "node:path";

import * as NodeServices from "@effect/platform-node/NodeServices";
import { it } from "@effect/vitest";
import { MessageId, ThreadId, TurnId } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import { describe, expect } from "vite-plus/test";

import * as RewindService from "./RewindService.ts";
import * as RewindStore from "./RewindStore.ts";
import * as ServerConfig from "../config.ts";
import { RewindEntryRepositoryLive } from "../persistence/Layers/RewindEntries.ts";
import { SqlitePersistenceMemory } from "../persistence/Layers/Sqlite.ts";
import * as ServerSettings from "../serverSettings.ts";
import * as VcsProcess from "../vcs/VcsProcess.ts";

const makeTestLayer = (sessionRewindEnabled: boolean) =>
  RewindService.layer.pipe(
    Layer.provideMerge(RewindStore.layer),
    Layer.provideMerge(RewindEntryRepositoryLive),
    Layer.provideMerge(SqlitePersistenceMemory),
    Layer.provideMerge(ServerSettings.layerTest({ experimental: { sessionRewindEnabled } })),
    Layer.provideMerge(VcsProcess.layer),
    Layer.provideMerge(
      ServerConfig.layerTest(process.cwd(), { prefix: "t3-rewind-service-test-" }),
    ),
    Layer.provideMerge(NodeServices.layer),
  );

const threadId = ThreadId.make("thread-rewind-service");

const writeFile = (filePath: string, contents: string) =>
  Effect.flatMap(FileSystem.FileSystem, (fs) => fs.writeFileString(filePath, contents));

describe("deriveStatus", () => {
  it("marks the newest applied entry as the undo target and the oldest undone as redo", () => {
    const base = {
      threadId,
      sequence: 0,
      storeId: "store-1",
      cwd: "/workspace",
      userMessageId: null,
      assistantMessageId: null,
      prompt: "",
      beforeTree: "tree-before",
      afterTree: "tree-after",
      files: [] as ReadonlyArray<string>,
      createdAt: "2026-01-01T00:00:00.000Z",
      updatedAt: "2026-01-01T00:00:00.000Z",
    };
    const entries = [
      { ...base, turnId: TurnId.make("turn-1"), sequence: 0, state: "applied" as const },
      { ...base, turnId: TurnId.make("turn-2"), sequence: 1, state: "applied" as const },
      { ...base, turnId: TurnId.make("turn-3"), sequence: 2, state: "undone" as const },
      { ...base, turnId: TurnId.make("turn-4"), sequence: 3, state: "undone" as const },
    ];

    const status = RewindService.deriveStatus({ threadId, available: true, entries });

    expect(status.undo?.turnId).toBe("turn-2");
    expect(status.redo?.turnId).toBe("turn-3");
    expect(status.appliedCount).toBe(2);
    expect(status.undoneCount).toBe(2);
  });

  it("reports nothing to do for an empty history", () => {
    const status = RewindService.deriveStatus({ threadId, available: true, entries: [] });

    expect(status).toEqual({
      threadId,
      available: true,
      undo: null,
      redo: null,
      appliedCount: 0,
      undoneCount: 0,
    });
  });
});

it.layer(makeTestLayer(false))("RewindService (disabled)", (it) => {
  it.effect("reports unavailable and captures nothing", () =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const rewind = yield* RewindService.RewindService;
      const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "rewind-disabled-" });

      expect(yield* rewind.isEnabled).toBe(false);
      expect(yield* rewind.beginTurn({ threadId, cwd: workspace })).toBeNull();

      const captured = yield* rewind.captureTurn({
        threadId,
        turnId: TurnId.make("turn-1"),
        cwd: workspace,
        userMessageId: null,
        assistantMessageId: null,
        prompt: "do work",
      });
      expect(Option.isNone(captured)).toBe(true);

      const status = yield* rewind.getStatus(threadId);
      expect(status.available).toBe(false);
      expect((yield* rewind.undo(threadId)).outcome).toBe("unavailable");
    }),
  );
});

it.layer(makeTestLayer(true))("RewindService (enabled)", (it) => {
  it.effect("captures a turn, then undoes and redoes only its files", () =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const rewind = yield* RewindService.RewindService;
      const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "rewind-cycle-" });
      const turnId = TurnId.make("turn-1");

      yield* writeFile(NodePath.join(workspace, "edited.txt"), "before\n");
      yield* rewind.beginTurn({ threadId, cwd: workspace });
      yield* writeFile(NodePath.join(workspace, "edited.txt"), "after\n");

      const captured = yield* rewind.captureTurn({
        threadId,
        turnId,
        cwd: workspace,
        userMessageId: MessageId.make("message-user-1"),
        assistantMessageId: MessageId.make("message-assistant-1"),
        prompt: "edit the file",
      });
      expect(Option.isSome(captured)).toBe(true);

      const afterCapture = yield* rewind.getStatus(threadId);
      expect(afterCapture.available).toBe(true);
      expect(afterCapture.undo?.turnId).toBe(turnId);
      expect(afterCapture.redo).toBeNull();

      const undone = yield* rewind.undo(threadId);
      expect(undone.outcome).toBe("applied");
      expect(undone.prompt).toBe("edit the file");
      expect(yield* fs.readFileString(NodePath.join(workspace, "edited.txt"))).toBe("before\n");
      expect(undone.status.undo).toBeNull();
      expect(undone.status.redo?.turnId).toBe(turnId);

      const redone = yield* rewind.redo(threadId);
      expect(redone.outcome).toBe("applied");
      expect(yield* fs.readFileString(NodePath.join(workspace, "edited.txt"))).toBe("after\n");
      expect(redone.status.undo?.turnId).toBe(turnId);
      expect(redone.status.redo).toBeNull();
    }),
  );

  it.effect("skips capture when a turn changed no files", () =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const rewind = yield* RewindService.RewindService;
      const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "rewind-noop-" });
      const noopThreadId = ThreadId.make("thread-rewind-noop");

      yield* writeFile(NodePath.join(workspace, "file.txt"), "unchanged\n");
      yield* rewind.beginTurn({ threadId: noopThreadId, cwd: workspace });

      const captured = yield* rewind.captureTurn({
        threadId: noopThreadId,
        turnId: TurnId.make("turn-noop"),
        cwd: workspace,
        userMessageId: null,
        assistantMessageId: null,
        prompt: "look around",
      });

      // No undo affordance for a read-only turn: clicking it would do nothing.
      expect(Option.isNone(captured)).toBe(true);
      expect((yield* rewind.getStatus(noopThreadId)).undo).toBeNull();
    }),
  );

  it.effect("is idempotent when the same turn is captured twice", () =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const rewind = yield* RewindService.RewindService;
      const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "rewind-idempotent-" });
      const dupThreadId = ThreadId.make("thread-rewind-duplicate");
      const turnId = TurnId.make("turn-dup");

      yield* writeFile(NodePath.join(workspace, "file.txt"), "v1\n");
      yield* rewind.beginTurn({ threadId: dupThreadId, cwd: workspace });
      yield* writeFile(NodePath.join(workspace, "file.txt"), "v2\n");

      const input = {
        threadId: dupThreadId,
        turnId,
        cwd: workspace,
        userMessageId: null,
        assistantMessageId: null,
        prompt: "edit",
      };
      expect(Option.isSome(yield* rewind.captureTurn(input))).toBe(true);
      // The reactor can observe both a runtime and a domain completion for one
      // turn; the second capture must not overwrite the before/after pair.
      expect(Option.isNone(yield* rewind.captureTurn(input))).toBe(true);

      yield* rewind.undo(dupThreadId);
      expect(yield* fs.readFileString(NodePath.join(workspace, "file.txt"))).toBe("v1\n");
    }),
  );

  it.effect("drops the redo path once a new turn is captured", () =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const rewind = yield* RewindService.RewindService;
      const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "rewind-branch-" });
      const branchThreadId = ThreadId.make("thread-rewind-branch");

      yield* writeFile(NodePath.join(workspace, "file.txt"), "v1\n");
      yield* rewind.beginTurn({ threadId: branchThreadId, cwd: workspace });
      yield* writeFile(NodePath.join(workspace, "file.txt"), "v2\n");
      yield* rewind.captureTurn({
        threadId: branchThreadId,
        turnId: TurnId.make("turn-1"),
        cwd: workspace,
        userMessageId: null,
        assistantMessageId: null,
        prompt: "first",
      });

      yield* rewind.undo(branchThreadId);
      expect((yield* rewind.getStatus(branchThreadId)).redo?.turnId).toBe("turn-1");

      yield* rewind.beginTurn({ threadId: branchThreadId, cwd: workspace });
      yield* writeFile(NodePath.join(workspace, "file.txt"), "v3\n");
      yield* rewind.captureTurn({
        threadId: branchThreadId,
        turnId: TurnId.make("turn-2"),
        cwd: workspace,
        userMessageId: null,
        assistantMessageId: null,
        prompt: "second",
      });

      // Redoing turn-1 on top of turn-2 would clobber newer work, so the redo
      // entry is discarded rather than left clickable.
      const status = yield* rewind.getStatus(branchThreadId);
      expect(status.redo).toBeNull();
      expect(status.undo?.turnId).toBe("turn-2");
    }),
  );

  it.effect("reports nothing-to-do at the ends of the history", () =>
    Effect.gen(function* () {
      const rewind = yield* RewindService.RewindService;
      const emptyThreadId = ThreadId.make("thread-rewind-empty");

      expect((yield* rewind.undo(emptyThreadId)).outcome).toBe("nothing-to-do");
      expect((yield* rewind.redo(emptyThreadId)).outcome).toBe("nothing-to-do");
    }),
  );

  it.effect("forgets all state for a thread", () =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const rewind = yield* RewindService.RewindService;
      const store = yield* RewindStore.RewindStore;
      const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "rewind-forget-" });
      const forgetThreadId = ThreadId.make("thread-rewind-forget");

      yield* writeFile(NodePath.join(workspace, "file.txt"), "v1\n");
      yield* rewind.beginTurn({ threadId: forgetThreadId, cwd: workspace });
      yield* writeFile(NodePath.join(workspace, "file.txt"), "v2\n");
      yield* rewind.captureTurn({
        threadId: forgetThreadId,
        turnId: TurnId.make("turn-1"),
        cwd: workspace,
        userMessageId: null,
        assistantMessageId: null,
        prompt: "edit",
      });

      const storeId = yield* store.storeIdForThread(forgetThreadId);
      const storePath = yield* store.storePath(storeId);
      expect(yield* fs.exists(storePath)).toBe(true);

      expect(yield* rewind.forgetThread(forgetThreadId)).toBeGreaterThan(0);
      expect(yield* fs.exists(storePath)).toBe(false);
      expect((yield* rewind.getStatus(forgetThreadId)).undo).toBeNull();
    }),
  );
});
