// @effect-diagnostics nodeBuiltinImport:off
/**
 * Cursor's account-wide usage adapter.
 *
 * Unlike the other providers, Cursor does not persist token usage in local
 * transcripts. The signed-in desktop app stores an access token in its VS Code
 * global-state database; that token derives the first-party web session used by
 * Cursor's dashboard API.
 *
 * Network execution stays in `UsageService`. This module owns credential
 * resolution plus the pure response parsing and pagination reconciliation.
 *
 * @module usageCursor
 */
import * as NodeCrypto from "node:crypto";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";

import * as Option from "effect/Option";
import * as Schema from "effect/Schema";

import { totalTokens, type UsageRecord } from "./usageTranscripts.ts";

type CursorAuthDatabase = {
  readonly prepare: (sql: string) => {
    readonly get: () => unknown;
  };
  readonly close: () => void;
};

export const CURSOR_USAGE_URL = "https://cursor.com/api/dashboard/get-filtered-usage-events";
export const CURSOR_USAGE_PAGE_SIZE = 1_000;
export const CURSOR_USAGE_MAX_PAGES = 200;

const FlexibleNumber = Schema.Union([Schema.Number, Schema.NumberFromString]);
const FlexibleString = Schema.Union([Schema.String, Schema.Number]);

const CursorUsagePageDocument = Schema.Struct({
  totalUsageEventsCount: Schema.optionalKey(FlexibleNumber),
  usageEventsDisplay: Schema.Array(Schema.Unknown),
});

const CursorTokenUsageDocument = Schema.Struct({
  inputTokens: Schema.optionalKey(FlexibleNumber),
  outputTokens: Schema.optionalKey(FlexibleNumber),
  cacheWriteTokens: Schema.optionalKey(FlexibleNumber),
  cacheReadTokens: Schema.optionalKey(FlexibleNumber),
  totalCents: Schema.optionalKey(FlexibleNumber),
});

const CursorUsageEventDocument = Schema.Struct({
  timestamp: FlexibleNumber,
  model: Schema.optionalKey(Schema.String),
  conversationId: Schema.optionalKey(FlexibleString),
  tokenUsage: Schema.optionalKey(CursorTokenUsageDocument),
});

const CursorJwtPayload = Schema.Struct({
  sub: Schema.String,
  exp: Schema.Number,
});

const decodeCursorPageDocument = Schema.decodeUnknownOption(CursorUsagePageDocument);
const decodeCursorUsageEvent = Schema.decodeUnknownOption(CursorUsageEventDocument);
const decodeCursorJwtPayload = Schema.decodeUnknownOption(CursorJwtPayload);

export interface CursorAppSession {
  readonly cookieHeader: string;
  /** Stable, non-secret account identity used to deduplicate account-wide data. */
  readonly accountFingerprint: string;
  readonly databasePath: string;
}

export function resolveCursorAuthDatabasePath(
  platform: NodeJS.Platform,
  home = NodeOS.homedir(),
  environment: Readonly<Record<string, string | undefined>> = process.env,
): string {
  if (platform === "darwin") {
    return NodePath.posix.join(
      home,
      "Library",
      "Application Support",
      "Cursor",
      "User",
      "globalStorage",
      "state.vscdb",
    );
  }

  if (platform === "win32") {
    const roamingAppData = environment["APPDATA"]?.trim();
    const base =
      roamingAppData && NodePath.win32.isAbsolute(roamingAppData)
        ? roamingAppData
        : NodePath.win32.join(home, "AppData", "Roaming");
    return NodePath.win32.join(base, "Cursor", "User", "globalStorage", "state.vscdb");
  }

  const configHome = environment["XDG_CONFIG_HOME"]?.trim();
  const base =
    configHome && NodePath.posix.isAbsolute(configHome)
      ? configHome
      : NodePath.posix.join(home, ".config");
  return NodePath.posix.join(base, "Cursor", "User", "globalStorage", "state.vscdb");
}

export async function readCursorAppSession(
  nowMs: number,
  databasePath: string,
): Promise<CursorAppSession | null> {
  let database: CursorAuthDatabase | null = null;
  try {
    const { openCursorAuthDatabase } = await import("./usageCursorNodeSqlite.ts");
    database = openCursorAuthDatabase(databasePath);
    const row = database
      .prepare("SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken' LIMIT 1")
      .get() as { readonly value?: unknown } | undefined;
    const accessToken = decodeSqliteString(row?.value)?.trim();
    if (!accessToken) return null;

    const parts = accessToken.split(".");
    const encodedPayload = parts[1];
    if (!encodedPayload) return null;

    const payloadDocument = JSON.parse(Buffer.from(encodedPayload, "base64url").toString("utf8"));
    const payload = Option.getOrNull(decodeCursorJwtPayload(payloadDocument));
    if (
      payload === null ||
      !Number.isFinite(payload.exp) ||
      payload.exp * 1_000 <= nowMs + 60_000
    ) {
      return null;
    }

    const userId = payload.sub.split("|").at(-1);
    if (!userId || !/^[A-Za-z0-9._-]+$/.test(userId)) return null;

    return {
      cookieHeader: `WorkosCursorSessionToken=${userId}%3A%3A${accessToken}`,
      accountFingerprint: NodeCrypto.createHash("sha256").update(userId).digest("hex"),
      databasePath,
    };
  } catch {
    return null;
  } finally {
    database?.close();
  }
}

function decodeSqliteString(value: unknown): string | null {
  if (typeof value === "string") return value;
  if (value instanceof Uint8Array) return Buffer.from(value).toString("utf8");
  return null;
}

export interface CursorUsagePage {
  readonly totalUsageEventsCount: number | null;
  readonly events: readonly CursorRawUsageEvent[];
}

export interface CursorRawUsageEvent {
  readonly document: unknown;
  readonly identity: string;
}

/** Validates a dashboard page while retaining exact raw events for boundary overlap checks. */
export function parseCursorUsagePage(document: unknown): CursorUsagePage | null {
  const decoded = Option.getOrNull(decodeCursorPageDocument(document));
  if (decoded === null) return null;

  const total = decoded.totalUsageEventsCount;
  if (total !== undefined && (!Number.isSafeInteger(total) || total < 0)) {
    return null;
  }

  return {
    totalUsageEventsCount: total ?? null,
    events: decoded.usageEventsDisplay.map((event) => ({
      document: event,
      identity: JSON.stringify(event),
    })),
  };
}

export class CursorUsagePaginationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "CursorUsagePaginationError";
  }
}

/**
 * Reconciles Cursor's page-boundary overlap against its authoritative count.
 * Equal legitimate rows are preserved unless the count proves an overlap must
 * be removed, and only adjacent boundaries are candidates.
 */
export function reconcileCursorUsagePages(
  pages: readonly CursorUsagePage[],
  completed: boolean,
): readonly CursorRawUsageEvent[] {
  const rawEvents = pages.flatMap((page) => page.events);
  if (!completed) {
    throw new CursorUsagePaginationError(
      `Cursor pagination reached its safety limit after ${rawEvents.length} events.`,
    );
  }

  let expectedTotal: number | null = null;
  for (const page of pages) {
    const pageTotal = page.totalUsageEventsCount;
    if (pageTotal === null) continue;
    if (expectedTotal !== null && expectedTotal !== pageTotal) {
      throw new CursorUsagePaginationError("Cursor changed its usage event count between pages.");
    }
    expectedTotal = pageTotal;
  }

  if (expectedTotal === null) return rawEvents;
  if (rawEvents.length < expectedTotal) {
    throw new CursorUsagePaginationError(
      `Cursor returned ${rawEvents.length} of ${expectedTotal} usage events.`,
    );
  }
  if (rawEvents.length === expectedTotal) return rawEvents;

  let removalsRemaining = rawEvents.length - expectedTotal;
  const reconciled: CursorRawUsageEvent[] = [...(pages[0]?.events ?? [])];
  for (let index = 1; index < pages.length; index += 1) {
    const previous = pages[index - 1]?.events ?? [];
    const current = pages[index]?.events ?? [];
    const overlap = boundaryOverlap(previous, current);
    const removalCount = Math.min(overlap, removalsRemaining);
    reconciled.push(...current.slice(removalCount));
    removalsRemaining -= removalCount;
  }

  if (removalsRemaining !== 0 || reconciled.length !== expectedTotal) {
    throw new CursorUsagePaginationError(
      `Cursor reported ${expectedTotal} events but returned ${rawEvents.length} without matching page overlap.`,
    );
  }
  return reconciled;
}

function boundaryOverlap(
  previous: readonly CursorRawUsageEvent[],
  current: readonly CursorRawUsageEvent[],
): number {
  const limit = Math.min(previous.length, current.length);
  for (let count = limit; count >= 1; count -= 1) {
    let matches = true;
    for (let offset = 0; offset < count; offset += 1) {
      if (previous[previous.length - count + offset]?.identity !== current[offset]?.identity) {
        matches = false;
        break;
      }
    }
    if (matches) return count;
  }
  return 0;
}

export type CursorEventParseResult =
  | { readonly _tag: "record"; readonly record: UsageRecord }
  | { readonly _tag: "ignored" }
  | { readonly _tag: "malformed" };

/** Maps one validated Cursor event into the shared record model. */
export function parseCursorUsageEvent(event: CursorRawUsageEvent): CursorEventParseResult {
  const decoded = Option.getOrNull(decodeCursorUsageEvent(event.document));
  if (decoded === null || !Number.isSafeInteger(decoded.timestamp) || decoded.timestamp <= 0) {
    return { _tag: "malformed" };
  }
  if (decoded.tokenUsage === undefined) return { _tag: "ignored" };

  const uncached = nonNegativeInt(decoded.tokenUsage.inputTokens);
  const cached = nonNegativeInt(decoded.tokenUsage.cacheReadTokens);
  const cacheCreation = nonNegativeInt(decoded.tokenUsage.cacheWriteTokens);
  const output = nonNegativeInt(decoded.tokenUsage.outputTokens);
  if (uncached === null || cached === null || cacheCreation === null || output === null) {
    return { _tag: "malformed" };
  }

  const reportedCostUsd = decoded.tokenUsage.totalCents;
  if (reportedCostUsd !== undefined && (!Number.isFinite(reportedCostUsd) || reportedCostUsd < 0)) {
    return { _tag: "malformed" };
  }

  const totals = {
    uncachedInputTokens: uncached,
    cachedInputTokens: cached,
    cacheCreationTokens: cacheCreation,
    outputTokens: output,
    // Cursor's dashboard does not split reasoning out of output.
    reasoningTokens: 0,
  };
  if (totalTokens(totals) === 0) return { _tag: "ignored" };

  const model = decoded.model?.trim() || "unknown";
  return {
    _tag: "record",
    record: {
      provider: "cursor",
      timestampMs: decoded.timestamp,
      model,
      apiProvider: "",
      sessionId: decoded.conversationId === undefined ? "" : String(decoded.conversationId),
      totals,
      // Cursor reports API-equivalent list cost in cents for each event.
      inputTokensEstimated: false,
      reportedCostUsd: reportedCostUsd === undefined ? null : reportedCostUsd / 100,
      // The endpoint exposes no event id. Pagination overlap is reconciled
      // before parsing; equal rows included in the authoritative count remain
      // distinct requests.
      dedupeKey: null,
    },
  };
}

function nonNegativeInt(value: number | undefined): number | null {
  if (value === undefined) return 0;
  return Number.isSafeInteger(value) && value >= 0 ? value : null;
}
