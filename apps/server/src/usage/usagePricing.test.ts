import { describe, expect, it } from "@effect/vitest";

import { cursorRateModel } from "@t3tools/provider-cursor/server/accountUsage";
import type { UsageSpeed } from "@t3tools/provider-core/server/usage";
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
import type { UsageRecord } from "@t3tools/provider-core/server/usage";

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
  const record = (
    model: string,
    reportedCostUsd: number | null = null,
    speed: UsageSpeed = "standard",
  ) => ({
    model,
    totals,
    reportedCostUsd,
    speed,
  });

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
      expect(priceUsage(table, record("example-model", reportedCostUsd), overrides)).toMatchObject({
        costUsd: 13.5,
        costSource: "modelPriced",
      });
    }
    expect(cacheSavingsUsd(table, record("example-model"), overrides)).toBe(1.5);
  });

  it("prices Cursor cache savings at the base model rate", () => {
    const table = parseRateTable({
      "claude-fable-5-1": rate(10e-6, 1e-6),
      "xai/grok-4.7": rate(2e-6, 0.5e-6),
      "openrouter/x-ai/grok-4.7": rate(3e-6, 0.5e-6),
    });
    const cursorRecord = (model: string) => ({
      ...record(model, 0.25),
      rateModel: cursorRateModel(model),
    });

    expect(cacheSavingsUsd(table, cursorRecord("claude-fable-5-1-thinking-high"))).toBeCloseTo(9);
    expect(cacheSavingsUsd(table, cursorRecord("cursor-grok-4.7-high-fast"))).toBeCloseTo(1.5);
    expect(cacheSavingsUsd(table, cursorRecord("default"))).toBe(0);
    expect(priceUsage(table, cursorRecord("grok-4.7-xhigh-fast"))).toMatchObject({
      costUsd: 0.25,
      costSource: "providerReported",
    });
  });

  it("falls back to the display model when the rate model is unlisted", () => {
    const table = parseRateTable({ "example-model": rate(1e-6, 0.1e-6) });
    const unlisted = { ...record("example-model"), rateModel: "reseller/example-model" };

    expect(priceUsage(table, unlisted)).toEqual(priceUsage(table, record("example-model")));
    expect(cacheSavingsUsd(table, unlisted)).toBeCloseTo(0.9);
  });

  it("prices unknown models offline and uses input prices for omitted cache rates", () => {
    const table = parseRateTable({});
    const overrides = createOverrideRateTable({
      "example-model": { inputCostPerMillionTokens: 2, outputCostPerMillionTokens: 8 },
    });

    expect(priceUsage(table, record("example-model"), overrides)).toMatchObject({
      costUsd: 14,
      costSource: "modelPriced",
    });
    expect(cacheSavingsUsd(table, record("example-model"), overrides)).toBe(0);
  });

  it("preserves explicit zero rates and matches only the exact trimmed model ID", () => {
    const table = parseRateTable({});
    const overrides = createOverrideRateTable({
      " vendor/example-model[1m] ": {
        inputCostPerMillionTokens: 0,
        outputCostPerMillionTokens: 0,
      },
    });
    expect(priceUsage(table, record(" vendor/example-model[1m] ", 99), overrides)).toMatchObject({
      costUsd: 0,
      costSource: "modelPriced",
    });
    for (const model of [
      "example-model[1m]",
      "vendor/example-model",
      "vendor/Example-model[1m]",
      "other/example-model[1m]",
    ]) {
      expect(priceUsage(table, record(model), overrides).costSource).toBe("unpriced");
      expect(priceUsage(table, record(model, 99), overrides)).toEqual({
        costUsd: 99,
        costSource: "providerReported",
        categoryCostUsd: null,
        speedPremiumUsd: 0,
      });
    }
  });

  it("prices fast-mode requests at the model's published fast multiple", () => {
    const table = parseRateTable({
      "claude-opus-5-5": { ...rate(4e-6, 2e-7), provider_specific_entry: { fast: 2, us: 1.1 } },
      "claude-fable-5-1": { ...rate(1e-5, 2.5e-7), provider_specific_entry: { us: 1.1 } },
    });
    const overrides = createOverrideRateTable({
      "claude-opus-5-5": { inputCostPerMillionTokens: 4, outputCostPerMillionTokens: 20 },
    });
    const cost = (model: string, speed: UsageSpeed, custom?: typeof overrides) =>
      priceUsage(table, record(model, null, speed), custom).costUsd;

    expect(cost("claude-opus-5-5", "fast")).toBeCloseTo(2 * cost("claude-opus-5-5", "standard"));
    expect(cacheSavingsUsd(table, record("claude-opus-5-5", null, "fast"))).toBeCloseTo(
      2 * cacheSavingsUsd(table, record("claude-opus-5-5")),
    );
    expect(cost("claude-fable-5-1", "fast")).toBe(cost("claude-fable-5-1", "standard"));
    expect(cost("claude-opus-5-5", "fast", overrides)).toBe(
      cost("claude-opus-5-5", "standard", overrides),
    );
  });

  it("splits cost by category and prices the speed premium", () => {
    const table = parseRateTable({
      "claude-opus-5-5": {
        ...rate(4e-6, 4e-7),
        cache_creation_input_token_cost: 5e-6,
        provider_specific_entry: { fast: 2 },
      },
    });
    const split = (input: number, cacheRead: number, cacheWrite: number, output: number) => ({
      input: expect.closeTo(input),
      cacheRead: expect.closeTo(cacheRead),
      cacheWrite: expect.closeTo(cacheWrite),
      output: expect.closeTo(output),
    });

    expect(priceUsage(table, record("claude-opus-5-5", null, "fast"))).toEqual({
      costUsd: expect.closeTo(58.8),
      costSource: "modelPriced",
      categoryCostUsd: split(8, 0.8, 10, 40),
      speedPremiumUsd: expect.closeTo(29.4),
    });
    expect(priceUsage(table, record("claude-opus-5-5", 29.4, "fast"))).toEqual({
      costUsd: 29.4,
      costSource: "providerReported",
      categoryCostUsd: split(4, 0.4, 5, 20),
      speedPremiumUsd: expect.closeTo(14.7),
    });
    expect(priceUsage(table, record("unknown-model", 29.4, "fast"))).toMatchObject({
      costUsd: 29.4,
      categoryCostUsd: null,
      speedPremiumUsd: 0,
    });
  });

  it("prices Codex priority and ultrafast requests at their published tier rates", () => {
    const table = parseRateTable({
      "gpt-6-astra": {
        ...rate(1e-5, 1e-6),
        input_cost_per_token_priority: 2e-5,
        output_cost_per_token_priority: 1e-4,
        cache_read_input_token_cost_priority: 2e-6,
        input_cost_per_token_ultrafast: 6e-5,
        output_cost_per_token_ultrafast: 3e-4,
      },
      "gpt-6-sol": rate(2e-6, 2e-7),
    });
    const cost = (model: string, speed: UsageSpeed) =>
      priceUsage(table, record(model, null, speed)).costUsd;
    const standard = cost("gpt-6-astra", "standard");

    expect(cost("gpt-6-astra", "fast")).toBeCloseTo(2 * standard);
    expect(cost("gpt-6-astra", "ultrafast")).toBeCloseTo(6 * standard);
    expect(cacheSavingsUsd(table, record("gpt-6-astra", null, "ultrafast"))).toBeCloseTo(
      6 * cacheSavingsUsd(table, record("gpt-6-astra")),
    );
    expect(cost("gpt-6-sol", "ultrafast")).toBe(cost("gpt-6-sol", "standard"));
  });

  it("prices a request above the model's long-context threshold at its above-tier rates", () => {
    const table = parseRateTable({
      "claude-haiku-5-5": {
        input_cost_per_token: 1e-6,
        output_cost_per_token: 5e-6,
        cache_read_input_token_cost: 1e-7,
        cache_creation_input_token_cost: 1.25e-6,
        input_cost_per_token_above_100k_tokens: 2e-6,
        output_cost_per_token_above_100k_tokens: 7.5e-6,
        cache_read_input_token_cost_above_100k_tokens: 2e-7,
        cache_creation_input_token_cost_above_100k_tokens: 2.5e-6,
      },
      "gpt-6-sol": {
        ...rate(2e-6, 2e-7),
        input_cost_per_token_priority: 4e-6,
        output_cost_per_token_priority: 2e-5,
        cache_read_input_token_cost_priority: 4e-7,
        input_cost_per_token_above_272k_tokens_priority: 8e-6,
        output_cost_per_token_above_272k_tokens_priority: 3e-5,
        cache_read_input_token_cost_above_272k_tokens_priority: 8e-7,
      },
    });
    const longRequest = (
      model: string,
      speed: UsageSpeed,
      totals: { uncached: number; cacheRead: number; cacheWrite: number; output: number },
    ) => ({
      model,
      speed,
      reportedCostUsd: null,
      totals: {
        uncachedInputTokens: totals.uncached,
        cachedInputTokens: totals.cacheRead,
        cacheCreationTokens: totals.cacheWrite,
        outputTokens: totals.output,
        reasoningTokens: 0,
      },
    });

    // The Claude prompt is input + cache read + cache write: 120K, above the 100K tier.
    expect
      .soft(
        priceUsage(
          table,
          longRequest("claude-haiku-5-5", "standard", {
            uncached: 10_000,
            cacheRead: 100_000,
            cacheWrite: 10_000,
            output: 1_000,
          }),
        ).costUsd,
      )
      .toBeCloseTo(10_000 * 2e-6 + 100_000 * 2e-7 + 10_000 * 2.5e-6 + 1_000 * 7.5e-6, 12);
    expect(
      priceUsage(
        table,
        longRequest("gpt-6-sol", "fast", {
          uncached: 20_000,
          cacheRead: 280_000,
          cacheWrite: 0,
          output: 1_000,
        }),
      ).costUsd,
    ).toBeCloseTo(20_000 * 8e-6 + 280_000 * 8e-7 + 1_000 * 3e-5, 12);
  });

  it("bills the long-context tier only above its threshold, filling missing tier fields from base", () => {
    const table = parseRateTable({
      "gpt-6-sol": {
        input_cost_per_token: 2e-6,
        output_cost_per_token: 1e-5,
        cache_read_input_token_cost: 2e-7,
        input_cost_per_token_priority: 4e-6,
        output_cost_per_token_priority: 2e-5,
        input_cost_per_token_above_272k_tokens: 4e-6,
      },
    });
    const cost = (prompt: number, speed: UsageSpeed = "standard") =>
      priceUsage(table, {
        model: "gpt-6-sol",
        speed,
        reportedCostUsd: null,
        totals: {
          uncachedInputTokens: prompt - 200_000,
          cachedInputTokens: 200_000,
          cacheCreationTokens: 0,
          outputTokens: 1_000,
          reasoningTokens: 0,
        },
      }).costUsd;

    expect(cost(272_000)).toBeCloseTo(72_000 * 2e-6 + 200_000 * 2e-7 + 1_000 * 1e-5, 12);
    // Output keeps the base rate; cache reads keep the base discount relative to input.
    expect(cost(272_001)).toBeCloseTo(72_001 * 4e-6 + 200_000 * 4e-7 + 1_000 * 1e-5, 12);
    // Without a priority long-context rate, priority requests keep the priority rate.
    expect(cost(272_001, "fast")).toBeCloseTo(72_001 * 4e-6 + 200_000 * 4e-7 + 1_000 * 2e-5, 12);
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

  it("keeps a bare alias when a reseller entry omits the fast tier", () => {
    const table = parseRateTable({
      "provider-a/example-model": {
        ...rate(1),
        input_cost_per_token_priority: 2,
        output_cost_per_token_priority: 10,
        input_cost_per_token_above_272k_tokens: 2,
        input_cost_per_token_above_272k_tokens_priority: 4,
        output_cost_per_token_above_272k_tokens_priority: 20,
      },
      "provider-b/example-model": { ...rate(1), input_cost_per_token_above_272k_tokens: 2 },
      "provider-c/example-model": rate(1),
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

  it("prices a bare name at the model maker's rate when a reseller marks it up", () => {
    const reseller = { "kyma/gemini-4-argon": rate(5.4e-6) };
    const makers = {
      "gemini/gemini-4-argon": rate(2e-6, 1e-7),
      "vertex_ai/gemini-4-argon": rate(2e-6, 1e-7),
    };

    for (const document of [
      { ...reseller, ...makers },
      { ...makers, ...reseller },
    ]) {
      const table = parseRateTable(document);
      expect(lookupRate(table, "gemini-4-argon")).toEqual(
        lookupRate(table, "gemini/gemini-4-argon"),
      );
    }
  });

  it("prices a bare name listed only under Bedrock IDs at the base anthropic rate", () => {
    const bedrock = (input: number, output: number, cacheRead: number, cacheCreation: number) => ({
      input_cost_per_token: input,
      output_cost_per_token: output,
      cache_read_input_token_cost: cacheRead,
      cache_creation_input_token_cost: cacheCreation,
    });
    const base = bedrock(8e-6, 4e-5, 2e-7, 1e-5);
    const regional = bedrock(8.8e-6, 4.4e-5, 2.2e-7, 1.1e-5);
    const govCloud = bedrock(9.6e-6, 4.8e-5, 2.4e-7, 1.2e-5);
    const rows = [
      ["anthropic.claude-mythos-5-2", base],
      ["global.anthropic.claude-mythos-5-2", base],
      ["us.anthropic.claude-mythos-5-2", regional],
      ["eu.anthropic.claude-mythos-5-2", regional],
      ["us-gov.anthropic.claude-mythos-5-2", govCloud],
      ["bedrock/us-gov-east-1/anthropic.claude-mythos-5-2", govCloud],
      ["bedrock/us-gov-west-1/anthropic.claude-mythos-5-2", govCloud],
    ] as const;

    for (const entries of [rows, rows.toReversed()]) {
      const table = parseRateTable(Object.fromEntries(entries));
      expect(lookupRate(table, "claude-mythos-5-2")).toMatchObject({
        inputCostPerToken: 8e-6,
        outputCostPerToken: 4e-5,
        cacheReadCostPerToken: 2e-7,
        cacheCreationCostPerToken: 1e-5,
      });
    }
  });

  it("leaves a bare name unpriced when model makers' own entries disagree", () => {
    const table = parseRateTable({
      "openai/example-model": rate(1),
      "xai/example-model": rate(3),
      "kyma/example-model": rate(1),
    });

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
      cacheCreation1hCostPerToken: 2e-5,
      fast: null,
      ultrafast: null,
      contextTiers: [],
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
    speed: "standard",
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

    expect(priced).toMatchObject({ costUsd: 0, costSource: "unpriced", pricedAs: null });
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
    expect(pricer.price(record({ reportedCostUsd: 1.25 }))).toMatchObject({
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
