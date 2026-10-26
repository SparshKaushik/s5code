/**
 * Model rate lookup and cost arithmetic. Single-vendor providers price against
 * LiteLLM; gateway providers (pi, OpenCode) against provider-scoped models.dev.
 * Pure: fetching and caching the tables lives in `UsageService`.
 */
import type {
  UsageCategoryCost,
  UsageCostSource,
  UsageModelAlias,
  UsageModelPriceOverride,
  UsageTokenTotals,
} from "@t3tools/contracts";

import {
  resolveCatalogModel,
  resolveModelRate,
  type CatalogEntry,
  type ModelCatalog,
} from "./usageModelCatalog.ts";
import type { UsageRecord, UsageSpeed } from "@t3tools/provider-core/server/usage";

export interface TokenRates {
  readonly inputCostPerToken: number;
  readonly outputCostPerToken: number;
  readonly cacheReadCostPerToken: number;
  readonly cacheCreationCostPerToken: number;
}

/** Omits LiteLLM's above_272k_tokens, flex and batches variants, which transcripts do not record. */
export interface ModelRate extends TokenRates {
  readonly fast: TokenRates | null;
  readonly ultrafast: TokenRates | null;
}

export type RateTable = ReadonlyMap<string, ModelRate>;

export function createOverrideRateTable(
  overrides: Readonly<Record<string, UsageModelPriceOverride>>,
): RateTable {
  return new Map(
    Object.entries(overrides).map(([model, prices]) => [
      model.trim(),
      {
        inputCostPerToken: prices.inputCostPerMillionTokens / 1_000_000,
        outputCostPerToken: prices.outputCostPerMillionTokens / 1_000_000,
        cacheReadCostPerToken:
          (prices.cacheReadCostPerMillionTokens ?? prices.inputCostPerMillionTokens) / 1_000_000,
        cacheCreationCostPerToken:
          (prices.cacheWriteCostPerMillionTokens ?? prices.inputCostPerMillionTokens) / 1_000_000,
        fast: null,
        ultrafast: null,
      },
    ]),
  );
}

type LiteLlmEntry = Readonly<Record<string, unknown>>;

function finiteNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

// Missing cache rates fall back to input pricing; Anthropic normally discounts reads and charges extra for writes.
function readTokenRates(
  entry: LiteLlmEntry,
  suffix: string,
  standard?: TokenRates,
): TokenRates | null {
  const input = finiteNumber(entry[`input_cost_per_token${suffix}`]);
  const output = finiteNumber(entry[`output_cost_per_token${suffix}`]);
  if (input === null || output === null) return null;
  const cacheRate = (name: string, field: "cacheReadCostPerToken" | "cacheCreationCostPerToken") =>
    finiteNumber(entry[`${name}${suffix}`]) ??
    (standard !== undefined && standard.inputCostPerToken > 0
      ? (standard[field] / standard.inputCostPerToken) * input
      : input);
  return {
    inputCostPerToken: input,
    outputCostPerToken: output,
    cacheReadCostPerToken: cacheRate("cache_read_input_token_cost", "cacheReadCostPerToken"),
    cacheCreationCostPerToken: cacheRate(
      "cache_creation_input_token_cost",
      "cacheCreationCostPerToken",
    ),
  };
}

function scaleTokenRates(rates: TokenRates, multiple: number): TokenRates {
  return {
    inputCostPerToken: rates.inputCostPerToken * multiple,
    outputCostPerToken: rates.outputCostPerToken * multiple,
    cacheReadCostPerToken: rates.cacheReadCostPerToken * multiple,
    cacheCreationCostPerToken: rates.cacheCreationCostPerToken * multiple,
  };
}

function fastMultiplier(entry: LiteLlmEntry): number | null {
  const specific = entry["provider_specific_entry"];
  if (typeof specific !== "object" || specific === null) return null;
  const fast = finiteNumber((specific as Record<string, unknown>)["fast"]);
  return fast !== null && fast > 0 ? fast : null;
}

/** Drops models missing input or output rates to avoid understating cost. */
export function parseRateTable(document: unknown): RateTable {
  const table = new Map<string, ModelRate>();
  if (typeof document !== "object" || document === null) return table;

  for (const [name, raw] of Object.entries(document as Record<string, unknown>)) {
    if (typeof raw !== "object" || raw === null) continue;
    const entry = raw as LiteLlmEntry;
    const standard = readTokenRates(entry, "");
    if (standard === null) continue;

    const key = normalizeRateKey(name);
    if (key.length === 0) continue;
    const multiple = fastMultiplier(entry);
    table.set(key, {
      ...standard,
      fast:
        multiple === null
          ? readTokenRates(entry, "_priority", standard)
          : scaleTokenRates(standard, multiple),
      ultrafast: readTokenRates(entry, "_ultrafast", standard),
    });
  }

  const aliasCandidates = new Map<string, ModelRate | null>();
  for (const [key, rate] of table) {
    const alias = bareModelName(key);
    if (alias.length === 0 || alias === key || table.has(alias)) continue;
    const held = aliasCandidates.get(alias);
    if (held === undefined) {
      aliasCandidates.set(alias, rate);
    } else if (held !== null) {
      aliasCandidates.set(alias, mergeAliasRate(held, rate));
    }
  }
  for (const [alias, rate] of aliasCandidates) {
    if (rate !== null) table.set(alias, rate);
  }

  return table;
}

function sameTokenRates(a: TokenRates | null, b: TokenRates | null): boolean {
  if (a === null || b === null) return a === b;
  return (
    a.inputCostPerToken === b.inputCostPerToken &&
    a.outputCostPerToken === b.outputCostPerToken &&
    a.cacheReadCostPerToken === b.cacheReadCostPerToken &&
    a.cacheCreationCostPerToken === b.cacheCreationCostPerToken
  );
}

// Resellers often omit speed tiers, so a missing tier fills from the other entry instead of conflicting.
function mergeAliasRate(a: ModelRate, b: ModelRate): ModelRate | null {
  const fast = a.fast ?? b.fast;
  const ultrafast = a.ultrafast ?? b.ultrafast;
  if (
    !sameTokenRates(a, b) ||
    !sameTokenRates(fast, b.fast ?? fast) ||
    !sameTokenRates(ultrafast, b.ultrafast ?? ultrafast)
  ) {
    return null;
  }
  return { ...a, fast, ultrafast };
}

function ratesAt(rate: ModelRate, speed: UsageSpeed): TokenRates {
  return (speed === "standard" ? null : rate[speed]) ?? rate;
}

function normalizeRateKey(model: string): string {
  return model.trim().toLowerCase();
}

function bareModelName(key: string): string {
  const slash = key.lastIndexOf("/");
  return slash === -1 ? key : key.slice(slash + 1);
}

// Price Claude's bracketed context variants at the base tier, the only name the rate table knows.
function stripVariantSuffix(key: string): string {
  const bracket = key.indexOf("[");
  return bracket === -1 ? key : key.slice(0, bracket);
}

// Synthetic messages are unbilled; bare family names are generation-ambiguous and remain unpriced.
const UNPRICEABLE_MODELS = new Set([
  "<synthetic>",
  "synthetic",
  "opus",
  "sonnet",
  "haiku",
  "fable",
]);

// Immutable tables permit caching rates across twice-per-record pricing lookups.
const resolvedRates = new WeakMap<RateTable, Map<string, ModelRate | null>>();

export function lookupRate(table: RateTable, model: string): ModelRate | null {
  let resolved = resolvedRates.get(table);
  if (resolved === undefined) {
    resolved = new Map();
    resolvedRates.set(table, resolved);
  }
  let rate = resolved.get(model);
  if (rate === undefined) {
    rate = resolveRate(table, model);
    resolved.set(model, rate);
  }
  return rate;
}

function resolveRate(table: RateTable, model: string): ModelRate | null {
  const key = stripVariantSuffix(normalizeRateKey(model));
  const bareName = bareModelName(key);
  if (bareName.length === 0 || UNPRICEABLE_MODELS.has(bareName)) return null;
  return table.get(key) ?? null;
}

export type PricedRecord = Pick<
  UsageRecord,
  "model" | "rateModel" | "totals" | "speed" | "reportedCostUsd"
>;

export interface PricedUsage {
  readonly costUsd: number;
  readonly costSource: UsageCostSource;
  /**
   * The catalog entry the cost was computed from, when one was involved.
   *
   * The UI shows this so a user can see which guess we made and correct it, and
   * so tagged rows can be labelled by what they were tagged as.
   */
  readonly pricedAs?: string | null;
  /** `costUsd` by token category, or `null` when no rates are known to split it. */
  readonly categoryCostUsd: UsageCategoryCost | null;
  readonly speedPremiumUsd: number;
}

function costByCategory(totals: UsageTokenTotals, rates: TokenRates): UsageCategoryCost {
  return {
    input: totals.uncachedInputTokens * rates.inputCostPerToken,
    cacheRead: totals.cachedInputTokens * rates.cacheReadCostPerToken,
    cacheWrite: totals.cacheCreationTokens * rates.cacheCreationCostPerToken,
    output: totals.outputTokens * rates.outputCostPerToken,
  };
}

function sumCategories(cost: UsageCategoryCost): number {
  return cost.input + cost.cacheRead + cost.cacheWrite + cost.output;
}

/** Reasoning tokens are already included in outputTokens; do not charge twice. */
export function priceUsage(
  table: RateTable,
  record: PricedRecord,
  overrides?: RateTable,
): PricedUsage {
  const { model, reportedCostUsd } = record;
  const override = overrides?.get(model.trim());
  const reported =
    override === undefined && reportedCostUsd !== null && Number.isFinite(reportedCostUsd)
      ? reportedCostUsd
      : null;
  const rate = override ?? lookupRate(table, record.rateModel ?? model);
  return priceWithRate(record, rate, "modelPriced", reported);
}

function priceWithRate(
  record: PricedRecord,
  rate: ModelRate | null,
  costSource: UsageCostSource,
  reported: number | null,
): PricedUsage {
  const unsplit = (costUsd: number, costSource: UsageCostSource): PricedUsage => ({
    costUsd,
    costSource,
    categoryCostUsd: null,
    speedPremiumUsd: 0,
  });
  if (rate === null) {
    return reported === null ? unsplit(0, "unpriced") : unsplit(reported, "providerReported");
  }

  const listCost = costByCategory(record.totals, ratesAt(rate, record.speed));
  const listCostUsd = sumCategories(listCost);
  if (reported !== null && listCostUsd <= 0) return unsplit(reported, "providerReported");
  const premiumUsd =
    record.speed === "standard"
      ? 0
      : listCostUsd - sumCategories(costByCategory(record.totals, rate));
  const scale = reported === null ? 1 : reported / listCostUsd;
  return {
    costUsd: reported ?? listCostUsd,
    costSource: reported === null ? costSource : "providerReported",
    categoryCostUsd: {
      input: listCost.input * scale,
      cacheRead: listCost.cacheRead * scale,
      cacheWrite: listCost.cacheWrite * scale,
      output: listCost.output * scale,
    },
    speedPremiumUsd: premiumUsd * scale,
  };
}

/**
 * Identity of a model as the user tags it: the T3 provider, the upstream api
 * provider, and the raw model name. All three matter, because the same name
 * from two gateways can be two different products at two different prices.
 */
function modelAliasKey(provider: string, apiProvider: string, model: string): string {
  return `${provider}\u0000${apiProvider.trim().toLowerCase()}\u0000${model.trim().toLowerCase()}`;
}

export interface UsagePricerOptions {
  /** LiteLLM's flat table, used for Claude Code and Codex. */
  readonly rates: RateTable;
  /** models.dev, used for pi and for every user tag. */
  readonly catalog: ModelCatalog;
  readonly aliases: readonly UsageModelAlias[];
  readonly priceOverrides?: RateTable | undefined;
}

/**
 * Precedence: provider-reported cost, then the user's tag, then automatic
 * detection. A tag records what we could not infer, so nothing overrides it.
 */
export class UsagePricer {
  readonly #rates: RateTable;
  readonly #catalog: ModelCatalog;
  readonly #aliasEntries = new Map<string, CatalogEntry>();
  readonly #overrides: RateTable | undefined;

  constructor(options: UsagePricerOptions) {
    this.#rates = options.rates;
    this.#catalog = options.catalog;
    this.#overrides = options.priceOverrides;
    for (const alias of options.aliases) {
      const entry = resolveCatalogModel(this.#catalog, alias.catalogModelId);
      // A tag pointing at a catalog entry this environment's snapshot does not
      // have resolves to nothing rather than to zero, so the model keeps
      // reporting as unpriced and the user is not told a tag worked when it did
      // not.
      if (entry !== null) {
        this.#aliasEntries.set(
          modelAliasKey(alias.provider, alias.apiProvider, alias.model),
          entry,
        );
      }
    }
  }

  /** The rate for a record, and where it came from. `null` means unpriced. */
  #resolve(
    record: UsageRecord,
  ): { rate: ModelRate; source: UsageCostSource; pricedAs: string | null } | null {
    const override = this.#overrides?.get(record.model.trim());
    if (override !== undefined) {
      return { rate: override, source: "modelPriced", pricedAs: null };
    }

    const tagged = this.#aliasEntries.get(
      modelAliasKey(record.provider, record.apiProvider, record.model),
    );
    if (tagged !== undefined) {
      return { rate: tagged.rate, source: "userTagged", pricedAs: tagged.id };
    }

    if (record.provider === "pi" || record.provider === "opencode") {
      const entry = resolveModelRate(this.#catalog, record.apiProvider, record.model);
      return entry === null
        ? null
        : { rate: entry.rate, source: "modelPriced", pricedAs: entry.id };
    }

    const rate = lookupRate(this.#rates, record.rateModel ?? record.model);
    return rate === null ? null : { rate, source: "modelPriced", pricedAs: null };
  }

  price(record: UsageRecord): PricedUsage {
    const override = this.#overrides?.get(record.model.trim());
    const reported =
      override === undefined &&
      record.reportedCostUsd !== null &&
      Number.isFinite(record.reportedCostUsd)
        ? record.reportedCostUsd
        : null;
    const resolved = this.#resolve(record);
    return {
      ...priceWithRate(record, resolved?.rate ?? null, resolved?.source ?? "unpriced", reported),
      pricedAs: reported === null ? (resolved?.pricedAs ?? null) : null,
    };
  }

  /**
   * What the cached input would have cost at full input rates, minus what it
   * actually cost. Drives the "cache savings" figure.
   */
  cacheSavingsUsd(record: UsageRecord): number {
    const resolved = this.#resolve(record);
    if (resolved === null) return 0;
    const rates = ratesAt(resolved.rate, record.speed);
    return (
      record.totals.cachedInputTokens * (rates.inputCostPerToken - rates.cacheReadCostPerToken)
    );
  }
}

export function cacheSavingsUsd(
  table: RateTable,
  record: PricedRecord,
  overrides?: RateTable,
): number {
  const rate =
    overrides?.get(record.model.trim()) ?? lookupRate(table, record.rateModel ?? record.model);
  if (rate === null) return 0;
  const rates = ratesAt(rate, record.speed);
  return record.totals.cachedInputTokens * (rates.inputCostPerToken - rates.cacheReadCostPerToken);
}
