/**
 * RewindStore — shadow-git snapshot storage for session rewind.
 *
 * Each thread gets its own git directory under
 * `<stateDir>/rewind/<storeId>`, with the project workspace attached as the
 * work tree. Snapshots are bare trees written with `git write-tree`; nothing
 * is ever written into the user's repository, so a rewind snapshot cannot
 * show up in `git log`, `git fsck`, or a push.
 *
 * Two invariants matter and are enforced here rather than by convention:
 *
 *   1. `.t3` is excluded from every `add`, and `normalizeRewindPath` rejects
 *      it on restore. Server state must never be snapshotted or rewritten.
 *   2. Restore is scoped to an explicit file list. The store never runs
 *      `git checkout`/`clean` across the whole worktree, so files the turn
 *      did not touch — including the user's staged work — are left alone.
 *
 * @module rewind/RewindStore
 */
import * as Context from "effect/Context";
import * as Crypto from "effect/Crypto";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";

import { RewindError, type ThreadId } from "@t3tools/contracts";

import * as ServerConfig from "../config.ts";
import * as VcsProcess from "../vcs/VcsProcess.ts";

const GIT_TIMEOUT_MS = 30_000;
const META_FILE = "store.json";

/**
 * Human-readable breadcrumb written next to each store so an operator can tell
 * which thread and workspace a directory under `<stateDir>/rewind` belongs to.
 * Never read back by the service; the database is the source of truth.
 */
const StoreMetadata = Schema.Struct({
  storeId: Schema.String,
  cwd: Schema.String,
});
const encodeStoreMetadata = Schema.encodeUnknownEffect(Schema.fromJsonString(StoreMetadata));

/** Excluded from every snapshot: server state must never round-trip. */
const EXCLUDED_TOP_LEVEL_DIRECTORY = ".t3";

export interface RewindStoreHandle {
  readonly storeId: string;
  readonly gitDir: string;
  readonly cwd: string;
}

export interface RewindStoreUsage {
  readonly storeId: string;
  readonly path: string;
  readonly bytes: number;
}

/**
 * Normalize a snapshot-relative path for restore.
 *
 * Returns `undefined` for anything that must not be written: absolute paths,
 * NUL bytes, `.` , parent traversal, and the `.t3` state directory (which is
 * excluded from snapshots, so restoring it would be writing state the store
 * never captured).
 */
export function normalizeRewindPath(file: string): string | undefined {
  if (file.length === 0 || file.includes("\0")) {
    return undefined;
  }
  const normalized = file.replaceAll("\\", "/");
  if (normalized.startsWith("/") || /^[A-Za-z]:\//.test(normalized)) {
    return undefined;
  }
  if (
    normalized === "." ||
    normalized === ".." ||
    normalized.startsWith("../") ||
    normalized.includes("/../") ||
    normalized.endsWith("/..")
  ) {
    return undefined;
  }
  if (
    normalized === EXCLUDED_TOP_LEVEL_DIRECTORY ||
    normalized.startsWith(`${EXCLUDED_TOP_LEVEL_DIRECTORY}/`)
  ) {
    return undefined;
  }
  return normalized;
}

/** Parse `git diff --name-only -z` output into unique, safe relative paths. */
export function parseNulSeparatedPaths(stdout: string): ReadonlyArray<string> {
  const seen = new Set<string>();
  for (const raw of stdout.split("\0")) {
    const normalized = normalizeRewindPath(raw.trim());
    if (normalized !== undefined) {
      seen.add(normalized);
    }
  }
  return [...seen];
}

export class RewindStore extends Context.Service<
  RewindStore,
  {
    /** Stable store id for a thread. Same input always yields the same id. */
    readonly storeIdForThread: (threadId: ThreadId) => Effect.Effect<string, RewindError>;

    /** Absolute path of a store's git directory. */
    readonly storePath: (storeId: string) => Effect.Effect<string, RewindError>;

    /** Create (or reuse) the store for a thread and bind it to `cwd`. */
    readonly open: (input: {
      readonly threadId: ThreadId;
      readonly cwd: string;
    }) => Effect.Effect<RewindStoreHandle, RewindError>;

    /** Snapshot the current work tree, returning the tree oid. */
    readonly snapshot: (handle: RewindStoreHandle) => Effect.Effect<string, RewindError>;

    /** Files that differ between two snapshot trees. */
    readonly changedFiles: (input: {
      readonly handle: RewindStoreHandle;
      readonly fromTree: string;
      readonly toTree: string;
    }) => Effect.Effect<ReadonlyArray<string>, RewindError>;

    /**
     * Restore exactly `files` from `tree`. Paths present in the tree are
     * written; paths absent from it are removed. Everything else in the
     * worktree, including the user's index, is untouched.
     */
    readonly restoreFiles: (input: {
      readonly handle: RewindStoreHandle;
      readonly tree: string;
      readonly files: ReadonlyArray<string>;
    }) => Effect.Effect<ReadonlyArray<string>, RewindError>;

    /** On-disk size of every existing store. */
    readonly listUsage: () => Effect.Effect<ReadonlyArray<RewindStoreUsage>, RewindError>;

    /** Delete a store directory. Missing stores are ignored. */
    readonly deleteStore: (storeId: string) => Effect.Effect<number, RewindError>;
  }
>()("t3/rewind/RewindStore") {}

const rewindError = (input: {
  readonly operation: string;
  readonly detail: string;
  readonly cause?: unknown;
}) =>
  new RewindError({
    operation: input.operation,
    detail: input.detail,
    ...(input.cause === undefined ? {} : { cause: input.cause }),
  });

export const make = Effect.gen(function* () {
  const config = yield* ServerConfig.ServerConfig;
  const fileSystem = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const process = yield* VcsProcess.VcsProcess;
  const crypto = yield* Crypto.Crypto;

  const initializedStores = new Set<string>();

  const storeIdForThread: RewindStore["Service"]["storeIdForThread"] = Effect.fn(
    "RewindStore.storeIdForThread",
  )(function* (threadId) {
    const digest = yield* crypto.digest("SHA-256", new TextEncoder().encode(threadId)).pipe(
      Effect.mapError((cause) =>
        rewindError({
          operation: "RewindStore.storeIdForThread",
          detail: "Failed to hash thread id for rewind store.",
          cause,
        }),
      ),
    );
    return Array.from(digest)
      .map((byte) => byte.toString(16).padStart(2, "0"))
      .join("")
      .slice(0, 24);
  });

  const storePath: RewindStore["Service"]["storePath"] = (storeId) =>
    Effect.succeed(path.join(config.rewindStoresDir, storeId));

  const git = Effect.fn("RewindStore.git")(function* (input: {
    readonly operation: string;
    readonly handle: RewindStoreHandle;
    readonly args: ReadonlyArray<string>;
    readonly allowNonZeroExit?: boolean;
  }) {
    return yield* process
      .run({
        operation: input.operation,
        command: "git",
        cwd: input.handle.cwd,
        args: ["--git-dir", input.handle.gitDir, "--work-tree", input.handle.cwd, ...input.args],
        timeoutMs: GIT_TIMEOUT_MS,
        // Snapshot diffs of a large worktree can be sizable; the default 1MB
        // cap would silently truncate the changed-file list.
        maxOutputBytes: 10_000_000,
        ...(input.allowNonZeroExit === true ? { allowNonZeroExit: true } : {}),
      })
      .pipe(
        Effect.mapError((cause) =>
          rewindError({
            operation: input.operation,
            detail: `git ${input.args[0] ?? "?"} failed in rewind store.`,
            cause,
          }),
        ),
      );
  });

  const addAll = (handle: RewindStoreHandle, operation: string) =>
    git({
      operation,
      handle,
      args: ["add", "--all", "--", ".", `:(exclude)${EXCLUDED_TOP_LEVEL_DIRECTORY}`],
    });

  const ensureStore = Effect.fn("RewindStore.ensureStore")(function* (handle: RewindStoreHandle) {
    if (initializedStores.has(handle.gitDir)) {
      return;
    }

    yield* fileSystem.makeDirectory(handle.gitDir, { recursive: true }).pipe(
      Effect.mapError((cause) =>
        rewindError({
          operation: "RewindStore.ensureStore",
          detail: "Failed to create rewind store directory.",
          cause,
        }),
      ),
    );
    yield* git({ operation: "RewindStore.ensureStore.init", handle, args: ["init", "--quiet"] });
    // Pin the settings that would otherwise make a snapshot lossy: line-ending
    // rewrites, symlink flattening, and long-path rejection on Windows.
    // fsmonitor is disabled because the store shares a worktree with the
    // user's own repository and must not race its daemon.
    for (const [key, value] of [
      ["core.autocrlf", "false"],
      ["core.longpaths", "true"],
      ["core.symlinks", "true"],
      ["core.fsmonitor", "false"],
      ["gc.auto", "0"],
    ] as const) {
      yield* git({
        operation: "RewindStore.ensureStore.config",
        handle,
        args: ["config", key, value],
      });
    }
    yield* fileSystem
      .writeFileString(
        path.join(handle.gitDir, META_FILE),
        yield* encodeStoreMetadata({ storeId: handle.storeId, cwd: handle.cwd }).pipe(
          Effect.orElseSucceed(() => ""),
        ),
      )
      .pipe(Effect.ignore);

    initializedStores.add(handle.gitDir);
  });

  const open: RewindStore["Service"]["open"] = Effect.fn("RewindStore.open")(function* (input) {
    const storeId = yield* storeIdForThread(input.threadId);
    const gitDir = yield* storePath(storeId);
    const handle: RewindStoreHandle = { storeId, gitDir, cwd: input.cwd };
    yield* ensureStore(handle);
    return handle;
  });

  const snapshot: RewindStore["Service"]["snapshot"] = Effect.fn("RewindStore.snapshot")(
    function* (handle) {
      yield* ensureStore(handle);
      yield* addAll(handle, "RewindStore.snapshot.add");
      const result = yield* git({
        operation: "RewindStore.snapshot.writeTree",
        handle,
        args: ["write-tree"],
      });
      const tree = result.stdout.trim();
      if (tree.length === 0) {
        return yield* rewindError({
          operation: "RewindStore.snapshot",
          detail: "git write-tree returned an empty tree oid.",
        });
      }
      // Anchor the tree behind a ref so a stray `git gc`/`prune` inside the
      // store cannot drop the snapshot that undo depends on.
      yield* git({
        operation: "RewindStore.snapshot.anchor",
        handle,
        args: ["update-ref", `refs/t3/rewind/${tree}`, `${tree}^{tree}`],
        allowNonZeroExit: true,
      });
      return tree;
    },
  );

  const changedFiles: RewindStore["Service"]["changedFiles"] = Effect.fn(
    "RewindStore.changedFiles",
  )(function* (input) {
    yield* ensureStore(input.handle);
    const result = yield* git({
      operation: "RewindStore.changedFiles",
      handle: input.handle,
      args: [
        "diff",
        "--name-only",
        "-z",
        "--no-renames",
        input.fromTree,
        input.toTree,
        "--",
        ".",
        `:(exclude)${EXCLUDED_TOP_LEVEL_DIRECTORY}`,
      ],
    });
    return parseNulSeparatedPaths(result.stdout);
  });

  const restoreFiles: RewindStore["Service"]["restoreFiles"] = Effect.fn(
    "RewindStore.restoreFiles",
  )(function* (input) {
    yield* ensureStore(input.handle);
    const restored: Array<string> = [];

    for (const file of input.files) {
      const relativePath = normalizeRewindPath(file);
      if (relativePath === undefined) {
        yield* Effect.logWarning("rewind restore skipped unsafe path", { file });
        continue;
      }

      const listed = yield* git({
        operation: "RewindStore.restoreFiles.lsTree",
        handle: input.handle,
        args: ["ls-tree", input.tree, "--", relativePath],
        allowNonZeroExit: true,
      });

      if (listed.exitCode === 0 && listed.stdout.trim().length > 0) {
        yield* git({
          operation: "RewindStore.restoreFiles.checkout",
          handle: input.handle,
          args: ["checkout", input.tree, "--", relativePath],
        });
      } else {
        // Absent from the snapshot: the turn created it, so undo removes it.
        yield* fileSystem
          .remove(path.join(input.handle.cwd, relativePath), { recursive: true, force: true })
          .pipe(Effect.ignore);
      }
      restored.push(relativePath);
    }

    // `checkout <tree> -- <path>` stages into the store's own index; refresh it
    // so the next snapshot diff is taken against what is now on disk.
    yield* addAll(input.handle, "RewindStore.restoreFiles.add");
    return restored;
  });

  const measureDirectory = (directory: string): Effect.Effect<number> =>
    Effect.gen(function* () {
      const entries = yield* fileSystem
        .readDirectory(directory, { recursive: false })
        .pipe(Effect.orElseSucceed(() => [] as ReadonlyArray<string>));
      let total = 0;
      for (const entry of entries) {
        const entryPath = path.join(directory, entry);
        const info = yield* fileSystem.stat(entryPath).pipe(Effect.orElseSucceed(() => null));
        if (info === null) {
          continue;
        }
        if (info.type === "Directory") {
          total += yield* measureDirectory(entryPath);
          continue;
        }
        total += Number(info.size);
      }
      return total;
    });

  const listUsage: RewindStore["Service"]["listUsage"] = Effect.fn("RewindStore.listUsage")(
    function* () {
      const storeIds = yield* fileSystem
        .readDirectory(config.rewindStoresDir, { recursive: false })
        .pipe(Effect.orElseSucceed(() => [] as ReadonlyArray<string>));
      const usage: Array<RewindStoreUsage> = [];
      for (const storeId of storeIds) {
        const directory = path.join(config.rewindStoresDir, storeId);
        const info = yield* fileSystem.stat(directory).pipe(Effect.orElseSucceed(() => null));
        if (info === null || info.type !== "Directory") {
          continue;
        }
        usage.push({
          storeId,
          path: directory,
          bytes: yield* measureDirectory(directory),
        });
      }
      return usage;
    },
  );

  const deleteStore: RewindStore["Service"]["deleteStore"] = Effect.fn("RewindStore.deleteStore")(
    function* (storeId) {
      const directory = yield* storePath(storeId);
      const bytes = yield* measureDirectory(directory);
      yield* fileSystem.remove(directory, { recursive: true, force: true }).pipe(Effect.ignore);
      initializedStores.delete(directory);
      return bytes;
    },
  );

  return RewindStore.of({
    storeIdForThread,
    storePath,
    open,
    snapshot,
    changedFiles,
    restoreFiles,
    listUsage,
    deleteStore,
  });
});

export const layer = Layer.effect(RewindStore, make);
