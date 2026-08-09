import { describe, expect, it } from "@effect/vitest";

import {
  CursorUsagePaginationError,
  parseCursorUsageEvent,
  parseCursorUsagePage,
  reconcileCursorUsagePages,
  resolveCursorAuthDatabasePath,
  type CursorUsagePage,
} from "./usageCursor.ts";

function event(overrides: Record<string, unknown> = {}): unknown {
  return {
    timestamp: "1786000000000",
    model: "composer-2",
    conversationId: "conversation-a",
    tokenUsage: {
      inputTokens: 100,
      outputTokens: 20,
      cacheReadTokens: 300,
      cacheWriteTokens: 10,
      totalCents: 2.5,
    },
    ...overrides,
  };
}

function page(total: number, events: readonly unknown[]): CursorUsagePage {
  const parsed = parseCursorUsagePage({
    totalUsageEventsCount: total,
    usageEventsDisplay: events,
  });
  if (parsed === null) throw new Error("Invalid test page");
  return parsed;
}

describe("parseCursorUsageEvent", () => {
  it("maps Cursor's disjoint token fields and API-equivalent cost", () => {
    const source = page(1, [event()]).events[0]!;
    const parsed = parseCursorUsageEvent(source);

    expect(parsed._tag).toBe("record");
    if (parsed._tag !== "record") return;
    expect(parsed.record).toMatchObject({
      provider: "cursor",
      timestampMs: 1_786_000_000_000,
      model: "composer-2",
      sessionId: "conversation-a",
      reportedCostUsd: 0.025,
      dedupeKey: null,
    });
    expect(parsed.record.totals).toEqual({
      uncachedInputTokens: 100,
      cachedInputTokens: 300,
      cacheCreationTokens: 10,
      outputTokens: 20,
      reasoningTokens: 0,
    });
  });

  it("accepts missing cache fields and falls back to an unknown model", () => {
    const source = page(1, [
      event({
        model: "",
        tokenUsage: { inputTokens: "10", outputTokens: 2, totalCents: "0.5" },
      }),
    ]).events[0]!;
    const parsed = parseCursorUsageEvent(source);

    expect(parsed._tag).toBe("record");
    if (parsed._tag !== "record") return;
    expect(parsed.record.model).toBe("unknown");
    expect(parsed.record.totals.cachedInputTokens).toBe(0);
    expect(parsed.record.totals.cacheCreationTokens).toBe(0);
    expect(parsed.record.reportedCostUsd).toBe(0.005);
  });

  it("ignores metered-only and zero-token events", () => {
    const meteredOnlyDocument = event({ chargedCents: 4 }) as Record<string, unknown>;
    delete meteredOnlyDocument["tokenUsage"];
    const meteredOnly = page(1, [meteredOnlyDocument]).events[0]!;
    const empty = page(1, [
      event({
        tokenUsage: {
          inputTokens: 0,
          outputTokens: 0,
          cacheReadTokens: 0,
          cacheWriteTokens: 0,
          totalCents: 0,
        },
      }),
    ]).events[0]!;

    expect(parseCursorUsageEvent(meteredOnly)._tag).toBe("ignored");
    expect(parseCursorUsageEvent(empty)._tag).toBe("ignored");
  });

  it("marks invalid timestamps and negative token counts malformed", () => {
    const invalidTime = page(1, [event({ timestamp: "nope" })]).events[0]!;
    const invalidTokens = page(1, [event({ tokenUsage: { inputTokens: -1, outputTokens: 2 } })])
      .events[0]!;

    expect(parseCursorUsageEvent(invalidTime)._tag).toBe("malformed");
    expect(parseCursorUsageEvent(invalidTokens)._tag).toBe("malformed");
  });
});

describe("Cursor pagination", () => {
  const first = event({ timestamp: "1786000000001" });
  const second = event({ timestamp: "1786000000002" });
  const third = event({ timestamp: "1786000000003" });

  it("removes only overlap proven by the authoritative event count", () => {
    const events = reconcileCursorUsagePages(
      [page(3, [first, second]), page(3, [second, third]), page(3, [])],
      true,
    );

    expect(events).toHaveLength(3);
    expect(events.map((item) => JSON.parse(item.identity).timestamp)).toEqual([
      "1786000000001",
      "1786000000002",
      "1786000000003",
    ]);
  });

  it("preserves identical legitimate rows when the count includes both", () => {
    expect(
      reconcileCursorUsagePages([page(2, [first]), page(2, [first]), page(2, [])], true),
    ).toHaveLength(2);
  });

  it("fails closed on truncation, inconsistent counts, or a safety cap", () => {
    expect(() => reconcileCursorUsagePages([page(2, [first])], true)).toThrow(
      CursorUsagePaginationError,
    );
    expect(() => reconcileCursorUsagePages([page(1, [first]), page(2, [second])], true)).toThrow(
      CursorUsagePaginationError,
    );
    expect(() => reconcileCursorUsagePages([page(1, [first])], false)).toThrow(
      CursorUsagePaginationError,
    );
  });

  it("rejects malformed page counts", () => {
    expect(parseCursorUsagePage({ totalUsageEventsCount: -1, usageEventsDisplay: [] })).toBeNull();
    expect(parseCursorUsagePage({ usageEventsDisplay: "not-an-array" })).toBeNull();
  });
});

describe("resolveCursorAuthDatabasePath", () => {
  it("uses the macOS application support directory", () => {
    expect(resolveCursorAuthDatabasePath("darwin", "/Users/test", {})).toBe(
      "/Users/test/Library/Application Support/Cursor/User/globalStorage/state.vscdb",
    );
  });

  it("uses APPDATA only when it is absolute on Windows", () => {
    expect(
      resolveCursorAuthDatabasePath("win32", "C:\\Users\\test", {
        APPDATA: "D:\\Roaming",
      }),
    ).toBe("D:\\Roaming\\Cursor\\User\\globalStorage\\state.vscdb");
    expect(
      resolveCursorAuthDatabasePath("win32", "C:\\Users\\test", {
        APPDATA: "relative",
      }),
    ).toBe("C:\\Users\\test\\AppData\\Roaming\\Cursor\\User\\globalStorage\\state.vscdb");
  });

  it("uses XDG_CONFIG_HOME only when it is absolute on Linux", () => {
    expect(
      resolveCursorAuthDatabasePath("linux", "/home/test", { XDG_CONFIG_HOME: "/config" }),
    ).toBe("/config/Cursor/User/globalStorage/state.vscdb");
    expect(
      resolveCursorAuthDatabasePath("linux", "/home/test", { XDG_CONFIG_HOME: "relative" }),
    ).toBe("/home/test/.config/Cursor/User/globalStorage/state.vscdb");
  });
});
