/**
 * The models.dev catalog, projected into rates.
 *
 * LiteLLM's table is a flat namespace, which is fine for Claude Code and Codex
 * because each speaks to one vendor. pi speaks to gateways, and gateways report
 * names LiteLLM has never heard of (`cline-pass/deepseek-v4-flash`) or resells
 * of names it prices as first-party. models.dev is provider-scoped, so those
 * resolve exactly.
 *
 * The important restraint here is refusing to guess. `claude-opus-5` is served
 * by eleven providers at three different prices; picking one and presenting the
 * result as fact would be worse than admitting we do not know. Those stay
 * unpriced until a user tags them, which is what {@link resolveCatalogModel} is
 * for.
 *
 * Pure. Fetching and caching the document lives in `UsageService`.
 *
 * @module usageModelCatalog
 */
import type { ModelRate } from "./usagePricing.ts";

/** models.dev publishes USD per million tokens; rates are per token. */
const PER_MILLION = 1_000_000;

export interface CatalogEntry {
  readonly id: string;
  readonly providerName: string;
  readonly modelName: string;
  readonly rate: ModelRate;
}

export interface ModelCatalog {
  /** Every entry, keyed by `<providerId>/<modelKey>` lowercased. */
  readonly byId: ReadonlyMap<string, CatalogEntry>;
  /**
   * Entries grouped by bare model key, lowercased. A model served by several
   * providers has several entries here, which is exactly the ambiguity
   * {@link resolveModelRate} refuses to resolve on the user's behalf.
   */
  readonly byModelKey: ReadonlyMap<string, readonly CatalogEntry[]>;
  readonly size: number;
}

export const EMPTY_CATALOG: ModelCatalog = {
  byId: new Map(),
  byModelKey: new Map(),
  size: 0,
};

function finiteNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function trimmedString(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed.length === 0 ? null : trimmed;
}

/**
 * Projects the models.dev document into a catalog.
 *
 * Entries without both an input and an output rate are dropped, matching the
 * LiteLLM parser: a half-priced model under-reports silently, which is worse
 * than reporting it as unpriced.
 *
 * Tiered rates (`tiers`, `context_over_200k`) are deliberately ignored in favour
 * of the base tier. Transcripts do not record which tier served a request, so
 * anything else would be a guess dressed up as precision.
 */
export function parseModelCatalog(document: unknown): ModelCatalog {
  if (typeof document !== "object" || document === null) return EMPTY_CATALOG;

  const byId = new Map<string, CatalogEntry>();
  const byModelKey = new Map<string, CatalogEntry[]>();

  for (const [providerId, rawProvider] of Object.entries(document as Record<string, unknown>)) {
    if (typeof rawProvider !== "object" || rawProvider === null) continue;
    const provider = rawProvider as Record<string, unknown>;
    const models = provider["models"];
    if (typeof models !== "object" || models === null) continue;
    const providerName = trimmedString(provider["name"]) ?? providerId;

    for (const [modelKey, rawModel] of Object.entries(models as Record<string, unknown>)) {
      if (typeof rawModel !== "object" || rawModel === null) continue;
      const cost = (rawModel as Record<string, unknown>)["cost"];
      if (typeof cost !== "object" || cost === null) continue;
      const costRecord = cost as Record<string, unknown>;

      const input = finiteNumber(costRecord["input"]);
      const output = finiteNumber(costRecord["output"]);
      if (input === null || output === null) continue;

      const inputCostPerToken = input / PER_MILLION;
      const entry: CatalogEntry = {
        id: `${providerId}/${modelKey}`,
        providerName,
        modelName: trimmedString((rawModel as Record<string, unknown>)["name"]) ?? modelKey,
        rate: {
          inputCostPerToken,
          outputCostPerToken: output / PER_MILLION,
          // A model that omits cache rates bills cached input as plain input
          // rather than as free, same rule the LiteLLM table follows. An
          // explicit zero is honoured: some providers really do read cache free.
          cacheReadCostPerToken: (finiteNumber(costRecord["cache_read"]) ?? input) / PER_MILLION,
          cacheCreationCostPerToken:
            (finiteNumber(costRecord["cache_write"]) ?? input) / PER_MILLION,
        },
      };

      byId.set(entry.id.toLowerCase(), entry);
      const bare = modelKey.trim().toLowerCase();
      const existing = byModelKey.get(bare);
      if (existing === undefined) byModelKey.set(bare, [entry]);
      else existing.push(entry);
    }
  }

  return { byId, byModelKey, size: byId.size };
}

/** Looks up an explicit `<providerId>/<modelKey>` entry, as a user tag carries. */
export function resolveCatalogModel(
  catalog: ModelCatalog,
  catalogModelId: string,
): CatalogEntry | null {
  return catalog.byId.get(catalogModelId.trim().toLowerCase()) ?? null;
}

function ratesAgree(a: ModelRate, b: ModelRate): boolean {
  return (
    a.inputCostPerToken === b.inputCostPerToken &&
    a.outputCostPerToken === b.outputCostPerToken &&
    a.cacheReadCostPerToken === b.cacheReadCostPerToken &&
    a.cacheCreationCostPerToken === b.cacheCreationCostPerToken
  );
}

/**
 * Resolves a transcript's `(apiProvider, model)` pair to a rate, or `null` when
 * the answer is genuinely unknown.
 *
 * Tried in order:
 *
 * 1. The api provider's own catalog entry. pi reports gateway ids without
 *    separators (`clinepass`) where models.dev hyphenates (`cline-pass`), so
 *    both sides are compared with separators stripped.
 * 2. The model key on its own, when every provider serving it agrees on price.
 *    This is what makes `cline-pass/deepseek-v4-flash` (one provider) and the
 *    long tail of single-source models resolve without ceremony.
 *
 * Anything else is ambiguous or absent and stays unpriced.
 */
export function resolveModelRate(
  catalog: ModelCatalog,
  apiProvider: string,
  model: string,
): CatalogEntry | null {
  const modelKey = model.trim().toLowerCase();
  if (modelKey.length === 0) return null;

  const candidates = catalog.byModelKey.get(modelKey);
  if (candidates === undefined || candidates.length === 0) return null;

  const wantedProvider = collapseProviderId(apiProvider);
  if (wantedProvider.length > 0) {
    for (const candidate of candidates) {
      const candidateProvider = candidate.id.slice(0, candidate.id.indexOf("/"));
      if (collapseProviderId(candidateProvider) === wantedProvider) return candidate;
    }
  }

  const [first] = candidates;
  if (first === undefined) return null;
  return candidates.every((candidate) => ratesAgree(candidate.rate, first.rate)) ? first : null;
}

/** `cline-pass` and pi's `clinepass` name the same gateway. */
function collapseProviderId(providerId: string): string {
  return providerId
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]/g, "");
}

export interface CatalogSearchResult {
  readonly id: string;
  readonly providerName: string;
  readonly modelName: string;
  readonly inputCostPerMillion: number;
  readonly outputCostPerMillion: number;
}

/**
 * Substring search over the catalog, for the tag picker.
 *
 * The catalog is a few thousand entries and megabytes on the wire, so it stays
 * on the server and the client asks questions of it instead.
 *
 * Ranking matters more than it looks. Searching "opus" matches well over a
 * hundred entries, most of them resellers republishing the same model under a
 * longer key. Whichever provider happens to sort first in the catalog is not a
 * useful answer, so every match is scored and the canonical, shortest-keyed
 * ones lead.
 */
export function searchCatalog(
  catalog: ModelCatalog,
  query: string,
  limit: number,
): readonly CatalogSearchResult[] {
  const needle = query.trim().toLowerCase();
  if (needle.length === 0) return [];

  const scored: { readonly rank: number; readonly entry: CatalogEntry; readonly key: string }[] =
    [];

  for (const [id, entry] of catalog.byId) {
    const modelKey = id.slice(id.indexOf("/") + 1);
    const rank = rankMatch(modelKey, id, entry.modelName.toLowerCase(), needle);
    if (rank === null) continue;
    scored.push({ rank, entry, key: modelKey });
  }

  // A shorter key is the more canonical name for the same model: `claude-opus-5`
  // is what a user means, `anthropic/claude-opus-4-6` republished by a reseller
  // is not. Id breaks remaining ties so results do not shuffle between calls.
  scored.sort(
    (a, b) =>
      a.rank - b.rank || a.key.length - b.key.length || a.entry.id.localeCompare(b.entry.id),
  );

  return scored.slice(0, limit).map(({ entry }) => ({
    id: entry.id,
    providerName: entry.providerName,
    modelName: entry.modelName,
    inputCostPerMillion: entry.rate.inputCostPerToken * PER_MILLION,
    outputCostPerMillion: entry.rate.outputCostPerToken * PER_MILLION,
  }));
}

/** Lower is better. `null` means no match at all. */
function rankMatch(modelKey: string, id: string, modelName: string, needle: string): number | null {
  if (modelKey === needle) return 0;
  // The last segment is the bare model name for keys like
  // `accounts/fireworks/models/deepseek-v4-flash`.
  const tail = modelKey.slice(modelKey.lastIndexOf("/") + 1);
  if (tail === needle) return 1;
  if (tail.startsWith(needle)) return 2;
  if (modelKey.includes(needle)) return 3;
  if (modelName.includes(needle)) return 4;
  // Matched only on the provider's own name, so every one of its models
  // qualifies. Useful, but never ahead of a real model-name hit.
  if (id.includes(needle)) return 5;
  return null;
}
