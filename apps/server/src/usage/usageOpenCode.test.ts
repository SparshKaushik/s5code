// @effect-diagnostics nodeBuiltinImport:off - seeds real opencode-shaped
// databases on disk, mirroring what the reader does in production.
import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";

import { afterEach, assert, beforeEach, describe, expect, it } from "@effect/vitest";

import { DatabaseSync } from "../provider/sqliteCompat.ts";
import { readOpenCodeUsage, resolveOpenCodeDatabasePath } from "./usageOpenCode.ts";

let dir: string;

beforeEach(async () => {
  dir = await NodeFSP.mkdtemp(NodePath.join(NodeOS.tmpdir(), "usage-opencode-test-"));
});

afterEach(async () => {
  await NodeFSP.rm(dir, { recursive: true, force: true });
});

function assistantData(overrides: Record<string, unknown> = {}): string {
  return JSON.stringify({
    time: { created: 1_789_000_000_000, completed: 1_789_000_001_000 },
    agent: "build",
    model: { id: "claude-sonnet-5", providerID: "anthropic", variant: "default" },
    finish: "stop",
    cost: 0,
    tokens: { input: 100, output: 20, reasoning: 0, cache: { read: 0, write: 0 } },
    ...overrides,
  });
}

function seedDatabase(
  dbPath: string,
  rows: ReadonlyArray<{
    id: string;
    sessionId: string;
    type?: string;
    seq: number;
    timeUpdated?: number;
    data: string;
  }>,
): void {
  const db = new DatabaseSync(dbPath);
  try {
    db.exec(
      `CREATE TABLE session_message (
        id TEXT PRIMARY KEY,
        session_id TEXT NOT NULL,
        type TEXT NOT NULL,
        seq INTEGER NOT NULL,
        time_created INTEGER NOT NULL,
        time_updated INTEGER NOT NULL,
        data TEXT NOT NULL
      )`,
    );
    const insert = db.prepare(
      `INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
       VALUES (?, ?, ?, ?, ?, ?, ?)`,
    );
    for (const row of rows) {
      insert.run(
        row.id,
        row.sessionId,
        row.type ?? "assistant",
        row.seq,
        row.timeUpdated ?? 1_789_000_000_000,
        row.timeUpdated ?? 1_789_000_001_000,
        row.data,
      );
    }
  } finally {
    db.close();
  }
}

describe("resolveOpenCodeDatabasePath", () => {
  it("honours OPENCODE_DB, then XDG_DATA_HOME, then ~/.local/share", () => {
    expect(resolveOpenCodeDatabasePath("/home/u", { OPENCODE_DB: "/custom/opencode.db" })).toBe(
      "/custom/opencode.db",
    );
    expect(resolveOpenCodeDatabasePath("/home/u", { XDG_DATA_HOME: "/data" })).toBe(
      NodePath.join("/data", "opencode", "opencode.db"),
    );
    expect(resolveOpenCodeDatabasePath("/home/u", {})).toBe(
      NodePath.join("/home/u", ".local", "share", "opencode", "opencode.db"),
    );
    // A relative XDG_DATA_HOME is ignored, matching the XDG spec.
    expect(resolveOpenCodeDatabasePath("/home/u", { XDG_DATA_HOME: "data" })).toBe(
      NodePath.join("/home/u", ".local", "share", "opencode", "opencode.db"),
    );
  });
});

describe("readOpenCodeUsage", () => {
  const SINCE = 1_789_000_000_000 - 36 * 60 * 60 * 1000;
  const UNTIL = 1_789_000_000_000 + 24 * 60 * 60 * 1000;

  it("parses assistant token payloads into usage records", () => {
    const dbPath = NodePath.join(dir, "opencode.db");
    seedDatabase(dbPath, [
      {
        id: "msg_1",
        sessionId: "ses_a",
        seq: 1,
        data: assistantData({
          tokens: { input: 100, output: 20, reasoning: 5, cache: { read: 40, write: 10 } },
          cost: 0.0125,
        }),
      },
      {
        id: "msg_2",
        sessionId: "ses_a",
        seq: 2,
        data: assistantData({
          model: { id: "gpt-5.5", providerID: "openai", variant: "default" },
          tokens: { input: 50, output: 10, reasoning: 0, cache: { read: 0, write: 0 } },
        }),
      },
    ]);

    const read = readOpenCodeUsage(dbPath, SINCE, UNTIL);
    assert.isNotNull(read);
    expect(read!.records).toHaveLength(2);

    const first = read!.records[0]!;
    expect(first.provider).toBe("opencode");
    expect(first.timestampMs).toBe(1_789_000_001_000);
    expect(first.model).toBe("claude-sonnet-5");
    expect(first.apiProvider).toBe("anthropic");
    expect(first.sessionId).toBe("ses_a");
    expect(first.dedupeKey).toBe("msg_1");
    expect(first.reportedCostUsd).toBe(0.0125);
    expect(first.totals).toEqual({
      uncachedInputTokens: 100,
      cachedInputTokens: 40,
      cacheCreationTokens: 10,
      outputTokens: 20,
      reasoningTokens: 5,
    });

    // A zero cost is "OpenCode had no rate", not a real $0.
    expect(read!.records[1]!.reportedCostUsd).toBeNull();
    expect(read!.malformedRecords).toBe(0);
  });

  it("skips non-assistant rows and assistant rows without tokens without counting them malformed", () => {
    const dbPath = NodePath.join(dir, "opencode.db");
    seedDatabase(dbPath, [
      { id: "msg_u", sessionId: "ses_a", type: "user", seq: 1, data: "{}" },
      {
        id: "msg_err",
        sessionId: "ses_a",
        seq: 2,
        data: assistantData({ tokens: undefined, finish: "error" }),
      },
      { id: "msg_3", sessionId: "ses_a", seq: 3, data: assistantData() },
    ]);

    const read = readOpenCodeUsage(dbPath, SINCE, UNTIL);
    assert.isNotNull(read);
    expect(read!.records).toHaveLength(1);
    expect(read!.malformedRecords).toBe(0);
  });

  it("counts unparseable payloads as malformed", () => {
    const dbPath = NodePath.join(dir, "opencode.db");
    seedDatabase(dbPath, [
      { id: "msg_bad", sessionId: "ses_a", seq: 1, data: "not json" },
      { id: "msg_4", sessionId: "ses_a", seq: 2, data: assistantData() },
    ]);

    const read = readOpenCodeUsage(dbPath, SINCE, UNTIL);
    assert.isNotNull(read);
    expect(read!.records).toHaveLength(1);
    expect(read!.malformedRecords).toBe(1);
  });

  it("simulates a rolling-prefix cache for Kiro context-style input, in seq order", () => {
    const dbPath = NodePath.join(dir, "opencode.db");
    const kiroData = (input: number, created: number) =>
      assistantData({
        model: { id: "claude-sonnet-5", providerID: "kiro", variant: "default" },
        time: { created, completed: created + 1 },
        tokens: { input, output: 10, reasoning: 0, cache: { read: 0, write: 0 } },
      });
    seedDatabase(dbPath, [
      { id: "msg_k1", sessionId: "ses_k", seq: 1, data: kiroData(1_000, 1_789_000_000_000) },
      { id: "msg_k2", sessionId: "ses_k", seq: 2, data: kiroData(1_800, 1_789_000_100_000) },
      // Compaction shrinks the context: the whole new input bills as fresh.
      { id: "msg_k3", sessionId: "ses_k", seq: 3, data: kiroData(600, 1_789_000_200_000) },
    ]);

    const read = readOpenCodeUsage(dbPath, SINCE, UNTIL);
    assert.isNotNull(read);
    const splits = read!.records.map((record) => ({
      uncached: record.totals.uncachedInputTokens,
      cached: record.totals.cachedInputTokens,
      estimated: record.inputTokensEstimated,
    }));
    expect(splits).toEqual([
      { uncached: 1_000, cached: 0, estimated: true },
      { uncached: 800, cached: 1_000, estimated: true },
      { uncached: 600, cached: 0, estimated: true },
    ]);
  });

  it("only reads sessions the window touched", () => {
    const dbPath = NodePath.join(dir, "opencode.db");
    seedDatabase(dbPath, [
      { id: "msg_new", sessionId: "ses_new", seq: 1, data: assistantData() },
      {
        id: "msg_old",
        sessionId: "ses_old",
        seq: 1,
        timeUpdated: 1_000_000,
        data: assistantData({ time: { created: 1_000_000, completed: 1_000_000 } }),
      },
    ]);

    const read = readOpenCodeUsage(dbPath, SINCE, UNTIL);
    assert.isNotNull(read);
    expect(read!.records.map((record) => record.sessionId)).toEqual(["ses_new"]);
  });

  it("returns null when the database cannot be opened", () => {
    expect(readOpenCodeUsage(NodePath.join(dir, "missing.db"), SINCE, UNTIL)).toBeNull();
  });
});
