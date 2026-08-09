import { describe, expect, it } from "@effect/vitest";

import {
  parseModelCatalog,
  resolveCatalogModel,
  resolveModelRate,
  searchCatalog,
} from "./usageModelCatalog.ts";

/** Shaped after models.dev's api.json, whose costs are USD per million tokens. */
const document = {
  anthropic: {
    id: "anthropic",
    name: "Anthropic",
    models: {
      "claude-opus-5": {
        id: "claude-opus-5",
        name: "Claude Opus 5",
        cost: { input: 5, output: 25, cache_read: 0.5, cache_write: 6.25 },
      },
    },
  },
  "cline-pass": {
    id: "cline-pass",
    name: "Cline Pass",
    models: {
      "cline-pass/deepseek-v4-flash": {
        id: "cline-pass/deepseek-v4-flash",
        name: "DeepSeek V4 Flash",
        cost: { input: 0.14, output: 0.28, cache_read: 0.0028 },
      },
    },
  },
  zenmux: {
    id: "zenmux",
    name: "ZenMux",
    models: {
      "moonshotai/kimi-k3-free": {
        id: "moonshotai/kimi-k3-free",
        name: "Kimi K3 Free",
        cost: { input: 0, output: 0, cache_read: 0 },
      },
      "claude-opus-5": {
        id: "claude-opus-5",
        name: "Claude Opus 5",
        cost: { input: 6, output: 30, cache_read: 0.6, cache_write: 7.5 },
      },
    },
  },
  broken: {
    id: "broken",
    name: "Broken",
    models: {
      "no-price": { id: "no-price", name: "No Price", cost: { input: 1 } },
    },
  },
};

const catalog = parseModelCatalog(document);

describe("parseModelCatalog", () => {
  it("converts per-million costs to per-token and keys by provider", () => {
    expect(catalog.byId.get("anthropic/claude-opus-5")).toEqual({
      id: "anthropic/claude-opus-5",
      providerName: "Anthropic",
      modelName: "Claude Opus 5",
      rate: {
        inputCostPerToken: 5e-6,
        outputCostPerToken: 2.5e-5,
        cacheReadCostPerToken: 5e-7,
        cacheCreationCostPerToken: 6.25e-6,
      },
    });
  });

  it("falls back to the input rate when a cache write cost is absent", () => {
    // Absent is not free: providers that omit cache_write bill writes at the
    // uncached input rate.
    expect(catalog.byId.get("cline-pass/cline-pass/deepseek-v4-flash")?.rate).toMatchObject({
      cacheCreationCostPerToken: 1.4e-7,
      cacheReadCostPerToken: 2.8e-9,
    });
  });

  it("keeps an explicit zero cost as free", () => {
    expect(catalog.byId.get("zenmux/moonshotai/kimi-k3-free")?.rate).toEqual({
      inputCostPerToken: 0,
      outputCostPerToken: 0,
      cacheReadCostPerToken: 0,
      cacheCreationCostPerToken: 0,
    });
  });

  it("drops models with no usable price", () => {
    expect(catalog.byId.get("broken/no-price")).toBeUndefined();
  });
});

describe("resolveModelRate", () => {
  it("matches a gateway whose pi provider id drops the hyphen", () => {
    // pi reports `clinepass`; models.dev calls the same gateway `cline-pass`.
    expect(resolveModelRate(catalog, "clinepass", "cline-pass/deepseek-v4-flash")).toMatchObject({
      id: "cline-pass/cline-pass/deepseek-v4-flash",
      rate: { inputCostPerToken: 1.4e-7 },
    });
  });

  it("resolves a bare model key when every provider agrees on the price", () => {
    expect(resolveModelRate(catalog, "tokenrouter", "moonshotai/kimi-k3-free")?.rate).toMatchObject(
      { inputCostPerToken: 0 },
    );
  });

  it("refuses to guess when providers disagree on the price", () => {
    // `claude-opus-5` costs $5/M from Anthropic and $6/M from ZenMux, and the
    // reseller pi actually used is neither. Unpriced keeps the model taggable
    // instead of quietly attributing a number that is wrong either way.
    expect(resolveModelRate(catalog, "agentrouter", "claude-opus-5")).toBeNull();
  });

  it("returns nothing for a model the catalog has never heard of", () => {
    expect(resolveModelRate(catalog, "cline", "cline-free/glm-5.2")).toBeNull();
  });
});

describe("resolveCatalogModel", () => {
  it("resolves an explicit user tag", () => {
    expect(resolveCatalogModel(catalog, "zenmux/claude-opus-5")?.rate.inputCostPerToken).toBe(6e-6);
  });

  it("returns nothing when a tag names a model this snapshot lacks", () => {
    // Better unpriced than silently free: a stale tag must not read as $0.
    expect(resolveCatalogModel(catalog, "anthropic/claude-opus-9")).toBeNull();
  });
});

describe("searchCatalog", () => {
  it("ranks model-key matches ahead of provider-name matches", () => {
    const results = searchCatalog(catalog, "opus", 10);

    expect(results.map((model) => model.id)).toEqual([
      "anthropic/claude-opus-5",
      "zenmux/claude-opus-5",
    ]);
  });

  it("leads with the canonical short key, not whichever reseller sorts first", () => {
    // The real catalog has well over a hundred entries matching "opus", most of
    // them resellers republishing it under a longer key. Returning the first
    // `limit` in catalog order buries the model the user actually meant.
    const withResellers = parseModelCatalog({
      ...document,
      aaa_reseller: {
        id: "aaa-reseller",
        name: "AAA Reseller",
        models: {
          "anthropic/claude-opus-5": {
            id: "anthropic/claude-opus-5",
            name: "Claude Opus 5",
            cost: { input: 6, output: 30 },
          },
        },
      },
    });

    expect(searchCatalog(withResellers, "claude-opus-5", 10)[0]?.id).toBe(
      "anthropic/claude-opus-5",
    );
  });

  it("matches the bare model name inside a nested key", () => {
    // Keys like `accounts/fireworks/models/deepseek-v4-flash` are common, and a
    // user searching the bare name should still find them.
    const nested = parseModelCatalog({
      fireworks: {
        id: "fireworks",
        name: "Fireworks",
        models: {
          "accounts/fireworks/models/deepseek-v4-flash": {
            id: "accounts/fireworks/models/deepseek-v4-flash",
            name: "DeepSeek V4 Flash",
            cost: { input: 0.9, output: 0.9 },
          },
        },
      },
    });

    expect(searchCatalog(nested, "deepseek-v4-flash", 10)).toHaveLength(1);
  });

  it("honours the limit", () => {
    expect(searchCatalog(catalog, "claude", 1)).toHaveLength(1);
  });

  it("reports costs per million, as the picker shows them", () => {
    expect(searchCatalog(catalog, "deepseek", 10)[0]).toEqual({
      id: "cline-pass/cline-pass/deepseek-v4-flash",
      providerName: "Cline Pass",
      modelName: "DeepSeek V4 Flash",
      inputCostPerMillion: 0.14,
      outputCostPerMillion: 0.28,
    });
  });

  it("answers an empty query with nothing rather than the whole catalog", () => {
    expect(searchCatalog(catalog, "  ", 10)).toEqual([]);
  });

  it("honours the result limit", () => {
    expect(searchCatalog(catalog, "claude", 1)).toHaveLength(1);
  });
});
