/**
 * Model rate lookup and cost arithmetic.
 *
 * Two rate sources, because two shapes of problem. Claude Code and Codex each
 * speak to one vendor under names LiteLLM already publishes, which is the table
 * `ccusage` prices against and the one that keeps those numbers stable. pi
 * speaks to gateways whose names only models.dev knows, provider-scoped.
 * Cursor reports API-equivalent cost on each dashboard event. The
 * {@link UsagePricer} picks per provider and applies the user's tags over both.
 *
 * Everything here is pure: fetching and caching the tables lives in
 * `UsageService`.
 *
 * @module usagePricing
 */
import type { UsageCostSource, UsageModelAlias, UsageTokenTotals } from "@t3tools/contracts";

import {
  EMPTY_CATALOG,
  resolveCatalogModel,
  resolveModelRate,
  type CatalogEntry,
  type ModelCatalog,
} from "./usageModelCatalog.ts";
import type { UsageRecord } from "./usageTranscripts.ts";

/**
 * The subset of a LiteLLM entry we price against. All values are USD per token.
 *
 * LiteLLM also publishes tiered variants (`*_above_272k_tokens`, `*_flex`,
 * `*_priority`, `*_batches`). We deliberately price at the base tier: the
 * transcripts don't record which tier served a request, so anything else would
 * be a guess dressed up as precision.
 */
export interface ModelRate {
  readonly inputCostPerToken: number;
  readonly outputCostPerToken: number;
  readonly cacheReadCostPerToken: number;
  readonly cacheCreationCostPerToken: number;
}

export type RateTable = ReadonlyMap<string, ModelRate>;

/** Raw shape of one LiteLLM entry, narrowed to the fields we read. */
interface LiteLlmEntry {
  readonly input_cost_per_token?: unknown;
  readonly output_cost_per_token?: unknown;
  readonly cache_read_input_token_cost?: unknown;
  readonly cache_creation_input_token_cost?: unknown;
}

function finiteNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/**
 * Projects the LiteLLM document into a rate table.
 *
 * Entries without both an input and an output rate are dropped: a half-priced
 * model would silently under-report cost, which is worse than reporting the
 * model as unpriced.
 */
export function parseRateTable(document: unknown): RateTable {
  const table = new Map<string, ModelRate>();
  if (typeof document !== "object" || document === null) return table;

  for (const [name, raw] of Object.entries(document as Record<string, unknown>)) {
    if (typeof raw !== "object" || raw === null) continue;
    const entry = raw as LiteLlmEntry;
    const input = finiteNumber(entry.input_cost_per_token);
    const output = finiteNumber(entry.output_cost_per_token);
    if (input === null || output === null) continue;

    table.set(normalizeModelName(name), {
      inputCostPerToken: input,
      outputCostPerToken: output,
      // Anthropic bills cache reads at a discount and cache writes at a
      // premium. When a model omits them, cached input is priced as plain
      // input rather than as free.
      cacheReadCostPerToken: finiteNumber(entry.cache_read_input_token_cost) ?? input,
      cacheCreationCostPerToken: finiteNumber(entry.cache_creation_input_token_cost) ?? input,
    });
  }
  return table;
}

/**
 * Canonicalises a model name for lookup.
 *
 * Strips a `provider/` prefix (LiteLLM publishes both `claude-opus-5` and
 * `anthropic/claude-opus-5`) and lowercases, since transcripts are inconsistent
 * about casing.
 */
export function normalizeModelName(model: string): string {
  const trimmed = model.trim().toLowerCase();
  const slash = trimmed.lastIndexOf("/");
  return slash === -1 ? trimmed : trimmed.slice(slash + 1);
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

export function lookupRate(table: RateTable, model: string): ModelRate | null {
  const normalized = normalizeModelName(model);
  if (normalized.length === 0 || UNPRICEABLE_MODELS.has(normalized)) return null;
  return table.get(normalized) ?? null;
}

export interface PricedUsage {
  readonly costUsd: number;
  readonly costSource: UsageCostSource;
  /**
   * The catalog entry the cost was computed from, when one was involved.
   *
   * The UI shows this so a user can see which guess we made and correct it, and
   * so tagged rows can be labelled by what they were tagged as.
   */
  readonly pricedAs: string | null;
}

function applyRate(rate: ModelRate, totals: UsageTokenTotals): number {
  // reasoningTokens is intentionally not charged separately: it is already
  // counted inside outputTokens.
  return (
    totals.uncachedInputTokens * rate.inputCostPerToken +
    totals.cachedInputTokens * rate.cacheReadCostPerToken +
    totals.cacheCreationTokens * rate.cacheCreationCostPerToken +
    totals.outputTokens * rate.outputCostPerToken
  );
}

/**
 * Identity of a model as the user tags it: the T3 provider, the upstream api
 * provider, and the raw model name. All three matter, because the same name
 * from two gateways can be two different products at two different prices.
 */
export function modelAliasKey(provider: string, apiProvider: string, model: string): string {
  return `${provider}\u0000${apiProvider.trim().toLowerCase()}\u0000${model.trim().toLowerCase()}`;
}

export interface UsagePricerOptions {
  /** LiteLLM's flat table, used for Claude Code and Codex. */
  readonly rates: RateTable;
  /** models.dev, used for pi and for every user tag. */
  readonly catalog: ModelCatalog;
  readonly aliases: readonly UsageModelAlias[];
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

  constructor(options: UsagePricerOptions) {
    this.#rates = options.rates;
    this.#catalog = options.catalog;
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
    const tagged = this.#aliasEntries.get(
      modelAliasKey(record.provider, record.apiProvider, record.model),
    );
    if (tagged !== undefined) {
      return { rate: tagged.rate, source: "userTagged", pricedAs: tagged.id };
    }

    if (record.provider === "pi") {
      const entry = resolveModelRate(this.#catalog, record.apiProvider, record.model);
      return entry === null
        ? null
        : { rate: entry.rate, source: "modelPriced", pricedAs: entry.id };
    }

    const rate = lookupRate(this.#rates, record.model);
    return rate === null ? null : { rate, source: "modelPriced", pricedAs: null };
  }

  price(record: UsageRecord): PricedUsage {
    if (record.reportedCostUsd !== null && Number.isFinite(record.reportedCostUsd)) {
      return {
        costUsd: record.reportedCostUsd,
        costSource: "providerReported",
        pricedAs: null,
      };
    }
    const resolved = this.#resolve(record);
    if (resolved === null) return { costUsd: 0, costSource: "unpriced", pricedAs: null };
    return {
      costUsd: applyRate(resolved.rate, record.totals),
      costSource: resolved.source,
      pricedAs: resolved.pricedAs,
    };
  }

  /**
   * What the cached input would have cost at full input rates, minus what it
   * actually cost. Drives the "cache savings" figure.
   */
  cacheSavingsUsd(record: UsageRecord): number {
    const resolved = this.#resolve(record);
    if (resolved === null) return 0;
    return (
      record.totals.cachedInputTokens *
      (resolved.rate.inputCostPerToken - resolved.rate.cacheReadCostPerToken)
    );
  }
}

/** A pricer that prices nothing, for tests and for a cold rate fetch. */
export function emptyPricer(): UsagePricer {
  return new UsagePricer({ rates: new Map(), catalog: EMPTY_CATALOG, aliases: [] });
}
