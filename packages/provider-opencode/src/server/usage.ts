import { expandHomePath } from "@t3tools/provider-core/server/pathExpansion";
import {
  totalTokens,
  type ProviderUsageReader,
  type UsageRecord,
} from "@t3tools/provider-core/server/usage";
import * as HostProcess from "@t3tools/shared/HostProcess";
import * as NodeSqliteClient from "@t3tools/shared/nodeSqliteClient";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Option from "effect/Option";
import * as Path from "effect/Path";
import type * as PlatformError from "effect/PlatformError";
import * as Semaphore from "effect/Semaphore";
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

// OpenCode provider IDs whose bare model IDs LiteLLM prices under its own prefix.
const LITELLM_PREFIXES = new Map([["opencode-go", "opencode_go"]]);
const OPENCODE_GATEWAYS = new Set(["opencode", "opencode-go"]);

/**
 * Kiro reports full prompt context as `tokens.input` with no cache split. Like
 * the pi parser, treat the previous context per session and model as cached prefix.
 */
type KiroContextMap = Map<string, number>;

// Records must arrive in native session order. Parsed records stay unestimated, so cached rows
// re-run this on every scan.
function estimateKiroCache(
  kiroContext: KiroContextMap,
  record: UsageRecord | null,
): UsageRecord | null {
  if (record === null || record.apiProvider !== "kiro") return record;
  const { totals } = record;
  const reportedInputTokens = totals.uncachedInputTokens;
  if (reportedInputTokens === 0 || totals.cachedInputTokens !== 0) return record;
  if (totals.cacheCreationTokens !== 0) return record;
  const key = `${record.sessionId}\u0000${record.model}`;
  const previousContextTokens = kiroContext.get(key) ?? 0;
  const cachedInputTokens =
    reportedInputTokens >= previousContextTokens ? previousContextTokens : 0;
  kiroContext.set(key, reportedInputTokens);
  return {
    ...record,
    totals: {
      ...totals,
      uncachedInputTokens: reportedInputTokens - cachedInputTokens,
      cachedInputTokens,
    },
    inputTokensEstimated: true,
  };
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

  const totals = {
    uncachedInputTokens: tokens(usage.input),
    cachedInputTokens: tokens(cache.read),
    cacheCreationTokens: tokens(cache.write),
    outputTokens: tokens(usage.output) + reasoningTokens,
    reasoningTokens,
  };
  if (totalTokens(totals) === 0) return null;
  const id = fallback.id || text(message.id);
  const cost = message.cost;
  const prefix = LITELLM_PREFIXES.get(apiProvider);
  const free = OPENCODE_GATEWAYS.has(apiProvider) && model.endsWith("-free");
  return {
    provider: "opencode",
    timestampMs,
    model,
    apiProvider,
    ...(prefix !== undefined && !model.includes("/") ? { rateModel: `${prefix}/${model}` } : {}),
    sessionId,
    totals,
    inputTokensEstimated: false,
    // OpenCode records zero for unknown rates and paid subscriptions alike, so only its own
    // gateways' free models report zero; shared pricing estimates every other zero.
    reportedCostUsd:
      typeof cost === "number" && Number.isFinite(cost) && (cost > 0 || (free && cost === 0))
        ? cost
        : null,
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

interface CachedRow {
  readonly updated: unknown;
  readonly size: unknown;
  readonly created: unknown;
  readonly record: UsageRecord | null;
}

// Parsed rows by `${databasePath}\0${table}`, then rowid, so unchanged rows are not read again.
const rowCache = new Map<string, { version: string; rows: Map<number, CachedRow> }>();
// Concurrent scans of the same database would each read every uncached row.
const scanLock = Semaphore.makeUnsafe(1);

const isNotFound = (cause: PlatformError.PlatformError) => cause.reason._tag === "NotFound";
const MESSAGE_PAGE_SIZE = 64;

const isSymbolicLink = Effect.fnUntraced(function* (path: string) {
  const fileSystem = yield* FileSystem.FileSystem;
  return yield* Effect.isSuccess(fileSystem.readLink(path));
});

const entryInfo = Effect.fn("entryInfo")(function* (path: string) {
  const fileSystem = yield* FileSystem.FileSystem;
  if (yield* isSymbolicLink(path))
    return { type: "SymbolicLink" as const, mtime: Option.none<Date>() };
  return yield* fileSystem.stat(path);
});

const LEGACY_STAT_CONCURRENCY = 4;

// Batch stats so huge directories do not stay in memory.
const LEGACY_STAT_BATCH = 256;

export const readOpenCodeUsage = Effect.fn("readOpenCodeUsage")(function* (
  root: string,
  sinceMs: number,
  retentionCutoffMs = sinceMs,
): Effect.fn.Return<OpenCodeUsageReadResult, never, FileSystem.FileSystem | Path.Path> {
  const fileSystem = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const files: { path: string; records: UsageRecord[] }[] = [];
  const seen = new Set<string>();
  const kiroContext: KiroContextMap = new Map();
  let found = false;
  let error = false;
  let databaseRows = 0;
  let databasePages = 0;
  const append = (records: UsageRecord[], record: UsageRecord | null) => {
    if (record === null || record.timestampMs < sinceMs) return;
    if (record.dedupeKey !== null) {
      if (seen.has(record.dedupeKey)) return;
      seen.add(record.dedupeKey);
    }
    records.push(record);
  };

  const rootIsFile = yield* entryInfo(root).pipe(
    Effect.map((info) => info.type === "File"),
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
              entryInfo(path.join(rootDir, name)).pipe(
                Effect.map((info) => info.type === "File"),
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
    // Taken before the snapshot opens, so a write that races the scan invalidates its rows.
    const [main, wal] = yield* Effect.forEach([file.path, `${file.path}-wal`], (target) =>
      Effect.option(fileSystem.stat(target)),
    );
    const stamp = (info: Option.Option<FileSystem.File.Info>) =>
      Option.match(info, {
        onNone: () => "none",
        onSome: (stat) => `${stat.size}:${Option.getOrNull(stat.mtime)?.getTime()}`,
      });
    // A replaced or restored database can repeat rowids and change markers, so every key includes
    // the main file; WAL commits leave it alone until a checkpoint.
    const identity = Option.match(main!, {
      onNone: () => "none",
      onSome: (stat) =>
        `${stat.dev}:${Option.getOrNull(stat.ino)}:${Option.getOrNull(stat.birthtime)?.getTime()}/${stamp(main!)}`,
    });
    yield* Effect.gen(function* () {
      const sql = yield* SqlClient.SqlClient;
      // Fail promptly on live-writer locks instead of stalling the server.
      yield* sql.unsafe("PRAGMA busy_timeout = 100");
      const tables = new Set(
        (yield* sql.unsafe<{ readonly name: unknown }>(
          "SELECT name FROM sqlite_master WHERE type = 'table'",
        )).map((row) => row.name),
      );
      if (!tables.has("message") && !tables.has("session_message")) error = true;
      const columnsOf = (table: string) =>
        sql
          .unsafe<{ readonly name: unknown }>(`PRAGMA table_info(${table})`)
          .pipe(Effect.map((rows) => new Set(rows.map((row) => row.name))));
      // A fork copies its parent's messages through the boundary under new ids, keeping
      // seq, creation time and data. Resolve each fork to its last copied seq so a copy
      // names its original as (session, seq). A fork whose boundary is gone keeps its usage.
      const forks = new Map<
        string,
        { readonly parent: string; readonly lastCopiedSeq: number; readonly created: number }
      >();
      if (
        tables.has("session_v2") &&
        tables.has("session_message") &&
        (yield* columnsOf("session_message")).has("seq") &&
        (yield* columnsOf("session_v2")).has("fork_boundary")
      ) {
        const resolved = yield* sql.unsafe<{
          readonly id: unknown;
          readonly parent: unknown;
          readonly created: unknown;
          readonly last_copied_seq: unknown;
        }>(
          `SELECT fork.id, fork.fork_session_id AS parent, fork.time_created AS created,
             (SELECT MAX(copied.seq) FROM session_message AS copied
              WHERE copied.session_id = fork.fork_session_id
                AND CASE json_extract(fork.fork_boundary, '$.type')
                  WHEN 'through' THEN copied.seq <= boundary.seq
                  WHEN 'before' THEN copied.seq < boundary.seq
                END) AS last_copied_seq
           FROM session_v2 AS fork
           JOIN session_message AS boundary
             ON boundary.session_id = fork.fork_session_id
            AND boundary.id = json_extract(fork.fork_boundary, '$.messageID')
           WHERE fork.fork_session_id IS NOT NULL AND json_valid(fork.fork_boundary)`,
        );
        for (const row of resolved) {
          if (typeof row.last_copied_seq !== "number" || typeof row.created !== "number") continue;
          forks.set(text(row.id), {
            parent: text(row.parent),
            lastCopiedSeq: row.last_copied_seq,
            created: row.created,
          });
        }
      }
      const original = (sessionId: string, seq: number, created: number) => {
        const visited = new Set<string>();
        let session = sessionId;
        for (
          let fork = forks.get(session);
          fork !== undefined &&
          seq <= fork.lastCopiedSeq &&
          created < fork.created &&
          !visited.has(session);
          fork = forks.get(session)
        ) {
          visited.add(session);
          session = fork.parent;
        }
        return `${session}:${seq}`;
      };
      const originals = new Set<string>();
      for (const table of ["message", "session_message"] as const) {
        if (!tables.has(table)) continue;
        const columns = yield* columnsOf(table);
        const timestamp = columns.has("time_created") ? "time_created" : "NULL";
        const updated = columns.has("time_updated") ? "time_updated" : "NULL";
        const lineage =
          table === "session_message" && forks.size > 0 && columns.has("seq") ? "seq" : "NULL";
        // Without time_updated an in-place edit can keep the length, so any database write counts.
        const version = updated === "NULL" ? `${identity}/${stamp(wal!)}` : identity;
        // OpenCode 2 records a compaction request's spend on its compaction message.
        const predicates =
          table === "session_message" ? ["type IN ('assistant', 'compaction')"] : [];
        if (timestamp !== "NULL") predicates.push("time_created >= ?");
        // List rowids and change markers so SQLite can use time indexes without reading message
        // bodies; octet_length comes from the record header.
        const order = columns.has("seq")
          ? "COALESCE(session_id, '') AS order_session, COALESCE(seq, 0) AS order_sequence"
          : "'' AS order_session, 0 AS order_sequence";
        const listed = (yield* sql.unsafe<{
          readonly row_id: number;
          readonly updated: unknown;
          readonly size: unknown;
          readonly created: unknown;
          readonly seq: unknown;
          readonly order_session: string;
          readonly order_sequence: number;
        }>(
          `SELECT rowid AS row_id, ${updated} AS updated, octet_length(data) AS size,
             ${timestamp} AS created, ${lineage} AS seq, ${order}
           FROM ${table}
           ${predicates.length ? `WHERE ${predicates.join(" AND ")}` : ""}`,
          timestamp === "NULL" ? [] : [sinceMs],
        )).toSorted((a, b) => a.row_id - b.row_id);
        databaseRows += listed.length;
        const key = `${file.path}\0${table}`;
        const previous = rowCache.get(key);
        const reusable = previous?.version === version ? previous.rows : undefined;
        const cached = new Map<number, CachedRow>();
        const misses: typeof listed = [];
        for (const row of listed) {
          const hit = reusable?.get(row.row_id);
          if (hit && hit.updated === row.updated && hit.size === row.size)
            cached.set(row.row_id, hit);
          else misses.push(row);
        }
        // Filter and project in SQLite: user diffs and assistant bodies can be hundreds of MB.
        const validAssistant = `CASE WHEN typeof(data) = 'text' AND json_valid(data)
          THEN json_type(data, '$.role') IS NULL OR data ->> '$.role' = 'assistant' END`;
        // Omit per-page spans; the enclosing scan records row and page counts.
        for (let start = 0; start < misses.length; start += MESSAGE_PAGE_SIZE) {
          const page = misses.slice(start, start + MESSAGE_PAGE_SIZE);
          databasePages++;
          const rows = yield* sql
            .unsafe<{
              readonly row_id: number;
              readonly id: unknown;
              readonly session_id: unknown;
              readonly data: unknown;
              readonly created: unknown;
            }>(
              `SELECT rowid AS row_id, id, session_id, ${timestamp} AS created, json_object(
                 'tokens', data -> '$.tokens', 'cost', data -> '$.cost',
                 'model', data -> '$.model', 'modelID', data -> '$.modelID',
                 'providerID', data -> '$.providerID',
                 'time', data -> '$.time', 'id', data -> '$.id', 'sessionID', data -> '$.sessionID'
               ) AS data
               FROM ${table}
               WHERE rowid IN (${page.map(() => "?").join(", ")}) AND ${validAssistant}
               ORDER BY rowid`,
              page.map((row) => row.row_id),
            )
            .pipe(Effect.withTracerEnabled(false));
          const parsed = new Map<number, UsageRecord | null>();
          for (const row of rows) {
            parsed.set(
              row.row_id,
              parseOpenCodeMessage(text(row.data), {
                id: text(row.id),
                sessionId: text(row.session_id),
                ...(typeof row.created === "number" ? { timestampMs: row.created } : {}),
              }),
            );
          }
          // Rows the SQL filter dropped are cached as empty so they are not read again either.
          for (const row of page) {
            cached.set(row.row_id, {
              updated: row.updated,
              size: row.size,
              created: row.created,
              record: parsed.get(row.row_id) ?? null,
            });
          }
          yield* Effect.yieldNow;
        }
        const kept: typeof listed = [];
        for (const row of listed) {
          const record = cached.get(row.row_id)!.record;
          if (record !== null && typeof row.seq === "number") {
            const copyOf = original(record.sessionId, row.seq, record.timestampMs);
            if (originals.has(copyOf)) continue;
            originals.add(copyOf);
          }
          kept.push(row);
        }
        // Kiro cache estimates follow native session order, which `seq` keeps after imports.
        const estimated = new Map<number, UsageRecord | null>();
        for (const row of kept.toSorted(
          (left, right) =>
            (left.order_session < right.order_session
              ? -1
              : left.order_session > right.order_session
                ? 1
                : 0) ||
            left.order_sequence - right.order_sequence ||
            left.row_id - right.row_id,
        )) {
          estimated.set(row.row_id, estimateKiroCache(kiroContext, cached.get(row.row_id)!.record));
        }
        for (const row of kept) append(file.records, estimated.get(row.row_id)!);
        // Rows before this window were not listed; keep them for a wider window until retention.
        for (const [rowId, entry] of reusable ?? []) {
          const created = entry.created;
          if (typeof created === "number" && created < sinceMs && created >= retentionCutoffMs)
            cached.set(rowId, entry);
        }
        rowCache.set(key, { version, rows: cached });
      }
    }).pipe(
      (read) => Effect.flatMap(SqlClient.SqlClient, (sql) => sql.withTransaction(read)),
      // Run the scan on a reader worker so large databases do not block the server's event loop.
      NodeSqliteClient.readOnly,
      Effect.provide(
        NodeSqliteClient.layer({ filename: file.path, readonly: true, readerWorker: true }),
      ),
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
      const names = yield* fileSystem.readDirectory(directory);
      for (let start = 0; start < names.length; start += LEGACY_STAT_BATCH) {
        const batch = names.slice(start, start + LEGACY_STAT_BATCH);
        const entries = batch.map((name) => path.join(directory, name));
        // stat follows symlinks; avoid expensive readLink failures unless link identity matters.
        const infos = yield* Effect.forEach(
          entries,
          (entry) =>
            fileSystem.stat(entry).pipe(
              Effect.catchTags({
                PlatformError: (cause) =>
                  isNotFound(cause)
                    ? Effect.succeed(null)
                    : isSymbolicLink(entry).pipe(
                        Effect.flatMap((link) =>
                          link ? Effect.succeed(null) : Effect.fail(cause),
                        ),
                      ),
              }),
              Effect.exit,
            ),
          { concurrency: LEGACY_STAT_CONCURRENCY },
        );
        for (const [index, name] of batch.entries()) {
          const entry = entries[index]!;
          const info = yield* infos[index]!;
          if (info?.type === "Directory") {
            if (!(yield* isSymbolicLink(entry))) directories.push(entry);
          } else if (info?.type === "File" && name.endsWith(".json")) {
            const id = name.slice(0, -5);
            // Messages cannot postdate file mtime; older files hold no in-window usage.
            const skip =
              seen.has(`opencode:${id}`) ||
              Option.exists(info.mtime, (mtime) => mtime.getTime() < sinceMs);
            if (found && skip) continue;
            if (yield* isSymbolicLink(entry)) continue;
            found = true;
            if (skip) continue;
            const file = { path: entry, records: [] as UsageRecord[] };
            files.push(file);
            yield* fileSystem.readFileString(entry).pipe(
              Effect.map((source) =>
                append(
                  file.records,
                  estimateKiroCache(kiroContext, parseOpenCodeMessage(source, { id })),
                ),
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
  yield* Effect.annotateCurrentSpan({
    "usage.database_rows": databaseRows,
    "usage.database_pages": databasePages,
  });
  return { files, missing: !found && !error, error };
}, scanLock.withPermit);

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
  scan: Effect.fn("openCodeUsageReader.scan")(function* ({
    instances,
    windowStartMs,
    retentionCutoffMs,
  }) {
    const roots = new Set<string>();
    for (const instance of instances) {
      // A remote OpenCode server keeps its history on that machine.
      if (instance.config?.serverUrl.trim()) continue;
      for (const root of yield* resolveOpenCodeDataDirs(instance.environment)) roots.add(root);
    }
    return yield* Effect.forEach(
      [...roots],
      (dir) =>
        readOpenCodeUsage(dir, windowStartMs, retentionCutoffMs).pipe(
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
