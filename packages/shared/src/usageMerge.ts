/**
 * Merges per-environment usage summaries into the single view the page renders.
 *
 * Pure, so the de-duplication and derivation rules can be tested without a
 * connected environment.
 *
 * @module usageMerge
 */
import type {
  EnvironmentId,
  UsageBucket,
  UsageProviderKind,
  UsageSourceFingerprint,
  UsageSummary,
} from "@t3tools/contracts";

export interface EnvironmentUsage {
  readonly environmentId: EnvironmentId;
  readonly label: string;
  readonly summary: UsageSummary;
}

export interface ProviderTotals {
  readonly provider: UsageProviderKind;
  readonly costUsd: number;
  readonly totalTokens: number;
  readonly records: number;
  readonly sessions: number;
  readonly costShare: number;
  readonly tokenShare: number;
}

export interface ModelTotals {
  readonly model: string;
  readonly provider: UsageProviderKind;
  /**
   * The upstream gateway, when one was named and this row represents a single
   * one. Empty once several gateways consolidate into one tagged model.
   */
  readonly apiProvider: string;
  /** The catalog entry this row was priced against, when any. */
  readonly pricedAs: string | null;
  /** True when no rate could be found, which is what makes a row worth tagging. */
  readonly unpriced: boolean;
  /**
   * True when this row exists because the user tagged it. Distinct from
   * `unpriced`: a tagged row is priced, but still needs the affordance so the
   * tag can be changed or removed.
   */
  readonly tagged: boolean;
  /** True when input/cache for any contributing record was simulated. */
  readonly inputTokensEstimated: boolean;
  readonly costUsd: number;
  readonly totalTokens: number;
  readonly records: number;
  readonly costShare: number;
}

export interface DailyTotals {
  readonly day: string;
  readonly costUsd: number;
  readonly totalTokens: number;
  readonly byProvider: ReadonlyMap<UsageProviderKind, { costUsd: number; totalTokens: number }>;
}

export interface HourlyTotals {
  readonly day: string;
  readonly hourStart: string;
  readonly costUsd: number;
  readonly totalTokens: number;
  readonly byProvider: ReadonlyMap<UsageProviderKind, { costUsd: number; totalTokens: number }>;
}

export interface CostQuality {
  readonly providerReportedShare: number;
  readonly modelPricedShare: number;
  readonly unpricedShare: number;
  readonly cacheSavingsUsd: number;
}

export interface MergedUsage {
  readonly costUsd: number;
  readonly inputTokensEstimated: boolean;
  readonly uncachedInputTokens: number;
  readonly cachedInputTokens: number;
  readonly cacheCreationTokens: number;
  readonly outputTokens: number;
  readonly reasoningTokens: number;
  readonly totalTokens: number;
  readonly records: number;
  readonly sessions: number;
  readonly providers: readonly ProviderTotals[];
  readonly models: readonly ModelTotals[];
  readonly daily: readonly DailyTotals[];
  readonly hourly: readonly HourlyTotals[];
  readonly costQuality: CostQuality;
  /** Environments whose data was dropped as a duplicate of another's. */
  readonly duplicateSources: readonly string[];
  readonly contributingEnvironments: readonly EnvironmentId[];
  readonly staleEnvironments: readonly EnvironmentId[];
}

/**
 * Two transcript sources are the same physical directory only when host,
 * provider, path and filesystem identity all agree. Cursor sources are
 * account-wide, so their hashed account identity matches across hosts.
 *
 * `volumeId` is what stops two machines that happen to share a hostname and a
 * home path, which is every Mac in a fleet, from collapsing into one source and
 * having one of them silently dropped.
 */
function fingerprintKey(fingerprint: UsageSourceFingerprint): string {
  // Cursor's dashboard is account-wide rather than machine-local. Two remote
  // environments signed into the same account therefore own one source even
  // when their hostnames differ; the hashed account id is in resolvedHomePath.
  if (fingerprint.provider === "cursor") {
    return [fingerprint.provider, fingerprint.resolvedHomePath].join("\u0000");
  }
  return [
    fingerprint.hostId,
    fingerprint.provider,
    fingerprint.resolvedHomePath,
    fingerprint.volumeId,
  ].join(" ");
}

/**
 * Decides which environment owns each physical transcript directory.
 *
 * Several environments on one machine (worktree servers, for instance) resolve
 * the same provider home and would otherwise double count every token. The
 * first environment in a stable order claims a fingerprint; the rest have that
 * provider's buckets dropped. Environments are sorted by id so the winner does
 * not change between renders.
 */
function claimSources(environments: readonly EnvironmentUsage[]): {
  readonly ownerByFingerprint: ReadonlyMap<string, EnvironmentId>;
  readonly duplicates: readonly string[];
} {
  const ownerByFingerprint = new Map<string, EnvironmentId>();
  const duplicates: string[] = [];

  const ordered = [...environments].sort((a, b) => a.environmentId.localeCompare(b.environmentId));

  for (const environment of ordered) {
    for (const source of environment.summary.sources) {
      if (source.status === "missing") continue;
      const key = fingerprintKey(source.fingerprint);
      if (ownerByFingerprint.has(key)) {
        duplicates.push(`${environment.label}: ${source.fingerprint.resolvedHomePath}`);
        continue;
      }
      ownerByFingerprint.set(key, environment.environmentId);
    }
  }

  return { ownerByFingerprint, duplicates };
}

/** Sources this environment owns after fingerprint claims, plus their buckets. */
function ownedContribution(
  environment: EnvironmentUsage,
  ownerByFingerprint: ReadonlyMap<string, EnvironmentId>,
): {
  readonly buckets: readonly UsageBucket[];
  readonly sessionsByProvider: ReadonlyMap<UsageProviderKind, number>;
} {
  const ownedProviders = new Set<UsageProviderKind>();
  const sessionsByProvider = new Map<UsageProviderKind, number>();
  for (const source of environment.summary.sources) {
    if (source.status === "missing") continue;
    const key = fingerprintKey(source.fingerprint);
    if (ownerByFingerprint.get(key) === environment.environmentId) {
      const provider = source.fingerprint.provider;
      ownedProviders.add(provider);
      // Distinct within a directory. Summing per-bucket session counts instead
      // would count a session once per day and model it spans.
      sessionsByProvider.set(
        provider,
        (sessionsByProvider.get(provider) ?? 0) + source.distinctSessions,
      );
    }
  }
  return {
    buckets: environment.summary.buckets.filter((bucket) => ownedProviders.has(bucket.provider)),
    sessionsByProvider,
  };
}

function bucketTokens(bucket: UsageBucket): number {
  // reasoningTokens is a subset of outputTokens and must not be added again.
  return (
    bucket.totals.uncachedInputTokens +
    bucket.totals.cachedInputTokens +
    bucket.totals.cacheCreationTokens +
    bucket.totals.outputTokens
  );
}

const EMPTY_MERGED: MergedUsage = {
  costUsd: 0,
  inputTokensEstimated: false,
  uncachedInputTokens: 0,
  cachedInputTokens: 0,
  cacheCreationTokens: 0,
  outputTokens: 0,
  reasoningTokens: 0,
  totalTokens: 0,
  records: 0,
  sessions: 0,
  providers: [],
  models: [],
  daily: [],
  hourly: [],
  costQuality: {
    providerReportedShare: 0,
    modelPricedShare: 0,
    unpricedShare: 0,
    cacheSavingsUsd: 0,
  },
  duplicateSources: [],
  contributingEnvironments: [],
  staleEnvironments: [],
};

/**
 * Merges every connected environment's summary.
 *
 * `expectedContractVersion` guards against an environment running older server
 * code: rather than blocking the page, its data is excluded and its id is
 * reported so the UI can say coverage is partial.
 */
export function mergeUsage(
  environments: readonly EnvironmentUsage[],
  expectedContractVersion: number,
): MergedUsage {
  if (environments.length === 0) return EMPTY_MERGED;

  const current: EnvironmentUsage[] = [];
  const staleEnvironments: EnvironmentId[] = [];
  for (const environment of environments) {
    if (environment.summary.contractVersion === expectedContractVersion) {
      current.push(environment);
    } else {
      staleEnvironments.push(environment.environmentId);
    }
  }

  const { ownerByFingerprint, duplicates } = claimSources(current);

  let costUsd = 0;
  let inputTokensEstimated = false;
  let uncachedInputTokens = 0;
  let cachedInputTokens = 0;
  let cacheCreationTokens = 0;
  let outputTokens = 0;
  let reasoningTokens = 0;
  let records = 0;
  let sessions = 0;
  let cacheSavingsUsd = 0;
  let providerReportedRecords = 0;
  let unpricedRecords = 0;

  const providerAccumulator = new Map<
    UsageProviderKind,
    { costUsd: number; totalTokens: number; records: number; sessions: number }
  >();
  const modelAccumulator = new Map<
    string,
    {
      provider: UsageProviderKind;
      model: string;
      apiProviders: Set<string>;
      pricedAs: string | null;
      unpricedOnly: boolean;
      tagged: boolean;
      inputTokensEstimated: boolean;
      costUsd: number;
      totalTokens: number;
      records: number;
    }
  >();
  const dailyAccumulator = new Map<
    string,
    {
      costUsd: number;
      totalTokens: number;
      byProvider: Map<UsageProviderKind, { costUsd: number; totalTokens: number }>;
    }
  >();
  const hourlyAccumulator = new Map<
    string,
    {
      day: string;
      hourStart: string;
      costUsd: number;
      totalTokens: number;
      byProvider: Map<UsageProviderKind, { costUsd: number; totalTokens: number }>;
    }
  >();
  const contributingEnvironments: EnvironmentId[] = [];

  for (const environment of current) {
    const { buckets, sessionsByProvider } = ownedContribution(environment, ownerByFingerprint);
    if (buckets.length > 0) contributingEnvironments.push(environment.environmentId);

    for (const [providerKind, providerSessions] of sessionsByProvider) {
      sessions += providerSessions;
      if (providerSessions === 0) continue;
      const provider = providerAccumulator.get(providerKind) ?? {
        costUsd: 0,
        totalTokens: 0,
        records: 0,
        sessions: 0,
      };
      provider.sessions += providerSessions;
      providerAccumulator.set(providerKind, provider);
    }

    for (const bucket of buckets) {
      const tokens = bucketTokens(bucket);

      costUsd += bucket.costUsd;
      inputTokensEstimated ||= bucket.inputTokensEstimated;
      cacheSavingsUsd += bucket.cacheSavingsUsd;
      uncachedInputTokens += bucket.totals.uncachedInputTokens;
      cachedInputTokens += bucket.totals.cachedInputTokens;
      cacheCreationTokens += bucket.totals.cacheCreationTokens;
      outputTokens += bucket.totals.outputTokens;
      reasoningTokens += bucket.totals.reasoningTokens;
      records += bucket.records;
      unpricedRecords += bucket.unpricedRecords;
      if (bucket.costSource === "providerReported") providerReportedRecords += bucket.records;

      const provider = providerAccumulator.get(bucket.provider) ?? {
        costUsd: 0,
        totalTokens: 0,
        records: 0,
        sessions: 0,
      };
      provider.costUsd += bucket.costUsd;
      provider.totalTokens += tokens;
      provider.records += bucket.records;
      providerAccumulator.set(bucket.provider, provider);

      // A tagged bucket keys by what the user said it is, so the same model
      // reported under several gateway names collapses into one row. Everything
      // else keeps its own identity, which is what keeps it taggable.
      const modelKey =
        bucket.costSource === "userTagged" && bucket.pricedAs !== null
          ? `${bucket.provider}\u0000tag\u0000${bucket.pricedAs}`
          : `${bucket.provider}\u0000${bucket.apiProvider}\u0000${bucket.model}`;
      const model = modelAccumulator.get(modelKey) ?? {
        provider: bucket.provider,
        model:
          bucket.costSource === "userTagged" && bucket.pricedAs !== null
            ? bucket.pricedAs
            : bucket.model,
        apiProviders: new Set<string>(),
        pricedAs: bucket.pricedAs,
        unpricedOnly: true,
        tagged: false,
        inputTokensEstimated: false,
        costUsd: 0,
        totalTokens: 0,
        records: 0,
      };
      if (bucket.apiProvider.length > 0) model.apiProviders.add(bucket.apiProvider);
      if (bucket.costSource !== "unpriced") model.unpricedOnly = false;
      if (bucket.costSource === "userTagged") model.tagged = true;
      model.inputTokensEstimated ||= bucket.inputTokensEstimated;
      model.costUsd += bucket.costUsd;
      model.totalTokens += tokens;
      model.records += bucket.records;
      modelAccumulator.set(modelKey, model);

      const day = dailyAccumulator.get(bucket.day) ?? {
        costUsd: 0,
        totalTokens: 0,
        byProvider: new Map<UsageProviderKind, { costUsd: number; totalTokens: number }>(),
      };
      day.costUsd += bucket.costUsd;
      day.totalTokens += tokens;
      const dayProvider = day.byProvider.get(bucket.provider) ?? { costUsd: 0, totalTokens: 0 };
      dayProvider.costUsd += bucket.costUsd;
      dayProvider.totalTokens += tokens;
      day.byProvider.set(bucket.provider, dayProvider);
      dailyAccumulator.set(bucket.day, day);

      if (bucket.hourStart !== undefined) {
        const hour = hourlyAccumulator.get(bucket.hourStart) ?? {
          day: bucket.day,
          hourStart: bucket.hourStart,
          costUsd: 0,
          totalTokens: 0,
          byProvider: new Map<UsageProviderKind, { costUsd: number; totalTokens: number }>(),
        };
        hour.costUsd += bucket.costUsd;
        hour.totalTokens += tokens;
        const hourProvider = hour.byProvider.get(bucket.provider) ?? {
          costUsd: 0,
          totalTokens: 0,
        };
        hourProvider.costUsd += bucket.costUsd;
        hourProvider.totalTokens += tokens;
        hour.byProvider.set(bucket.provider, hourProvider);
        hourlyAccumulator.set(bucket.hourStart, hour);
      }
    }
  }

  const totalTokens = uncachedInputTokens + cachedInputTokens + cacheCreationTokens + outputTokens;

  const providers: ProviderTotals[] = [...providerAccumulator.entries()]
    .map(([provider, totals]) => ({
      provider,
      costUsd: totals.costUsd,
      totalTokens: totals.totalTokens,
      records: totals.records,
      sessions: totals.sessions,
      costShare: costUsd === 0 ? 0 : totals.costUsd / costUsd,
      tokenShare: totalTokens === 0 ? 0 : totals.totalTokens / totalTokens,
    }))
    .sort((a, b) => b.costUsd - a.costUsd);

  const models: ModelTotals[] = [...modelAccumulator.values()]
    .map((totals) => ({
      model: totals.model,
      provider: totals.provider,
      // Only meaningful when exactly one gateway fed the row: naming one of
      // several would be a lie, and naming none of one loses the distinction.
      apiProvider: totals.apiProviders.size === 1 ? [...totals.apiProviders][0]! : "",
      pricedAs: totals.pricedAs,
      unpriced: totals.unpricedOnly,
      tagged: totals.tagged,
      inputTokensEstimated: totals.inputTokensEstimated,
      costUsd: totals.costUsd,
      totalTokens: totals.totalTokens,
      records: totals.records,
      costShare: costUsd === 0 ? 0 : totals.costUsd / costUsd,
    }))
    .sort((a, b) => b.costUsd - a.costUsd || b.totalTokens - a.totalTokens);

  const daily: DailyTotals[] = [...dailyAccumulator.entries()]
    .map(([day, totals]) => ({
      day,
      costUsd: totals.costUsd,
      totalTokens: totals.totalTokens,
      byProvider: totals.byProvider,
    }))
    .sort((a, b) => a.day.localeCompare(b.day));

  const hourly: HourlyTotals[] = [...hourlyAccumulator.values()].sort((a, b) =>
    a.hourStart.localeCompare(b.hourStart),
  );

  return {
    costUsd,
    inputTokensEstimated,
    uncachedInputTokens,
    cachedInputTokens,
    cacheCreationTokens,
    outputTokens,
    reasoningTokens,
    totalTokens,
    records,
    sessions,
    providers,
    models,
    daily,
    hourly,
    costQuality: {
      providerReportedShare: records === 0 ? 0 : providerReportedRecords / records,
      unpricedShare: records === 0 ? 0 : unpricedRecords / records,
      modelPricedShare:
        records === 0 ? 0 : (records - providerReportedRecords - unpricedRecords) / records,
      cacheSavingsUsd,
    },
    duplicateSources: duplicates,
    contributingEnvironments,
    staleEnvironments,
  };
}
