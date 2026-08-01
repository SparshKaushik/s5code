// @effect-diagnostics nodeBuiltinImport:off
import * as NodePath from "node:path";

import * as NodeServices from "@effect/platform-node/NodeServices";
import { it } from "@effect/vitest";
import { ThreadId } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import { describe, expect } from "vite-plus/test";

import * as RewindStore from "./RewindStore.ts";
import * as ServerConfig from "../config.ts";
import * as VcsProcess from "../vcs/VcsProcess.ts";

const VcsProcessTestLayer = VcsProcess.layer.pipe(Layer.provide(NodeServices.layer));
const ServerConfigLayer = ServerConfig.ServerConfig.layerTest(process.cwd(), {
  prefix: "t3-rewind-store-test-",
});
const TestLayer = RewindStore.layer.pipe(
  Layer.provideMerge(VcsProcessTestLayer),
  Layer.provideMerge(ServerConfigLayer),
  Layer.provideMerge(NodeServices.layer),
);

describe("normalizeRewindPath", () => {
  it("accepts relative paths and normalizes separators", () => {
    expect(RewindStore.normalizeRewindPath("src/app.ts")).toBe("src/app.ts");
    expect(RewindStore.normalizeRewindPath("src\\app.ts")).toBe("src/app.ts");
  });

  it("rejects paths that could escape the workspace", () => {
    expect(RewindStore.normalizeRewindPath("/etc/passwd")).toBeUndefined();
    expect(RewindStore.normalizeRewindPath("C:/Windows/system32")).toBeUndefined();
    expect(RewindStore.normalizeRewindPath("..")).toBeUndefined();
    expect(RewindStore.normalizeRewindPath("../outside.ts")).toBeUndefined();
    expect(RewindStore.normalizeRewindPath("src/../../outside.ts")).toBeUndefined();
    expect(RewindStore.normalizeRewindPath("src/..")).toBeUndefined();
    expect(RewindStore.normalizeRewindPath(".")).toBeUndefined();
    expect(RewindStore.normalizeRewindPath("")).toBeUndefined();
    expect(RewindStore.normalizeRewindPath("src/\0app.ts")).toBeUndefined();
  });

  it("rejects the server state directory", () => {
    // `.t3` is excluded from snapshots, so restoring it would write state the
    // store never captured.
    expect(RewindStore.normalizeRewindPath(".t3")).toBeUndefined();
    expect(RewindStore.normalizeRewindPath(".t3/state.sqlite")).toBeUndefined();
    expect(RewindStore.normalizeRewindPath(".t3rc")).toBe(".t3rc");
  });
});

describe("parseNulSeparatedPaths", () => {
  it("deduplicates and drops unsafe entries", () => {
    expect(
      RewindStore.parseNulSeparatedPaths("a.ts\0b.ts\0a.ts\0../escape.ts\0.t3/state.sqlite\0\0"),
    ).toEqual(["a.ts", "b.ts"]);
  });
});

it.layer(TestLayer)("RewindStore.layer", (it) => {
  const writeFile = (filePath: string, contents: string) =>
    Effect.flatMap(FileSystem.FileSystem, (fs) => fs.writeFileString(filePath, contents));

  it.effect("snapshots and restores only the files a turn touched", () =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const store = yield* RewindStore.RewindStore;
      const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "rewind-workspace-" });

      yield* writeFile(NodePath.join(workspace, "kept.txt"), "original kept\n");
      yield* writeFile(NodePath.join(workspace, "edited.txt"), "before\n");

      const handle = yield* store.open({
        threadId: ThreadId.make("thread-rewind-store"),
        cwd: workspace,
      });
      const before = yield* store.snapshot(handle);

      // Simulate a turn: edit one file, create another.
      yield* writeFile(NodePath.join(workspace, "edited.txt"), "after\n");
      yield* writeFile(NodePath.join(workspace, "created.txt"), "new\n");
      const after = yield* store.snapshot(handle);

      const changed = yield* store.changedFiles({ handle, fromTree: before, toTree: after });
      expect([...changed].toSorted()).toEqual(["created.txt", "edited.txt"]);

      // Unrelated user edit made after the turn; undo must leave it alone.
      yield* writeFile(NodePath.join(workspace, "kept.txt"), "user edit\n");

      const restored = yield* store.restoreFiles({ handle, tree: before, files: changed });
      expect([...restored].toSorted()).toEqual(["created.txt", "edited.txt"]);
      expect(yield* fs.readFileString(NodePath.join(workspace, "edited.txt"))).toBe("before\n");
      expect(yield* fs.exists(NodePath.join(workspace, "created.txt"))).toBe(false);
      expect(yield* fs.readFileString(NodePath.join(workspace, "kept.txt"))).toBe("user edit\n");

      // Redo returns the turn's changes without touching the user's edit.
      yield* store.restoreFiles({ handle, tree: after, files: changed });
      expect(yield* fs.readFileString(NodePath.join(workspace, "edited.txt"))).toBe("after\n");
      expect(yield* fs.readFileString(NodePath.join(workspace, "created.txt"))).toBe("new\n");
      expect(yield* fs.readFileString(NodePath.join(workspace, "kept.txt"))).toBe("user edit\n");
    }),
  );

  it.effect("never writes into the workspace repository", () =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const store = yield* RewindStore.RewindStore;
      const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "rewind-clean-" });

      yield* writeFile(NodePath.join(workspace, "file.txt"), "content\n");
      const handle = yield* store.open({
        threadId: ThreadId.make("thread-rewind-isolation"),
        cwd: workspace,
      });
      yield* store.snapshot(handle);

      // The store's git dir lives in server state, and the workspace gains no
      // `.git` of its own.
      expect(handle.gitDir.startsWith(workspace)).toBe(false);
      expect(yield* fs.exists(NodePath.join(workspace, ".git"))).toBe(false);
    }),
  );

  it.effect("excludes the server state directory from snapshots", () =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const store = yield* RewindStore.RewindStore;
      const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "rewind-state-" });

      yield* writeFile(NodePath.join(workspace, "file.txt"), "v1\n");
      const handle = yield* store.open({
        threadId: ThreadId.make("thread-rewind-state"),
        cwd: workspace,
      });
      const before = yield* store.snapshot(handle);

      yield* fs.makeDirectory(NodePath.join(workspace, ".t3"), { recursive: true });
      yield* writeFile(NodePath.join(workspace, ".t3", "state.sqlite"), "db\n");
      const after = yield* store.snapshot(handle);

      expect(yield* store.changedFiles({ handle, fromTree: before, toTree: after })).toEqual([]);
    }),
  );

  it.effect("reports and deletes store usage", () =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const store = yield* RewindStore.RewindStore;
      const workspace = yield* fs.makeTempDirectoryScoped({ prefix: "rewind-usage-" });
      const threadId = ThreadId.make("thread-rewind-usage");

      yield* writeFile(NodePath.join(workspace, "file.txt"), "content\n");
      const handle = yield* store.open({ threadId, cwd: workspace });
      yield* store.snapshot(handle);

      const usage = yield* store.listUsage();
      const record = usage.find((candidate) => candidate.storeId === handle.storeId);
      expect(record).toBeDefined();
      expect(record?.bytes).toBeGreaterThan(0);

      expect(yield* store.deleteStore(handle.storeId)).toBeGreaterThan(0);
      expect(yield* fs.exists(handle.gitDir)).toBe(false);
      // Deleting a store that is already gone reports zero rather than failing.
      expect(yield* store.deleteStore(handle.storeId)).toBe(0);
    }),
  );

  it.effect("derives a stable store id per thread", () =>
    Effect.gen(function* () {
      const store = yield* RewindStore.RewindStore;
      const threadId = ThreadId.make("thread-rewind-id");

      const first = yield* store.storeIdForThread(threadId);
      const second = yield* store.storeIdForThread(threadId);
      const other = yield* store.storeIdForThread(ThreadId.make("thread-rewind-other"));

      expect(first).toBe(second);
      expect(first).not.toBe(other);
    }),
  );
});
