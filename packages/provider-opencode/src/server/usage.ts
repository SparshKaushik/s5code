/**
 * Usage history for OpenCode, read from its SQLite databases and legacy JSON
 * message store under each data directory.
 *
 * @module provider-opencode/server/usage
 */

import { expandHomePath } from "@t3tools/provider-core/server/pathExpansion";
import {
  totalTokens,
  type ProviderUsageReader,
  type UsageRecord,
} from "@t3tools/provider-core/server/usage";
import * as HostProcess from "@t3tools/shared/HostProcess";
import * as NodeSqliteClient from "@t3tools/shared/nodeSqliteClient";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import type * as PlatformError from "effect/PlatformError";
import * as SqlClient from "effect/sql/SqlClient";

import type { OpenCodeSettings } from "../settings.ts";

function object(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function tokens(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? Math.trunc(value) : 0;
}

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

/**
 * Kiro reports the full prompt context as `tokens.input` with no cache split,
 * unlike every other provider behind OpenCode. A rolling-prefix estimator
 * treats the previous request's context as the cached prefix for the next one,
 * which is what the upstream pi parser does for the same quirk. The map is
 * keyed per session and model so a context reset in one session does not
 * leak into another's estimate.
 */
type KiroContextMap = Map<string, number>;

function estimateKiroSplit(
  kiroContext: KiroContextMap,
  sessionId: string,
  model: string,
  reportedInputTokens: number,
): { uncachedInputTokens: number; cachedInputTokens: number } {
  const key = `${sessionId}\u0000${model}`;
  const previousContextTokens = kiroContext.get(key) ?? 0;
  const cachedInputTokens =
    reportedInputTokens >= previousContextTokens ? previousContextTokens : 0;
  kiroContext.set(key, reportedInputTokens);
  return { uncachedInputTokens: reportedInputTokens - cachedInputTokens, cachedInputTokens };
}

/**
 * OpenCode stores uncached input and reasoning separately from input/output.
 * `model.providerID` becomes the record's `apiProvider` because OpenCode talks
 * to gateways, so provider-scoped catalog pricing needs it.
 */
function parseOpenCodeMessage(
  source: string,
  fallback: {
    readonly id?: string;
    readonly sessionId?: string;
    readonly timestampMs?: number;
    readonly kiroContext?: KiroContextMap;
  } = {},
): UsageRecord | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(source);
  } catch {
    return null;
  }
  const message = object(parsed);
  if (message.role !== undefined && message.role !== "assistant") return null;
  const usage = object(message.tokens);
  const cache = object(usage.cache);
  const modelReference = object(message.model);
  const model = text(modelReference.id) || text(modelReference.modelID) || text(message.modelID);
  const apiProvider =
    text(modelReference.providerID) || text(modelReference.provider) || text(message.providerID);
  const timestampMs =
    object(message.time).completed ?? object(message.time).created ?? fallback.timestampMs;
  if (!model || typeof timestampMs !== "number" || !Number.isFinite(timestampMs)) return null;
  const reasoningTokens = tokens(usage.reasoning);
  const sessionId = fallback.sessionId || text(message.sessionID);

  const reportedInputTokens = tokens(usage.input);
  const inputTokensEstimated =
    apiProvider === "kiro" &&
    fallback.kiroContext !== undefined &&
    reportedInputTokens > 0 &&
    tokens(cache.read) === 0 &&
    tokens(cache.write) === 0;
  const { uncachedInputTokens, cachedInputTokens } = inputTokensEstimated
    ? estimateKiroSplit(fallback.kiroContext, sessionId, model, reportedInputTokens)
    : { uncachedInputTokens: reportedInputTokens, cachedInputTokens: tokens(cache.read) };

  const totals = {
    uncachedInputTokens,
    cachedInputTokens,
    cacheCreationTokens: tokens(cache.write),
    outputTokens: tokens(usage.output) + reasoningTokens,
    reasoningTokens,
  };
  if (totalTokens(totals) === 0) return null;
  const id = fallback.id || text(message.id);
  const cost = message.cost;
  return {
    provider: "opencode",
    timestampMs,
    model,
    apiProvider,
    sessionId,
    totals,
    inputTokensEstimated,
    // OpenCode writes zero for models without a known rate, including paid
    // subscription models. Let the shared price table estimate those records.
    reportedCostUsd: typeof cost === "number" && Number.isFinite(cost) && cost > 0 ? cost : null,
    speed: "standard",
    dedupeKey: id ? `opencode:${id}` : null,
  };
}

export interface OpenCodeUsageReadResult {
  readonly files: readonly { readonly path: string; readonly records: readonly UsageRecord[] }[];
  readonly missing: boolean;
  readonly error: boolean;
}

export function resolveOpenCodeDatabasePath(
  home: string,
  environment: Readonly<Record<string, string | undefined>>,
  path: Pick<Path.Path, "join" | "isAbsolute">,
): string {
  const override = environment["OPENCODE_DB"]?.trim();
  if (override) return expandHomePath(override, home);
  const dataHome = environment["XDG_DATA_HOME"]?.trim();
  const base =
    dataHome && path.isAbsolute(dataHome) ? dataHome : path.join(home, ".local", "share");
  return path.join(base, "opencode", "opencode.db");
}

const isNotFound = (cause: PlatformError.PlatformError) => cause.reason._tag === "NotFound";

/** A regular file or directory, never a symlink to one. */
const entryType = Effect.fn("entryType")(function* (path: string) {
  const fileSystem = yield* FileSystem.FileSystem;
  const link = yield* Effect.exit(fileSystem.readLink(path));
  if (Exit.isSuccess(link)) return "SymbolicLink" as const;
  return (yield* fileSystem.stat(path)).type;
});

/** Reads current SQLite and pre-migration JSON stores without modifying either. */
export const readOpenCodeUsage = Effect.fn("readOpenCodeUsage")(function* (
  root: string,
  sinceMs: number,
): Effect.fn.Return<OpenCodeUsageReadResult, never, FileSystem.FileSystem | Path.Path> {
  const fileSystem = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const files: { path: string; records: UsageRecord[] }[] = [];
  const seen = new Set<string>();
  const kiroContext: KiroContextMap = new Map();
  let found = false;
  let error = false;
  const append = (records: UsageRecord[], record: UsageRecord | null) => {
    if (record === null || record.timestampMs < sinceMs) return;
    if (record.dedupeKey !== null) {
      if (seen.has(record.dedupeKey)) return;
      seen.add(record.dedupeKey);
    }
    records.push(record);
  };

  const rootIsFile = yield* entryType(root).pipe(
    Effect.map((type) => type === "File"),
    Effect.orElseSucceed(() => false),
  );
  const rootDir = rootIsFile ? path.dirname(root) : root;
  const databases = rootIsFile
    ? [path.basename(root)]
    : yield* fileSystem.readDirectory(root).pipe(
        Effect.flatMap((names) =>
          Effect.filter(
            names.filter((name) => /^opencode(?:-[a-zA-Z0-9_-]+)?\.db$/.test(name)),
            // One database removed between listing and stat must not hide the rest.
            (name) =>
              entryType(path.join(rootDir, name)).pipe(
                Effect.map((type) => type === "File"),
                Effect.catchTags({
                  PlatformError: (cause) =>
                    isNotFound(cause) ? Effect.succeed(false) : Effect.fail(cause),
                }),
              ),
          ),
        ),
        Effect.map((names) =>
          names.sort((a, b) =>
            a === "opencode.db" ? -1 : b === "opencode.db" ? 1 : a.localeCompare(b),
          ),
        ),
        Effect.catchTags({
          PlatformError: (cause) => {
            if (!isNotFound(cause)) error = true;
            return Effect.succeed([]);
          },
        }),
      );
  for (const name of databases) {
    found = true;
    const file = { path: path.join(rootDir, name), records: [] as UsageRecord[] };
    files.push(file);
    yield* Effect.gen(function* () {
      const sql = yield* SqlClient.SqlClient;
      // A busy live provider should fail this source promptly rather than
      // stalling the server while SQLite waits for its writer.
      yield* sql.unsafe("PRAGMA busy_timeout = 100");
      const tables = new Set(
        (yield* sql.unsafe<{ readonly name: unknown }>(
          "SELECT name FROM sqlite_master WHERE type = 'table'",
        )).map((row) => row.name),
      );
      if (!tables.has("message") && !tables.has("session_message")) error = true;
      for (const table of ["message", "session_message"] as const) {
        if (!tables.has(table)) continue;
        const columns = new Set(
          (yield* sql.unsafe<{ readonly name: unknown }>(`PRAGMA table_info(${table})`)).map(
            (row) => row.name,
          ),
        );
        const timestamp = columns.has("time_created") ? "time_created" : "NULL";
        const predicates = table === "session_message" ? ["type = 'assistant'"] : [];

        if (timestamp !== "NULL") predicates.push("time_created >= ?");
        const orderBy = columns.has("seq")
          ? " ORDER BY session_id, seq"
          : ` ORDER BY session_id, ${timestamp === "NULL" ? "id" : "time_created"}, id`;
        const where = predicates.length > 0 ? ` WHERE ${predicates.join(" AND ")}` : "";
        const rows = yield* sql.unsafe<{
          readonly id: unknown;
          readonly session_id: unknown;
          readonly data: unknown;
          readonly created: unknown;
        }>(
          `SELECT id, session_id, data, ${timestamp} AS created FROM ${table}${where}${orderBy}`,
          timestamp === "NULL" ? [] : [sinceMs],
        );
        for (const [index, row] of rows.entries()) {
          append(
            file.records,
            parseOpenCodeMessage(text(row.data), {
              kiroContext,
              id: text(row.id),
              sessionId: text(row.session_id),
              ...(typeof row.created === "number" ? { timestampMs: row.created } : {}),
            }),
          );
          if (index % 256 === 255) yield* Effect.yieldNow;
        }
      }
    }).pipe(
      Effect.provide(NodeSqliteClient.layer({ filename: file.path, readonly: true })),
      Effect.catchTags({
        SqlError: () => {
          error = true;
          return Effect.void;
        },
      }),
    );
  }

  // Do not follow symlinks, including cycles. Database records win over their
  // old JSON copies when OpenCode has migrated a store in place.
  const directories = [path.join(rootDir, "storage", "message")];
  while (directories.length > 0) {
    const directory = directories.pop()!;
    yield* Effect.gen(function* () {
      for (const name of yield* fileSystem.readDirectory(directory)) {
        const entry = path.join(directory, name);
        const type = yield* entryType(entry).pipe(
          Effect.catchTags({
            PlatformError: (cause) =>
              isNotFound(cause) ? Effect.succeed(null) : Effect.fail(cause),
          }),
        );
        if (type === "Directory") {
          directories.push(entry);
        } else if (type === "File" && name.endsWith(".json")) {
          found = true;
          const id = name.slice(0, -5);
          if (seen.has(`opencode:${id}`)) continue;
          const file = { path: entry, records: [] as UsageRecord[] };
          files.push(file);
          yield* fileSystem.readFileString(entry).pipe(
            Effect.map((source) =>
              append(file.records, parseOpenCodeMessage(source, { id, kiroContext })),
            ),
            Effect.catchTags({
              PlatformError: (cause) => {
                if (!isNotFound(cause)) error = true;
                return Effect.void;
              },
            }),
          );
        }
      }
    }).pipe(
      Effect.catchTags({
        PlatformError: (cause) => {
          if (!isNotFound(cause)) error = true;
          return Effect.void;
        },
      }),
    );
  }
  return { files, missing: !found && !error, error };
});

/**
 * The data directories to read: `OPENCODE_DATA_DIR` (comma-separated) or the
 * XDG default, canonicalized so aliases count once.
 */
const resolveOpenCodeDataDirs = Effect.fn("resolveOpenCodeDataDirs")(function* (
  environment: NodeJS.ProcessEnv,
) {
  const fileSystem = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const homeDirectory = yield* HostProcess.HomeDirectory;
  const roots = environment["OPENCODE_DB"]?.trim()
    ? [environment["OPENCODE_DB"]!.trim()]
    : environment["OPENCODE_DATA_DIR"]
        ?.split(",")
        .map((value) => value.trim())
        .filter(Boolean);
  const defaults = [path.dirname(resolveOpenCodeDatabasePath(homeDirectory, environment, path))];
  const canonical = new Set<string>();
  for (const root of roots?.length ? roots : defaults) {
    const resolved = path.resolve(expandHomePath(root, homeDirectory));
    canonical.add(yield* fileSystem.realPath(resolved).pipe(Effect.orElseSucceed(() => resolved)));
  }
  return [...canonical];
});

export type OpenCodeUsageReaderEnv = FileSystem.FileSystem | Path.Path;

export const openCodeUsageReader: ProviderUsageReader<OpenCodeSettings, OpenCodeUsageReaderEnv> = {
  kind: "scan",
  provider: "opencode",
  scan: Effect.fn("openCodeUsageReader.scan")(function* ({ instances, windowStartMs }) {
    const hostEnvironment = yield* HostProcess.Environment;
    const roots = new Set(yield* resolveOpenCodeDataDirs(hostEnvironment));
    for (const instance of instances) {
      if (instance.config?.serverUrl.trim()) continue;
      for (const root of yield* resolveOpenCodeDataDirs(instance.environment)) roots.add(root);
    }
    return yield* Effect.forEach(
      [...roots],
      (dir) =>
        readOpenCodeUsage(dir, windowStartMs).pipe(
          Effect.map((result) => ({
            dir,
            files: result.missing && !result.error ? null : result.files,
            status: result.error ? ("partial" as const) : ("ok" as const),
            ...(result.error ? { message: "Some OpenCode history could not be read." } : {}),
          })),
        ),
      { concurrency: "unbounded" },
    );
  }),
};
