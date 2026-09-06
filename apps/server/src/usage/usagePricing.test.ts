import { describe, expect, it } from "@effect/vitest";

import {
  cacheSavingsUsd,
  createOverrideRateTable,
  lookupRate,
  parseRateTable,
  priceUsage,
  UsagePricer,
  type RateTable,
} from "./usagePricing.ts";
import { parseModelCatalog } from "./usageModelCatalog.ts";
import type { UsageRecord } from "./usageTranscripts.ts";

const rate = (input: number, cacheRead?: number) => ({
  input_cost_per_token: input,
  output_cost_per_token: input * 5,
  ...(cacheRead === undefined ? {} : { cache_read_input_token_cost: cacheRead }),
});

describe("usage pricing", () => {
  const totals = {
    uncachedInputTokens: 1_000_000,
    cachedInputTokens: 1_000_000,
    cacheCreationTokens: 1_000_000,
    outputTokens: 1_000_000,
    reasoningTokens: 500_000,
  };

  it("uses custom token rates ahead of public and provider-reported costs", () => {
    const table = parseRateTable({ "example-model": rate(1) });
    const overrides = createOverrideRateTable({
      "example-model": {
        inputCostPerMillionTokens: 2,
        outputCostPerMillionTokens: 8,
        cacheReadCostPerMillionTokens: 0.5,
        cacheWriteCostPerMillionTokens: 3,
      },
    });

    for (const reportedCostUsd of [null, 99]) {
      expect(priceUsage(table, "example-model", totals, reportedCostUsd, overrides)).toEqual({
        costUsd: 13.5,
        costSource: "modelPriced",
      });
    }
    expect(cacheSavingsUsd(table, "example-model", totals, overrides)).toBe(1.5);
  });

  it("prices unknown models offline and uses input prices for omitted cache rates", () => {
    const table = parseRateTable({});
    const overrides = createOverrideRateTable({
      "example-model": { inputCostPerMillionTokens: 2, outputCostPerMillionTokens: 8 },
    });

    expect(priceUsage(table, "example-model", totals, null, overrides)).toEqual({
      costUsd: 14,
      costSource: "modelPriced",
    });
    expect(cacheSavingsUsd(table, "example-model", totals, overrides)).toBe(0);
  });

  it("preserves explicit zero rates and matches only the exact trimmed model ID", () => {
    const table = parseRateTable({});
    const overrides = createOverrideRateTable({
      " vendor/example-model[1m] ": {
        inputCostPerMillionTokens: 0,
        outputCostPerMillionTokens: 0,
      },
    });
    expect(priceUsage(table, " vendor/example-model[1m] ", totals, 99, overrides)).toEqual({
      costUsd: 0,
      costSource: "modelPriced",
    });
    for (const model of [
      "example-model[1m]",
      "vendor/example-model",
      "vendor/Example-model[1m]",
      "other/example-model[1m]",
    ]) {
      expect(priceUsage(table, model, totals, null, overrides).costSource).toBe("unpriced");
      expect(priceUsage(table, model, totals, 99, overrides)).toEqual({
        costUsd: 99,
        costSource: "providerReported",
      });
    }
  });

  it("keeps the canonical Fable rate separate from DeepInfra in either order", () => {
    const canonical = ["claude-fable-5", rate(1e-5, 1e-6)] as const;
    const deepInfra = ["deepinfra/anthropic/claude-fable-5", rate(1e-5)] as const;

    for (const entries of [
      [canonical, deepInfra],
      [deepInfra, canonical],
    ]) {
      const table = parseRateTable(Object.fromEntries(entries));

      expect(lookupRate(table, "claude-fable-5")?.cacheReadCostPerToken).toBe(1e-6);
      expect(lookupRate(table, "deepinfra/anthropic/claude-fable-5")?.cacheReadCostPerToken).toBe(
        1e-5,
      );
      expect(lookupRate(table, "other/claude-fable-5")).toBeNull();
    }
  });

  it("prices a bracketed context-tier variant at the base model's rate", () => {
    const table = parseRateTable({ "claude-fable-5-1": rate(1e-5, 2.5e-7) });

    expect(lookupRate(table, "claude-fable-5-1[1m]")).toEqual(
      lookupRate(table, "claude-fable-5-1"),
    );
    expect(lookupRate(table, "anthropic/Claude-Fable-5-1[1m]")).toBeNull();
  });

  it("adds a bare alias when every qualified entry has the same rate", () => {
    const table = parseRateTable({
      "provider-a/example-model": rate(1),
      "provider-b/example-model": rate(1),
    });

    expect(lookupRate(table, "example-model")).toEqual(
      lookupRate(table, "provider-a/example-model"),
    );
  });

  it("leaves an ambiguous bare name unpriced", () => {
    const table = parseRateTable({
      "provider-a/example-model": rate(1),
      "provider-b/example-model": rate(3),
    });

    expect(lookupRate(table, "provider-a/example-model")?.inputCostPerToken).toBe(1);
    expect(lookupRate(table, "provider-b/example-model")?.inputCostPerToken).toBe(3);
    expect(lookupRate(table, "example-model")).toBeNull();
  });
});

const rates: RateTable = new Map([
  [
    "claude-fable-5",
    {
      inputCostPerToken: 1e-5,
      outputCostPerToken: 5e-5,
      cacheReadCostPerToken: 1e-6,
      cacheCreationCostPerToken: 1.25e-5,
    },
  ],
]);

const catalog = parseModelCatalog({
  anthropic: {
    id: "anthropic",
    name: "Anthropic",
    models: {
      "claude-opus-5": {
        name: "Claude Opus 5",
        cost: { input: 5, output: 25, cache_read: 0.5, cache_write: 6.25 },
      },
      "claude-fable-5": {
        name: "Claude Fable 5",
        // Deliberately different from the LiteLLM entry above, so a test can
        // tell which table priced a record.
        cost: { input: 999, output: 999, cache_read: 999, cache_write: 999 },
      },
    },
  },
  "cline-pass": {
    id: "cline-pass",
    name: "Cline Pass",
    models: {
      "cline-pass/deepseek-v4-flash": {
        name: "DeepSeek V4 Flash",
        cost: { input: 0.14, output: 0.28, cache_read: 0.0028 },
      },
    },
  },
  // A reseller of `claude-opus-5` at its own margin. This is what makes the
  // bare name ambiguous, exactly as it is in the real catalog, where eleven
  // providers serve it at three different prices.
  openrouter: {
    id: "openrouter",
    name: "OpenRouter",
    models: {
      "claude-opus-5": {
        name: "Claude Opus 5",
        cost: { input: 6, output: 30, cache_read: 0.6, cache_write: 7.5 },
      },
    },
  },
});

function record(overrides: Partial<UsageRecord> = {}): UsageRecord {
  return {
    provider: "claude",
    timestampMs: 1_786_000_000_000,
    model: "claude-fable-5",
    apiProvider: "",
    sessionId: "session-a",
    totals: {
      uncachedInputTokens: 100,
      cachedInputTokens: 1000,
      cacheCreationTokens: 10,
      outputTokens: 50,
      reasoningTokens: 0,
    },
    inputTokensEstimated: false,
    reportedCostUsd: null,
    dedupeKey: null,
    ...overrides,
  };
}

describe("UsagePricer", () => {
  const pricer = new UsagePricer({ rates, catalog, aliases: [] });

  it("prices Claude and Codex from LiteLLM, not models.dev", () => {
    // Both tables know `claude-fable-5`. Existing providers must keep the
    // numbers they already report, so the models.dev entry must not win.
    expect(pricer.price(record()).costUsd).toBeCloseTo(0.004625, 9);
  });

  it("prices pi from models.dev, scoped to the gateway that served it", () => {
    const priced = pricer.price(
      record({ provider: "pi", apiProvider: "clinepass", model: "cline-pass/deepseek-v4-flash" }),
    );

    // 100*1.4e-7 + 1000*2.8e-9 + 10*1.4e-7 + 50*2.8e-7
    expect(priced.costUsd).toBeCloseTo(0.0000322, 12);
    expect(priced.costSource).toBe("modelPriced");
    // Reported so the UI can show which catalog entry the guess came from.
    expect(priced.pricedAs).toBe("cline-pass/cline-pass/deepseek-v4-flash");
  });

  it("leaves a model no catalog can pin down unpriced", () => {
    const priced = pricer.price(
      record({ provider: "pi", apiProvider: "agentrouter", model: "claude-opus-5" }),
    );

    expect(priced).toEqual({ costUsd: 0, costSource: "unpriced", pricedAs: null });
  });

  it("prices simulated Kiro cache at cache-read rates", () => {
    const priced = pricer.price(
      record({
        provider: "pi",
        apiProvider: "anthropic",
        model: "claude-opus-5",
        inputTokensEstimated: true,
        totals: {
          uncachedInputTokens: 1_000,
          cachedInputTokens: 99_000,
          cacheCreationTokens: 0,
          outputTokens: 500,
          reasoningTokens: 0,
        },
      }),
    );

    // 1k fresh input + 99k simulated cache reads + real output.
    expect(priced.costUsd).toBeCloseTo(0.067, 9);
    expect(priced.costSource).toBe("modelPriced");
  });

  it("prefers a reported cost over every table", () => {
    expect(pricer.price(record({ reportedCostUsd: 1.25 }))).toEqual({
      costUsd: 1.25,
      costSource: "providerReported",
      pricedAs: null,
    });
  });
});

describe("UsagePricer tagging", () => {
  const tagged = new UsagePricer({
    rates,
    catalog,
    aliases: [
      {
        provider: "pi",
        apiProvider: "agentrouter",
        model: "claude-opus-5",
        catalogModelId: "anthropic/claude-opus-5" as never,
      },
    ],
  });

  it("prices a tagged model at the catalog entry the user chose", () => {
    const priced = tagged.price(
      record({ provider: "pi", apiProvider: "agentrouter", model: "claude-opus-5" }),
    );

    // 100*5e-6 + 1000*5e-7 + 10*6.25e-6 + 50*2.5e-5
    expect(priced.costUsd).toBeCloseTo(0.0023125, 9);
    expect(priced.costSource).toBe("userTagged");
    expect(priced.pricedAs).toBe("anthropic/claude-opus-5");
  });

  it("does not apply a tag to the same model name from a different gateway", () => {
    // Gateways resell at their own margins; tagging one must not speak for all.
    expect(
      tagged.price(record({ provider: "pi", apiProvider: "tokenrouter", model: "claude-opus-5" }))
        .costSource,
    ).toBe("unpriced");
  });

  it("still lets a reported cost win over a tag", () => {
    expect(
      tagged.price(
        record({
          provider: "pi",
          apiProvider: "agentrouter",
          model: "claude-opus-5",
          reportedCostUsd: 2,
        }),
      ).costSource,
    ).toBe("providerReported");
  });

  it("leaves a tag pointing at an unknown catalog entry unpriced, not free", () => {
    const stale = new UsagePricer({
      rates,
      catalog,
      aliases: [
        {
          provider: "pi",
          apiProvider: "agentrouter",
          model: "claude-opus-5",
          catalogModelId: "anthropic/claude-opus-9" as never,
        },
      ],
    });

    expect(
      stale.price(record({ provider: "pi", apiProvider: "agentrouter", model: "claude-opus-5" }))
        .costSource,
    ).toBe("unpriced");
  });
});

describe("UsagePricer.cacheSavingsUsd", () => {
  it("reports what the cached input would have cost at full input rates", () => {
    const pricer = new UsagePricer({ rates, catalog, aliases: [] });

    // 1000 * (1e-5 - 1e-6)
    expect(pricer.cacheSavingsUsd(record())).toBeCloseTo(0.009, 9);
  });

  it("claims no savings for a model it cannot price", () => {
    const pricer = new UsagePricer({ rates, catalog, aliases: [] });

    expect(pricer.cacheSavingsUsd(record({ model: "kimi-k3" }))).toBe(0);
  });
});
