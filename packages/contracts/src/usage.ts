/**
 * Usage reporting contract.
 *
 * Each environment scans the provider CLIs' own on-disk session transcripts
 * (`~/.claude/projects/**\/*.jsonl`, `~/.codex/sessions/**\/*.jsonl`,
 * Each environment scans the provider CLIs' own on-disk session transcripts
 * (`~/.claude/projects/**\/*.jsonl`, `~/.codex/sessions/**\/*.jsonl`,
 * `~/.grok/sessions/**\/updates.jsonl`, `~/.pi/agent/sessions/**\/*.jsonl`) plus
 * Cursor's account-wide dashboard API rather than relying on T3 Code's own
 * orchestration projections, so usage stays complete even for turns that were
 * never driven through T3 Code. This mirrors the approach `ccusage` takes.
 *
 * Environments return pre-aggregated `(day, hourStart?, provider, model)`
 * buckets. Raw transcript records never cross the wire.
 *
 * @module usage
 */
import * as Effect from "effect/Effect";
import * as Schema from "effect/Schema";

import { NonNegativeInt, TrimmedNonEmptyString, TrimmedString } from "./baseSchemas.ts";

/**
 * Bumped whenever the shape of {@link UsageSummary} changes incompatibly. The
 * client renders partial coverage when an environment reports an older version
 * rather than failing the whole page.
 */
export const USAGE_CONTRACT_VERSION = 7 as const;

/**
 * Oldest {@link UsageSummary} version a current client will still merge.
 *
 * v5 only adds `grok` to {@link UsageProviderKind}; v4 Claude/Codex buckets
 * remain valid, so mixed-version environments keep those totals instead of
 * treating every older server as stale.
 */
export const USAGE_MERGE_COMPATIBLE_SINCE = 4 as const;

export const UsageProviderKind = Schema.Literals(["claude", "codex", "cursor", "grok", "pi"]);
export type UsageProviderKind = typeof UsageProviderKind.Type;

/**
 * A models.dev catalog entry, as `<providerId>/<modelKey>`.
 *
 * The model key may itself contain slashes (`cline-pass/deepseek-v4-flash`),
 * so only the first separator delimits the provider.
 */
export const UsageCatalogModelId = TrimmedNonEmptyString.check(
  Schema.isPattern(/^[^/]+\/.+$/),
).pipe(Schema.brand("UsageCatalogModelId"));
export type UsageCatalogModelId = typeof UsageCatalogModelId.Type;

/**
 * A user's answer to "what is this model, really?".
 *
 * Routers and gateways report names that no catalog knows (`cline-free/glm-5.2`)
 * or names several catalogs price differently (`claude-opus-5` costs three
 * different amounts across the eleven providers that serve it). Those stay
 * unpriced rather than guessed, and this is how a user resolves one. Tagged
 * records are priced at the target's rates and consolidate into its row.
 */
export const UsageModelAlias = Schema.Struct({
  provider: UsageProviderKind,
  /**
   * The upstream API provider the transcript named, or `""` when the provider
   * talks to one vendor. Part of the identity because two gateways reselling
   * the same model name can charge differently, so tagging one must not
   * silently retag the other.
   */
  apiProvider: TrimmedString,
  /** The model name exactly as the transcript reported it. */
  model: TrimmedNonEmptyString,
  catalogModelId: UsageCatalogModelId,
});
export type UsageModelAlias = typeof UsageModelAlias.Type;

/**
 * A calendar day in the reporting time zone, formatted `YYYY-MM-DD`.
 *
 * Days are bucketed server-side so that a turn always lands on the day the user
 * experienced it, not the UTC day.
 */
const USAGE_DAY_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

export const UsageDay = TrimmedNonEmptyString.check(Schema.isPattern(USAGE_DAY_PATTERN)).pipe(
  Schema.brand("UsageDay"),
);
export type UsageDay = typeof UsageDay.Type;

export const UsageResolution = Schema.Literals(["day", "hour"]);
export type UsageResolution = typeof UsageResolution.Type;

/**
 * Why a bucket's cost is what it is.
 *
 * - `providerReported` - the transcript carried an explicit cost figure.
 * - `modelPriced` - we matched the model against a rate table or custom price override.
 * - `userTagged` - the model was unknown until the user mapped it to a catalog
 *   entry, and is priced at that entry's rates.
 * - `unpriced` - tokens are known, rates are not. Counted in totals, excluded
 *   from cost. These are the rows worth tagging.
 */
export const UsageCostSource = Schema.Literals([
  "providerReported",
  "modelPriced",
  "userTagged",
  "unpriced",
]);
export type UsageCostSource = typeof UsageCostSource.Type;

/**
 * Token counts for a bucket.
 *
 * `cachedInputTokens` and `cacheCreationTokens` are disjoint from
 * `uncachedInputTokens`; summing all three gives total input. `reasoningTokens`
 * is a *subset* of `outputTokens` (Codex reports it that way, and Anthropic
 * folds thinking into output), so it must never be added on top.
 */
export const UsageTokenTotals = Schema.Struct({
  uncachedInputTokens: NonNegativeInt,
  cachedInputTokens: NonNegativeInt,
  cacheCreationTokens: NonNegativeInt,
  outputTokens: NonNegativeInt,
  reasoningTokens: NonNegativeInt,
});
export type UsageTokenTotals = typeof UsageTokenTotals.Type;

/**
 * One `(day, hourStart?, provider, model)` cell. `hourStart` is the UTC start
 * instant of a rolling bucket and is present only for hourly requests.
 *
 * `costUsd` is the raw API-equivalent cost of these tokens. It is not money
 * spent: subscription plans bill separately. `unpricedRecords` counts records
 * whose tokens are included in the token totals but which contributed nothing
 * to `costUsd`.
 */
export const UsageBucket = Schema.Struct({
  day: UsageDay,
  hourStart: Schema.optional(TrimmedNonEmptyString),
  provider: UsageProviderKind,
  model: TrimmedNonEmptyString,
  /**
   * The upstream API provider, when the usage source names one. pi routes through
   * gateways, so this is what separates `claude-opus-5` served by a reseller
   * from the same name served by Anthropic. Empty for providers that speak to a
   * single vendor.
   */
  apiProvider: Schema.String,
  totals: UsageTokenTotals,
  costUsd: Schema.Number,
  /**
   * What the cached input would have cost at full input rates minus what it
   * actually cost. Requires the rate table, so it is computed alongside cost
   * rather than derived on the client.
   */
  cacheSavingsUsd: Schema.Number,
  /**
   * True when at least one record's input/cache split was simulated from a
   * rolling context estimate rather than reported as billed tokens.
   */
  inputTokensEstimated: Schema.Boolean,
  costSource: UsageCostSource,
  /**
   * The catalog entry this cell was priced against, as `<providerId>/<modelKey>`,
   * or `null` when no catalog was involved (provider-reported, LiteLLM-priced,
   * or unpriced).
   *
   * Present so the UI can show which model we assumed and let the user correct
   * it, rather than presenting a guess as fact.
   */
  pricedAs: Schema.NullOr(UsageCatalogModelId),
  /** Distinct assistant responses, after de-duplication. */
  records: NonNegativeInt,
  unpricedRecords: NonNegativeInt,
  /** Distinct transcript sessions that contributed to this cell. */
  sessions: NonNegativeInt,
});
export type UsageBucket = typeof UsageBucket.Type;

/**
 * Identifies the physical transcript directory or remote account a source read
 * from.
 *
 * Two environments on the same machine (worktree servers, for example) resolve
 * the same provider home and would otherwise double count. The client drops
 * duplicate fingerprints before merging.
 */
export const UsageSourceFingerprint = Schema.Struct({
  hostId: TrimmedNonEmptyString,
  provider: UsageProviderKind,
  /**
   * Stable source identity. Local transcript sources carry a filesystem path;
   * account-wide APIs carry a non-secret account identifier such as
   * `cursor-account:<hash>`.
   */
  resolvedHomePath: TrimmedNonEmptyString,
  /**
   * Filesystem identity of a transcript directory, as `device:inode`. Empty
   * for remote account sources.
   *
   * Hostname and path alone are not enough for local sources: every Mac in a
   * fleet resolves
   * `/Users/<user>/.claude`, so two machines that happen to share a hostname
   * would look like one source and have their usage silently dropped. The
   * device/inode pair is stable for two servers reading the same directory and
   * effectively never collides across machines. Empty when it cannot be read.
   */
  volumeId: Schema.String,
});
export type UsageSourceFingerprint = typeof UsageSourceFingerprint.Type;

export const UsageSourceStatus = Schema.Literals(["ok", "missing", "partial", "failed"]);
export type UsageSourceStatus = typeof UsageSourceStatus.Type;

export const UsageSource = Schema.Struct({
  fingerprint: UsageSourceFingerprint,
  status: UsageSourceStatus,
  scannedFiles: NonNegativeInt,
  skippedFiles: NonNegativeInt,
  /** Records that parsed but carried no recognisable usage payload. */
  malformedRecords: NonNegativeInt,
  /**
   * Distinct transcript sessions seen under this directory. Buckets also carry
   * per-bucket session counts, but a session spans days and models, so summing
   * those overcounts; this is the figure clients should total.
   */
  distinctSessions: NonNegativeInt,
  message: Schema.NullOr(TrimmedNonEmptyString),
});
export type UsageSource = typeof UsageSource.Type;

export const UsagePricingStatus = Schema.Literals(["fresh", "cached", "unavailable"]);
export type UsagePricingStatus = typeof UsagePricingStatus.Type;

/**
 * Provenance for the rate table, so the UI can be honest about how good the
 * cost figures are.
 */
export const UsagePricing = Schema.Struct({
  status: UsagePricingStatus,
  source: TrimmedNonEmptyString,
  fetchedAt: Schema.NullOr(Schema.String),
  knownModels: NonNegativeInt,
});
export type UsagePricing = typeof UsagePricing.Type;

export const UsageSummaryInput = Schema.Struct({
  /** Inclusive first day of the window, in `timeZone`. */
  sinceDay: UsageDay,
  /** Inclusive last day of the window, in `timeZone`. */
  untilDay: UsageDay,
  /**
   * IANA zone the client wants days bucketed in. An offset would be wrong for
   * any window that crosses a DST boundary.
   */
  timeZone: TrimmedNonEmptyString,
  /**
   * The client's model tags. Pricing runs server-side (the catalog is megabytes
   * and never crosses the wire), but the tags belong to the user rather than to
   * one environment, so they travel with the request and apply everywhere.
   */
  modelAliases: Schema.Array(UsageModelAlias).pipe(Schema.withDecodingDefault(Effect.succeed([]))),
  /** Defaults to daily for older clients. */
  resolution: Schema.optional(UsageResolution),
  /** Inclusive UTC instant for an hourly rolling window. */
  sinceTime: Schema.optional(TrimmedNonEmptyString),
  /** Exclusive UTC instant for an hourly rolling window. */
  untilTime: Schema.optional(TrimmedNonEmptyString),
});
export type UsageSummaryInput = typeof UsageSummaryInput.Type;

/** One candidate in the tag picker. */
export const UsageCatalogModel = Schema.Struct({
  id: UsageCatalogModelId,
  providerName: TrimmedNonEmptyString,
  modelName: TrimmedNonEmptyString,
  /** USD per million input/output tokens, for a sanity check before tagging. */
  inputCostPerMillion: Schema.Number,
  outputCostPerMillion: Schema.Number,
});
export type UsageCatalogModel = typeof UsageCatalogModel.Type;

export const UsageModelSearchInput = Schema.Struct({
  query: TrimmedNonEmptyString,
});
export type UsageModelSearchInput = typeof UsageModelSearchInput.Type;

export const UsageModelSearchResult = Schema.Struct({
  models: Schema.Array(UsageCatalogModel),
});
export type UsageModelSearchResult = typeof UsageModelSearchResult.Type;

export const UsageSummary = Schema.Struct({
  contractVersion: Schema.Number,
  readAt: Schema.String,
  timeZone: TrimmedNonEmptyString,
  sinceDay: UsageDay,
  untilDay: UsageDay,
  buckets: Schema.Array(UsageBucket),
  sources: Schema.Array(UsageSource),
  pricing: UsagePricing,
  /** Wall-clock cost of the scan, surfaced in diagnostics. */
  scanDurationMs: NonNegativeInt,
});
export type UsageSummary = typeof UsageSummary.Type;

export class UsageReadError extends Schema.TaggedErrorClass<UsageReadError>()("UsageReadError", {
  reason: Schema.Literals(["scanFailed", "invalidWindow"]),
  /** Stable, bounded description. The underlying failure travels in `cause`. */
  detail: TrimmedNonEmptyString,
  cause: Schema.optional(Schema.Defect()),
}) {
  override get message(): string {
    return `Usage read failed (${this.reason}): ${this.detail}`;
  }
}
