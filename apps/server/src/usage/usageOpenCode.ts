// @effect-diagnostics nodeBuiltinImport:off
/**
 * OpenCode's on-disk usage adapter.
 *
 * OpenCode V2 keeps no JSONL transcripts: every session's messages live in one
 * SQLite database (`~/.local/share/opencode/opencode.db`, overridable with
 * `OPENCODE_DB`). Assistant rows carry a JSON `data` payload with the final
 * `tokens` and `cost` for that message, which is the usage this reader reports.
 *
 * Message-level totals diverge from the `session_v2` token rollup: OpenCode
 * removes assistant messages for superseded steps (steering, retries) while
 * still counting their usage in the rollup, and nothing durable records which
 * day or model those steps belonged to. Summed across real sessions the gap is
 * a few percent of input tokens; correct per-day attribution is worth more
 * than matching a counter that lumps deleted steps into the session total.
 *
 * Like pi, OpenCode talks to gateways, so each record keeps the message's
 * `model.providerID` as `apiProvider` for provider-scoped pricing. Kiro under
 * OpenCode has the same "context instead of billed tokens" quirk pi has, so the
 * rolling-prefix cache simulation from the pi parser is reused here.
 *
 * Node/bun sqlite access stays behind `sqliteCompat`; the file is opened
 * read-only so a scan never touches a live service's WAL.
 *
 * @module usageOpenCode
 */
import * as NodePath from "node:path";

import { expandHomePath } from "../pathExpansion.ts";
import { DatabaseSync } from "../provider/sqliteCompat.ts";
import { totalTokens, type UsageRecord } from "./usageTranscripts.ts";

type OpenCodeDatabase = {
  readonly prepare: (sql: string) => {
    readonly all: (...params: unknown[]) => unknown[];
  };
  readonly close: () => void;
};

/**
 * Where this environment's OpenCode database lives.
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
    dataHome && NodePath.isAbsolute(dataHome) ? dataHome : NodePath.join(home, ".local", "share");
  return NodePath.join(base, "opencode", "opencode.db");
}

/** One `session_message` row, narrowed to the shape the parser reads. */
interface OpenCodeMessageRow {
  readonly id: string;
  readonly sessionId: string;
  readonly data: Record<string, unknown>;
}

function int(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? Math.trunc(value) : 0;
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null ? (value as Record<string, unknown>) : null;
}

function finitePositive(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : null;
}

function millis(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : null;
}

/**
 * Rolling-prefix cache simulation for Kiro's context-based input, per session.
 *
 * Same rule the pi parser applies: when `cache.read`/`cache.write` are both
 * zero and `input` is positive, `input` is a context size rather than a billed
 * token count. Only the unchanged prefix of a non-shrinking context counts as
 * cached; a smaller context means compaction or a model switch and is billed
 * fresh. Keyed by model because a mid-session switch restarts the context.
 */
interface KiroContextEstimator {
  readonly contextTokensByModel: Map<string, number>;
}

const makeKiroEstimator = (): KiroContextEstimator => ({
  contextTokensByModel: new Map(),
});

/**
 * Parses one assistant `session_message` row into a usage record.
 *
 * `tokens.input`, `tokens.cache.read`, and `tokens.cache.write` are disjoint in
 * OpenCode's accounting, matching pi. `cost` is OpenCode's own USD figure;
 * zero means it had no rate for the model, so like pi's zero-cost object it is
 * treated as "not priced" and left to our rate tables.
 */
function parseAssistantMessage(
  row: OpenCodeMessageRow,
  estimator: KiroContextEstimator,
): UsageRecord | null {
  const tokens = record(row.data["tokens"]);
  if (tokens === null) return null;

  const modelRef = record(row.data["model"]);
  const model = typeof modelRef?.["id"] === "string" ? modelRef["id"] : "";
  if (model.length === 0) return null;
  const apiProvider = typeof modelRef?.["providerID"] === "string" ? modelRef["providerID"] : "";

  const time = record(row.data["time"]);
  const timestampMs = millis(time?.["completed"]) ?? millis(time?.["created"]);
  if (timestampMs === null) return null;

  const reportedInputTokens = int(tokens["input"]);
  const cache = record(tokens["cache"]);
  const reportedCachedInputTokens = int(cache?.["read"]);
  const reportedCacheCreationTokens = int(cache?.["write"]);
  const outputTokens = int(tokens["output"]);

  const inputTokensEstimated =
    apiProvider === "kiro" &&
    reportedInputTokens > 0 &&
    reportedCachedInputTokens === 0 &&
    reportedCacheCreationTokens === 0;

  let uncachedInputTokens = reportedInputTokens;
  let cachedInputTokens = reportedCachedInputTokens;
  if (inputTokensEstimated) {
    const previousContextTokens = estimator.contextTokensByModel.get(model) ?? 0;
    cachedInputTokens = reportedInputTokens >= previousContextTokens ? previousContextTokens : 0;
    uncachedInputTokens = reportedInputTokens - cachedInputTokens;
    estimator.contextTokensByModel.set(model, reportedInputTokens);
  }

  const totals = {
    uncachedInputTokens,
    cachedInputTokens,
    cacheCreationTokens: reportedCacheCreationTokens,
    outputTokens,
    reasoningTokens: Math.min(outputTokens, int(tokens["reasoning"])),
  };
  if (totalTokens(totals) === 0) return null;

  return {
    provider: "opencode",
    timestampMs,
    model,
    apiProvider,
    sessionId: row.sessionId,
    totals,
    inputTokensEstimated,
    reportedCostUsd: finitePositive(row.data["cost"]),
    // Message ids survive a session fork; the same id in two sessions is the
    // same message billed once.
    dedupeKey: row.id,
  };
}

export interface OpenCodeUsageRead {
  /** Parsed assistant-message records. */
  readonly records: readonly UsageRecord[];
  /** Assistant rows whose payload could not be read at all. */
  readonly malformedRecords: number;
}

/**
 * Reads assistant-message usage out of `opencode.db`.
 *
 * Sessions the window touched are read whole, in `seq` order, because the
 * Kiro cache simulation needs the session's full context history to be right
 * for in-window records; the aggregator still drops the out-of-window records
 * a slack margin admits. Returns `null` when the database cannot be opened,
 * which the caller reports as a failed source rather than an empty one.
 *
 * Assistant rows without a `tokens` payload (errors, interrupted messages)
 * are skipped, not malformed: they never had usage to report.
 */
export function readOpenCodeUsage(
  databasePath: string,
  sinceMs: number,
  untilMs: number,
): OpenCodeUsageRead | null {
  let database: OpenCodeDatabase | null = null;
  try {
    database = new DatabaseSync(databasePath, { readOnly: true }) as OpenCodeDatabase;

    const sessionRows = database
      .prepare(
        `SELECT DISTINCT session_id FROM session_message
         WHERE time_updated >= ? AND time_updated < ?`,
      )
      .all(sinceMs, untilMs) as ReadonlyArray<{ readonly session_id?: unknown }>;
    const sessionIds = sessionRows
      .map((row) => row.session_id)
      .filter((id): id is string => typeof id === "string");

    const records: UsageRecord[] = [];
    let malformedRecords = 0;
    const statement = database.prepare(
      `SELECT id, session_id, data FROM session_message
       WHERE session_id = ? AND type = 'assistant' ORDER BY seq`,
    );
    for (const sessionId of sessionIds) {
      const estimator = makeKiroEstimator();
      for (const raw of statement.all(sessionId)) {
        const row = raw as { readonly id?: unknown; readonly data?: unknown };
        if (typeof row.id !== "string" || typeof row.data !== "string") {
          malformedRecords += 1;
          continue;
        }
        let data: unknown;
        try {
          data = JSON.parse(row.data);
        } catch {
          malformedRecords += 1;
          continue;
        }
        const parsed = record(data);
        if (parsed === null) {
          malformedRecords += 1;
          continue;
        }
        const usage = parseAssistantMessage({ id: row.id, sessionId, data: parsed }, estimator);
        if (usage === null) continue;
        records.push(usage);
      }
    }

    return { records, malformedRecords };
  } catch {
    return null;
  } finally {
    try {
      database?.close();
    } catch {
      // A failed close leaves nothing a read-only handle can corrupt.
    }
  }
}
