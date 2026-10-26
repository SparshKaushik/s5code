/**
 * Usage reporting contract. Environments scan native provider transcripts and
 * databases, including work driven outside S5 Code, like `ccusage`, and return
 * pre-aggregated buckets. Raw transcript records never cross the wire.
 */
import * as Effect from "effect/Effect";
import * as Schema from "effect/Schema";

import {
  ForwardCompatibleArray,
  NonNegativeInt,
  TrimmedNonEmptyString,
  TrimmedString,
} from "./baseSchemas.ts";

/** Bump for incompatible UsageSummary changes; skipped unknown array variants need no bump. */
export const USAGE_CONTRACT_VERSION = 9 as const;

/**
 * Oldest {@link UsageSummary} version a current client still merges. v4-v6
 * buckets decode because `apiProvider` has a decoding default.
 */
export const USAGE_MERGE_COMPATIBLE_SINCE = 4 as const;

export const UsageProviderKind = Schema.Literals([
  "claude",
  "codex",
  "grok",
  "cursor",
  "opencode",
  "antigravity",
  "pi",
  "muse",
]);
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
 * Maps a gateway model name no catalog knows, or that catalogs price
 * differently, to a catalog entry. Tagged records use the target's rates and row.
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
 * Cost source: `providerReported` from the transcript, `modelPriced` from a rate
 * table or override, `userTagged` via a tag, or `unpriced` (tokens counted,
 * cost excluded; worth tagging).
 */
export const UsageCostSource = Schema.Literals([
  "providerReported",
  "modelPriced",
  "userTagged",
  "unpriced",
]);
export type UsageCostSource = typeof UsageCostSource.Type;

/** Sum uncached input, cache reads, and cache writes for input totals; reasoning is already included in output. */
export const UsageTokenTotals = Schema.Struct({
  uncachedInputTokens: NonNegativeInt,
  cachedInputTokens: NonNegativeInt,
  cacheCreationTokens: NonNegativeInt,
  outputTokens: NonNegativeInt,
  reasoningTokens: NonNegativeInt,
});
export type UsageTokenTotals = typeof UsageTokenTotals.Type;

export const UsageCategoryCost = Schema.Struct({
  input: Schema.Number,
  cacheRead: Schema.Number,
  cacheWrite: Schema.Number,
  output: Schema.Number,
});
export type UsageCategoryCost = typeof UsageCategoryCost.Type;

export const UsageBucket = Schema.Struct({
  day: UsageDay,
  hourStart: Schema.optional(TrimmedNonEmptyString),
  provider: UsageProviderKind,
  model: TrimmedNonEmptyString,
  /**
   * Upstream API provider when the source names one; separates reseller and
   * first-party models with the same name. Empty for single-vendor providers and
   * optional on the wire so upstream buckets still decode.
   */
  apiProvider: Schema.String.pipe(Schema.withDecodingDefault(Effect.succeed(""))),
  /** Source directory, so overlapping multi-home environments merge once per source. */
  sourcePath: Schema.optional(TrimmedNonEmptyString),
  totals: UsageTokenTotals,
  costUsd: Schema.Number,
  cacheSavingsUsd: Schema.Number,
  /**
   * True when at least one record's input/cache split was simulated from a
   * rolling context estimate rather than reported as billed tokens.
   */
  inputTokensEstimated: Schema.Boolean.pipe(Schema.withDecodingDefault(Effect.succeed(false))),
  /**
   * `costUsd` by token category. Cost with no known rates stays out of it, and
   * it is absent when nothing could be split or the server predates it.
   */
  categoryCostUsd: Schema.optional(UsageCategoryCost),
  fastCostUsd: Schema.optional(Schema.Number),
  ultrafastCostUsd: Schema.optional(Schema.Number),
  speedPremiumUsd: Schema.optional(Schema.Number),
  costSource: UsageCostSource,
  /**
   * Catalog entry this cell was priced against (`<providerId>/<modelKey>`), or
   * `null` without a catalog, so the UI can show the assumption and allow correction.
   */
  pricedAs: Schema.NullOr(UsageCatalogModelId).pipe(
    Schema.withDecodingDefault(Effect.succeed(null)),
  ),
  /** Distinct assistant responses, after de-duplication. */
  records: NonNegativeInt,
  unpricedRecords: NonNegativeInt,
  sessions: NonNegativeInt,
});
export type UsageBucket = typeof UsageBucket.Type;

/**
 * Physical transcript directory or remote account a source read. Clients drop
 * duplicate fingerprints so same-machine environments do not double count.
 */
export const UsageSourceFingerprint = Schema.Struct({
  hostId: TrimmedNonEmptyString,
  provider: UsageProviderKind,
  resolvedHomePath: TrimmedNonEmptyString,
  /** Transcript directory device:inode distinguishes machines sharing a hostname and home path. */
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
  malformedRecords: NonNegativeInt,
  /** Total this count; bucket counts double-count sessions spanning days or models. */
  distinctSessions: NonNegativeInt,
  message: Schema.NullOr(TrimmedNonEmptyString),
  action: Schema.optionalKey(Schema.Literal("enableCursorKeychain")),
  /** Cached response during refresh; repeat with awaitRefresh for fresh data. */
  refreshing: Schema.optionalKey(Schema.Literal(true)),
});
export type UsageSource = typeof UsageSource.Type;

export const UsagePricingStatus = Schema.Literals(["fresh", "cached", "unavailable"]);
export type UsagePricingStatus = typeof UsagePricingStatus.Type;

export const UsagePricing = Schema.Struct({
  status: UsagePricingStatus,
  source: TrimmedNonEmptyString,
  fetchedAt: Schema.NullOr(Schema.String),
  knownModels: NonNegativeInt,
});
export type UsagePricing = typeof UsagePricing.Type;

export const UsageSummaryInput = Schema.Struct({
  sinceDay: UsageDay,
  untilDay: UsageDay,
  /** IANA bucketing zone; offsets fail across DST boundaries. */
  timeZone: TrimmedNonEmptyString,
  /**
   * The client's model tags. Pricing runs server-side (the catalog is megabytes
   * and never crosses the wire), but the tags belong to the user rather than to
   * one environment, so they travel with the request and apply everywhere.
   */
  modelAliases: Schema.Array(UsageModelAlias).pipe(Schema.withDecodingDefault(Effect.succeed([]))),
  /** Defaults to daily for older clients. */
  resolution: Schema.optional(UsageResolution),
  sinceTime: Schema.optional(TrimmedNonEmptyString),
  untilTime: Schema.optional(TrimmedNonEmptyString),
  awaitRefresh: Schema.optional(Schema.Boolean),
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
  buckets: ForwardCompatibleArray(UsageBucket),
  sources: ForwardCompatibleArray(UsageSource),
  pricing: UsagePricing,
  scanDurationMs: NonNegativeInt,
});
export type UsageSummary = typeof UsageSummary.Type;

export class UsageReadError extends Schema.TaggedError<UsageReadError>()("UsageReadError", {
  reason: Schema.Literals(["scanFailed", "invalidWindow"]),
  detail: TrimmedNonEmptyString,
  cause: Schema.optional(Schema.Defect()),
}) {
  override get message(): string {
    return `Usage read failed (${this.reason}): ${this.detail}`;
  }
}
