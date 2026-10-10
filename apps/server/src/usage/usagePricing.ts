/**
 * Model rate lookup and cost arithmetic.
 *
 * Two rate sources, because two shapes of problem. Claude Code and Codex each
 * speak to one vendor under names LiteLLM already publishes, which is the table
 * `ccusage` prices against and the one that keeps those numbers stable. pi and
 * OpenCode speak to gateways whose names only models.dev knows,
 * provider-scoped. The
 * {@link UsagePricer} picks per provider and applies the user's tags over both.
 *
 * Everything here is pure: fetching and caching the tables lives in
 * `UsageService`.
 *
 * @module usagePricing
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

/** Token rates for one billing speed. All values are USD per token. */
export interface TokenRates {
  readonly inputCostPerToken: number;
  readonly outputCostPerToken: number;
  readonly cacheReadCostPerToken: number;
  readonly cacheCreationCostPerToken: number;
}

/**
 * The subset of a LiteLLM entry we price against: standard rates, plus rates
 * for each faster speed the model publishes. A request at a speed with no
 * published rates bills at the standard rates.
 *
 * LiteLLM also publishes `*_above_272k_tokens`, `*_flex`, and `*_batches`
 * variants. Transcripts don't record those, so we don't price them.
 */
export interface ModelRate extends TokenRates {
  /**
   * From LiteLLM's `provider_specific_entry.fast` multiple (Claude fast mode),
   * or else its `*_priority` rates (Codex `priority`).
   */
  readonly fast: TokenRates | null;
  /** From LiteLLM's `*_ultrafast` rates (Codex `ultrafast`). */
  readonly ultrafast: TokenRates | null;
}

export type RateTable = ReadonlyMap<string, ModelRate>;

/**
 * Custom IDs keep their case, provider prefix, and variant suffix. Custom rates
 * apply as entered, at every speed.
 */
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

/** One raw LiteLLM entry. Field names carry a tier suffix, e.g. `_priority`. */
type LiteLlmEntry = Readonly<Record<string, unknown>>;

function finiteNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/**
 * Reads one rate set, `suffix` selecting a tier such as `_priority`. Returns
 * `null` without both an input and an output rate.
 *
 * Anthropic bills cache reads at a discount and cache writes at a premium.
 * When the standard tier omits them, cached input is priced as plain input
 * rather than as free. A faster tier that omits them keeps the standard tier's
 * cache-to-input ratio.
 */
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

/** Reads `provider_specific_entry.fast`, e.g. `2` for Claude Opus 5.5. */
function fastMultiplier(entry: LiteLlmEntry): number | null {
  const specific = entry["provider_specific_entry"];
  if (typeof specific !== "object" || specific === null) return null;
  const fast = finiteNumber((specific as Record<string, unknown>)["fast"]);
  return fast !== null && fast > 0 ? fast : null;
}

/**
 * Projects the LiteLLM document into a rate table.
 *
 * Entries without both an input and an output rate are dropped: a half-priced
 * model would silently under-report cost, which is worse than reporting the
 * model as unpriced.
 *
 * Entries keep their full normalized key; a bare name is aliased only when no
 * canonical entry exists and every qualified entry has the same rate.
 */
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

  // `null` marks a bare name claimed at conflicting rates: no alias for it.
  const aliasCandidates = new Map<string, ModelRate | null>();
  for (const [key, rate] of table) {
    const alias = bareModelName(key);
    if (alias.length === 0 || alias === key || table.has(alias)) continue;
    const held = aliasCandidates.get(alias);
    if (held === undefined) {
      aliasCandidates.set(alias, rate);
    } else if (held !== null && !sameRate(held, rate)) {
      aliasCandidates.set(alias, null);
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

function sameRate(a: ModelRate, b: ModelRate): boolean {
  return (
    sameTokenRates(a, b) &&
    sameTokenRates(a.fast, b.fast) &&
    sameTokenRates(a.ultrafast, b.ultrafast)
  );
}

/** The rates a request at `speed` bills at. */
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

/**
 * Drops a bracketed variant suffix such as `claude-fable-5-1[1m]`, which
 * Claude Code writes for the 1M context tier. The rate table only knows the
 * base name, and we price at the base tier anyway.
 */
function stripVariantSuffix(key: string): string {
  const bracket = key.indexOf("[");
  return bracket === -1 ? key : key.slice(0, bracket);
}

/**
 * Models we never price, regardless of the table.
 *
 * `<synthetic>` marks locally generated messages that were never billed. Bare
 * family names ("opus", "sonnet") are genuinely ambiguous across generations,
 * so we report them as unpriced instead of guessing a generation.
 */
const UNPRICEABLE_MODELS = new Set([
  "<synthetic>",
  "synthetic",
  "opus",
  "sonnet",
  "haiku",
  "fable",
]);

/**
 * Lookups per table, by raw model name. A scan prices every record twice
 * against a few dozen models, and tables are never mutated once built.
 */
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

/** The parts of a transcript record that decide its price. */
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
  /** What `costUsd` exceeds the same tokens at standard rates. `0` without rates. */
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

/**
 * Prices one record's tokens.
 *
 * A provider-reported cost is kept as is, and split by category and speed in
 * proportion to the model's list rates when those are known.
 *
 * `reasoningTokens` is intentionally not charged separately: it is already
 * counted inside `outputTokens`.
 */
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
 * Resolves one record to a cost.
 *
 * Order matters and is the whole design: an explicit cost the provider reported
 * beats everything, then the user's own tag, then automatic detection. A user
 * who tags a model has said something we could not work out, so nothing later
 * gets to override it.
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
