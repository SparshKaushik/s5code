import { describe, expect, it } from "@effect/vitest";

import { initialPiScanState, parsePiLine } from "@t3tools/provider-pi/server/usage";
import { totalTokens } from "@t3tools/provider-core/server/usage";

describe("parsePiLine", () => {
  /** Shaped after a real pi session record. */
  function piMessage(
    overrides: {
      responseId?: string | null;
      provider?: string;
      model?: string;
      costTotal?: number;
      input?: number;
      cacheRead?: number;
      cacheWrite?: number;
      output?: number;
      reasoning?: number;
    } = {},
  ): string {
    const output = overrides.output ?? 512;
    return JSON.stringify({
      type: "message",
      id: "01K9V",
      parentId: "01K9U",
      timestamp: "2026-08-03T04:13:41.221Z",
      message: {
        role: "assistant",
        api: "anthropic-messages",
        provider: overrides.provider ?? "clinepass",
        model: overrides.model ?? "cline-pass/deepseek-v4-flash",
        usage: {
          input: overrides.input ?? 12,
          output,
          cacheRead: overrides.cacheRead ?? 30_000,
          cacheWrite: overrides.cacheWrite ?? 4_000,
          cacheWrite1h: 4_000,
          reasoning: overrides.reasoning ?? 128,
          totalTokens: 12 + output + 30_000 + 4_000,
          cost: {
            input: 0.000_01,
            output: 0.000_2,
            cacheRead: 0.000_3,
            cacheWrite: 0.000_4,
            total: overrides.costTotal ?? 0.000_91,
          },
        },
        stopReason: "stop",
        timestamp: "2026-08-03T04:13:41.221Z",
        ...(overrides.responseId === null
          ? {}
          : { responseId: overrides.responseId ?? "msg_01abc" }),
      },
    });
  }

  it("extracts disjoint token totals, the gateway, and the response id", () => {
    const record = parsePiLine(piMessage(), initialPiScanState());

    expect(record).toMatchObject({
      provider: "pi",
      model: "cline-pass/deepseek-v4-flash",
      apiProvider: "clinepass",
      reportedCostUsd: 0.000_91,
      dedupeKey: "msg_01abc",
    });
    expect(record?.totals).toEqual({
      uncachedInputTokens: 12,
      cachedInputTokens: 30_000,
      cacheCreationTokens: 4_000,
      outputTokens: 512,
      reasoningTokens: 128,
    });
    expect(totalTokens(record!.totals)).toBe(34_524);
  });

  it("simulates Kiro rolling-prefix cache and marks it estimated", () => {
    const state = initialPiScanState();
    const first = parsePiLine(
      piMessage({
        provider: "kiro",
        model: "claude-opus-5",
        input: 100_000,
        cacheRead: 0,
        cacheWrite: 0,
      }),
      state,
    );
    const next = parsePiLine(
      piMessage({
        provider: "kiro",
        model: "claude-opus-5",
        input: 105_000,
        cacheRead: 0,
        cacheWrite: 0,
      }),
      state,
    );

    expect(first?.inputTokensEstimated).toBe(true);
    expect(first?.totals).toMatchObject({ uncachedInputTokens: 100_000, cachedInputTokens: 0 });
    expect(next?.inputTokensEstimated).toBe(true);
    expect(next?.totals).toMatchObject({ uncachedInputTokens: 5_000, cachedInputTokens: 100_000 });
  });

  it("treats a shrinking Kiro context as a fresh reset", () => {
    const state = initialPiScanState();
    parsePiLine(
      piMessage({
        provider: "kiro",
        model: "claude-opus-5",
        input: 100_000,
        cacheRead: 0,
        cacheWrite: 0,
      }),
      state,
    );
    const compacted = parsePiLine(
      piMessage({
        provider: "kiro",
        model: "claude-opus-5",
        input: 40_000,
        cacheRead: 0,
        cacheWrite: 0,
      }),
      state,
    );

    expect(compacted?.totals).toMatchObject({ uncachedInputTokens: 40_000, cachedInputTokens: 0 });
  });

  it("keeps provider-reported cache authoritative", () => {
    const record = parsePiLine(
      piMessage({ provider: "kiro", input: 100, cacheRead: 500, cacheWrite: 25 }),
      initialPiScanState(),
    );

    expect(record?.inputTokensEstimated).toBe(false);
    expect(record?.totals).toMatchObject({
      uncachedInputTokens: 100,
      cachedInputTokens: 500,
      cacheCreationTokens: 25,
    });
  });

  it("attributes messages to the session id from the opening line", () => {
    const state = initialPiScanState();
    const session = JSON.stringify({
      type: "session",
      version: 3,
      id: "019fc5d3-0000-7000-8000-000000000000",
      timestamp: "2026-08-03T04:12:56.548Z",
      cwd: "/home/theo/project",
    });

    expect(parsePiLine(session, state)).toBeNull();
    expect(parsePiLine(piMessage(), state)?.sessionId).toBe("019fc5d3-0000-7000-8000-000000000000");
  });

  it("treats a zero reported cost as unpriced", () => {
    const record = parsePiLine(piMessage({ costTotal: 0 }), initialPiScanState());
    expect(record?.reportedCostUsd).toBeNull();
  });

  it("skips non-assistant messages and messages with no tokens", () => {
    const userLine = JSON.stringify({
      type: "message",
      timestamp: "2026-08-03T04:13:41.221Z",
      message: { role: "user", usage: { input: 5, output: 0 } },
    });

    expect(parsePiLine(userLine, initialPiScanState())).toBeNull();
    expect(parsePiLine(piMessage({ output: 0 }), initialPiScanState())).not.toBeNull();
  });

  it("clamps reasoning to output because it is a subset", () => {
    const record = parsePiLine(piMessage({ output: 100, reasoning: 400 }), initialPiScanState());
    expect(record?.totals.reasoningTokens).toBe(100);
  });
});
