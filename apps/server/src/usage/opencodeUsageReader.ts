// SQLite reads live OpenCode databases; Node fs walks legacy JSON history.
// The databases go through sqliteCompat so the compiled binary can run under
// Bun, where `node:sqlite` does not exist.
// @effect-diagnostics nodeBuiltinImport:off
import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import * as NodeTimersPromises from "node:timers/promises";

import { expandHomePath } from "../pathExpansion.ts";
import { DatabaseSync } from "../provider/sqliteCompat.ts";

import { totalTokens, type UsageRecord } from "./usageTranscripts.ts";

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
  const key = `${sessionId}\${model}`;
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

/**
 * Where this environment's OpenCode database lives when the instance
 * environment names one explicitly.
 *
 * `OPENCODE_DB` is OpenCode's own override and wins. Otherwise it is the data
 * directory (`XDG_DATA_HOME` when set, `~/.local/share` elsewhere) that
 * `opencode serve` uses, on every platform.
 */
export function resolveOpenCodeDatabasePath(
  home: string,
  environment: Readonly<Record<string, string | undefined>>,
): string {
  const override = environment["OPENCODE_DB"]?.trim();
  if (override) return expandHomePath(override);
  const dataHome = environment["XDG_DATA_HOME"]?.trim();
  const base =
    dataHome && NodePath.isAbsolute(dataHome)
      ? dataHome
      : NodePath.join(home || NodeOS.homedir(), ".local", "share");
  return NodePath.join(base, "opencode", "opencode.db");
}

const DATABASE_NAME_PATTERN = /^opencode(?:-[a-zA-Z0-9_-]+)?\.db$/;

/** Reads current SQLite and pre-migration JSON stores without modifying either. */
export async function readOpenCodeUsage(
  root: string,
  sinceMs: number,
): Promise<OpenCodeUsageReadResult> {
  const files: { path: string; records: UsageRecord[] }[] = [];
  const seen = new Set<string>();
  // Kiro context histories are per session, so the estimator lives for the
  // whole read and is keyed by session and model inside.
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

  // `root` may point at one database file (an explicit OPENCODE_DB override)
  // or at a data directory to scan for `opencode*.db` files.
  let rootDir = root;
  let databases: string[] = [];
  try {
    const stat = await NodeFSP.stat(root);
    if (stat.isFile()) {
      // An explicit OPENCODE_DB path names the database itself; a wrong guess
      // fails to open below and surfaces as an error rather than silently
      // scanning the parent directory.
      rootDir = NodePath.dirname(root);
      databases = [NodePath.basename(root)];
    }
  } catch {
    // Missing root: the directory scan reports it through `missing`.
  }
  if (databases.length === 0 && rootDir === root) {
    try {
      databases = (await NodeFSP.readdir(root, { withFileTypes: true }))
        .filter((entry) => entry.isFile() && DATABASE_NAME_PATTERN.test(entry.name))
        .map((entry) => entry.name)
        .sort((a, b) => (a === "opencode.db" ? -1 : b === "opencode.db" ? 1 : a.localeCompare(b)));
    } catch (cause) {
      if (object(cause).code !== "ENOENT") error = true;
    }
  }
  for (const name of databases) {
    found = true;
    const file = { path: NodePath.join(rootDir, name), records: [] as UsageRecord[] };
    files.push(file);
    let database: { close(): void } | undefined;
    try {
      const db = new DatabaseSync(NodePath.join(rootDir, name), { readOnly: true });
      database = db;
      // A busy live provider should fail this source promptly rather than
      // stalling the server while SQLite waits for its writer.
      db.exec("PRAGMA busy_timeout = 100");
      const tables = new Set(
        db
          .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
          .all()
          .map((row: { name: string }) => row.name),
      );
      if (!tables.has("message") && !tables.has("session_message")) error = true;
      for (const table of ["message", "session_message"] as const) {
        if (!tables.has(table)) continue;
        const columns = new Set(
          db
            .prepare(`PRAGMA table_info(${table})`)
            .all()
            .map((row: { name: string }) => row.name),
        );
        const timestamp = columns.has("time_created") ? "time_created" : "NULL";
        const predicates = table === "session_message" ? ["type = 'assistant'"] : [];
        if (timestamp !== "NULL") predicates.push("time_created >= ?");
        const where = predicates.length > 0 ? ` WHERE ${predicates.join(" AND ")}` : "";
        // Rows stream per session and in `seq` order when the column exists so
        // the Kiro rolling-prefix estimate sees the same order OpenCode wrote.
        const orderBy = columns.has("session_id")
          ? columns.has("seq")
            ? " ORDER BY session_id, seq"
            : " ORDER BY session_id"
          : "";
        const statement = db.prepare(
          `SELECT id, session_id, data, ${timestamp} AS created FROM ${table}${where}${orderBy}`,
        );
        let count = 0;
        for (const row of statement.iterate(...(timestamp === "NULL" ? [] : [sinceMs]))) {
          append(
            file.records,
            parseOpenCodeMessage(text(row.data), {
              id: text(row.id),
              sessionId: text(row.session_id),
              kiroContext,
              ...(typeof row.created === "number" ? { timestampMs: row.created } : {}),
            }),
          );
          if (++count % 256 === 0) await NodeTimersPromises.setImmediate();
        }
      }
    } catch {
      error = true;
    } finally {
      database?.close();
    }
  }

  // Do not follow symlinks, including cycles. Database records win over their
  // old JSON copies when OpenCode has migrated a store in place.
  const directories = [NodePath.join(rootDir, "storage", "message")];
  while (directories.length > 0) {
    const directory = directories.pop()!;
    try {
      for (const entry of await NodeFSP.readdir(directory, { withFileTypes: true })) {
        const path = NodePath.join(directory, entry.name);
        if (entry.isDirectory()) {
          directories.push(path);
        } else if (entry.isFile() && entry.name.endsWith(".json")) {
          found = true;
          const id = entry.name.slice(0, -5);
          if (seen.has(`opencode:${id}`)) continue;
          const file = { path, records: [] as UsageRecord[] };
          files.push(file);
          try {
            append(
              file.records,
              parseOpenCodeMessage(await NodeFSP.readFile(path, "utf8"), { id }),
            );
          } catch (cause) {
            if (object(cause).code !== "ENOENT") error = true;
          }
        }
      }
    } catch (cause) {
      if (object(cause).code !== "ENOENT") error = true;
    }
  }
  return { files, missing: !found && !error, error };
}
