/**
 * UsageService - scans provider transcripts and returns priced daily usage.
 *
 * The scan reads the provider CLIs' own session files rather than T3 Code's
 * orchestration projections, so usage covers turns driven outside T3 Code too.
 * This is the approach `ccusage` takes.
 *
 * Transcripts are append-only, so parsed records are memoised per file by
 * `(size, mtime)`. A cold 30-day scan of ~1.4 GB lands around 2-3 seconds; warm
 * scans only reparse files that changed.
 *
 * @module UsageService
 */
import * as NodeOS from "node:os";

import {
  USAGE_CONTRACT_VERSION,
  type UsageCatalogModel,
  type UsageModelSearchInput,
  type UsageModelSearchResult,
  type UsageProviderKind,
  type UsageSource,
  type UsageSummary,
  type UsageSummaryInput,
  UsageReadError,
} from "@t3tools/contracts";
import * as Cause from "effect/Cause";
import * as Clock from "effect/Clock";
import * as Context from "effect/Context";
import * as DateTime from "effect/DateTime";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";
import { HttpClient, HttpClientResponse } from "effect/unstable/http";

import { ServerConfig } from "../config.ts";
import * as ServerSettings from "../serverSettings.ts";
import { resolveClaudeHomePath } from "../provider/Drivers/ClaudeHome.ts";
import { resolveCodexHomeLayout } from "../provider/Drivers/CodexHomeLayout.ts";
import { expandHomePath } from "../pathExpansion.ts";
import { UsageAggregator } from "./usageAggregation.ts";
import {
  EMPTY_CATALOG,
  parseModelCatalog,
  searchCatalog,
  type ModelCatalog,
} from "./usageModelCatalog.ts";
import { parseRateTable, UsagePricer, type RateTable } from "./usagePricing.ts";
import {
  listTranscriptFiles,
  readDirectoryVolumeId,
  readTranscriptRecords,
} from "./usageTranscriptReader.ts";
import {
  decodeScanCache,
  dedupeWithinFile,
  encodeScanCache,
  pruneScanCache,
  type ScanCache,
} from "./usageScanCache.ts";
import type { UsageRecord } from "./usageTranscripts.ts";

const LITELLM_RATES_URL =
  "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

/**
 * models.dev's catalog, which is provider-scoped and therefore the only one
 * that can price a gateway's model names. Used for pi and for every user tag.
 */
const MODELS_DEV_CATALOG_URL = "https://models.dev/api.json";

/** Most a catalog search returns. The picker is a list, not a database browser. */
const MODEL_SEARCH_LIMIT = 50;

/** Rates move rarely; a day-old table keeps the page working offline. */
const RATES_TTL_MS = 24 * 60 * 60 * 1000;

/**
 * Files are filtered by mtime before opening. The slack covers a session whose
 * last write lands just before local midnight on the window's first day.
 */
const MTIME_SLACK_MS = 36 * 60 * 60 * 1000;

/** Longest window the UI offers, plus slack. Older entries are pruned. */
const CACHE_RETENTION_DAYS = 90;

/** On-disk shape of the rate snapshot. */
const RatesCacheFile = Schema.Struct({
  fetchedAtMs: Schema.Number,
  document: Schema.Unknown,
});
const decodeRatesCache = Schema.decodeUnknownEffect(
  Schema.fromJsonString(RatesCacheFile as unknown as Schema.Codec<typeof RatesCacheFile.Type>),
);
const encodeRatesCache = Schema.encodeEffect(
  Schema.fromJsonString(RatesCacheFile as unknown as Schema.Codec<typeof RatesCacheFile.Type>),
);

/** The scan cache is narrowed by hand in `usageScanCache`, so JSON is enough here. */
const ScanCacheJson = Schema.fromJsonString(Schema.Unknown as unknown as Schema.Codec<unknown>);
const decodeScanCacheFile = Schema.decodeUnknownEffect(ScanCacheJson);
const encodeScanCacheFile = Schema.encodeEffect(ScanCacheJson);

export class UsageService extends Context.Service<
  UsageService,
  {
    readonly readSummary: (input: UsageSummaryInput) => Effect.Effect<UsageSummary, UsageReadError>;
    readonly searchModels: (
      input: UsageModelSearchInput,
    ) => Effect.Effect<UsageModelSearchResult, UsageReadError>;
  }
>()("t3/usage/UsageService") {}

/** Empty summary, for suites that only need the RPC surface to resolve. */
export const layerTest = Layer.succeed(
  UsageService,
  UsageService.of({
    readSummary: (input) =>
      Effect.succeed({
        contractVersion: USAGE_CONTRACT_VERSION,
        readAt: "1970-01-01T00:00:00.000Z",
        timeZone: input.timeZone,
        sinceDay: input.sinceDay,
        untilDay: input.untilDay,
        buckets: [],
        sources: [],
        pricing: {
          status: "unavailable",
          source: LITELLM_RATES_URL,
          fetchedAt: null,
          knownModels: 0,
        },
        scanDurationMs: 0,
      }),
    searchModels: () => Effect.succeed({ models: [] }),
  }),
);

export const make = Effect.gen(function* () {
  const fileSystem = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const config = yield* ServerConfig;
  const settingsService = yield* ServerSettings.ServerSettingsService;
  const httpClient = yield* HttpClient.HttpClient;

  const fileCache: ScanCache = new Map();
  let cacheDirty = false;

  const ratesCachePath = path.join(config.stateDir, "usage-model-rates.json");
  const catalogCachePath = path.join(config.stateDir, "usage-model-catalog.json");
  const scanCachePath = path.join(config.stateDir, "usage-scan-cache.json");
  let rates: RateTable = new Map();
  let ratesFetchedAtMs: number | null = null;
  let ratesStatus: UsageSummary["pricing"]["status"] = "unavailable";
  let catalog: ModelCatalog = EMPTY_CATALOG;
  let catalogFetchedAtMs: number | null = null;
  let catalogStatus: UsageSummary["pricing"]["status"] = "unavailable";

  const loadCachedRates = Effect.fn("UsageService.loadCachedRates")(function* () {
    if (ratesFetchedAtMs !== null) return;
    const fromDisk = yield* fileSystem.readFileString(ratesCachePath).pipe(
      Effect.flatMap((raw) => decodeRatesCache(raw)),
      Effect.catchCause(() => Effect.succeed(null)),
    );
    if (fromDisk !== null) {
      const parsed = parseRateTable(fromDisk.document);
      if (parsed.size > 0) {
        rates = parsed;
        ratesFetchedAtMs = fromDisk.fetchedAtMs;
        ratesStatus = "cached";
      }
    }
  });

  /**
   * Same shape as the rates cache, but the models.dev document. The two share
   * a TTL but live in separate files so a corrupted one cannot drag the other
   * down with it.
   */
  const loadCachedCatalog = Effect.fn("UsageService.loadCachedCatalog")(function* () {
    if (catalogFetchedAtMs !== null) return;
    const fromDisk = yield* fileSystem.readFileString(catalogCachePath).pipe(
      Effect.flatMap((raw) => decodeRatesCache(raw)),
      Effect.catchCause(() => Effect.succeed(null)),
    );
    if (fromDisk !== null) {
      const parsed = parseModelCatalog(fromDisk.document);
      if (parsed.size > 0) {
        catalog = parsed;
        catalogFetchedAtMs = fromDisk.fetchedAtMs;
        catalogStatus = "cached";
      }
    }
  });

  const fetchRates = Effect.fn("UsageService.fetchRates")(function* () {
    const fetched = yield* httpClient.get(LITELLM_RATES_URL).pipe(
      Effect.flatMap(HttpClientResponse.filterStatusOk),
      Effect.flatMap((response) => response.json),
      Effect.timeout(10_000),
      Effect.catchCause(() => Effect.succeed(null)),
    );
    if (fetched === null) return null;

    const parsed = parseRateTable(fetched);
    if (parsed.size === 0) return null;
    return { document: fetched, parsed };
  });

  const fetchCatalog = Effect.fn("UsageService.fetchCatalog")(function* () {
    const fetched = yield* httpClient.get(MODELS_DEV_CATALOG_URL).pipe(
      Effect.flatMap(HttpClientResponse.filterStatusOk),
      Effect.flatMap((response) => response.json),
      Effect.timeout(10_000),
      Effect.catchCause(() => Effect.succeed(null)),
    );
    if (fetched === null) return null;

    const parsed = parseModelCatalog(fetched);
    if (parsed.size === 0) return null;
    return { document: fetched, parsed };
  });

  const persistRates = Effect.fn("UsageService.persistRates")(function* (
    cachePath: string,
    fetchedAtMs: number,
    document: unknown,
  ) {
    // A snapshot we cannot write is a slower next start, not a failed read.
    yield* encodeRatesCache({ fetchedAtMs, document }).pipe(
      Effect.flatMap((serialized) => fileSystem.writeFileString(cachePath, serialized)),
      Effect.catchCause(() => Effect.void),
    );
  });

  /**
   * Loads the LiteLLM rate table, preferring a fresh copy and falling back to
   * the on-disk snapshot. With neither, every model reports as unpriced rather
   * than the page failing.
   */
  const ensureRates = Effect.fn("UsageService.ensureRates")(function* () {
    const now = yield* Clock.currentTimeMillis;
    if (ratesFetchedAtMs !== null && now - ratesFetchedAtMs < RATES_TTL_MS) return;

    if (ratesFetchedAtMs === null) yield* loadCachedRates();
    if (ratesFetchedAtMs !== null && now - ratesFetchedAtMs < RATES_TTL_MS) return;

    const fetched = yield* fetchRates();
    if (fetched === null) {
      // The refresh failed; whatever we are serving is now past its TTL and
      // must not keep claiming to be fresh.
      if (rates.size > 0) ratesStatus = "cached";
      return;
    }

    rates = fetched.parsed;
    ratesFetchedAtMs = now;
    ratesStatus = "fresh";

    yield* persistRates(ratesCachePath, now, fetched.document);
  });

  /** Same lifecycle as {@link ensureRates}, for the models.dev catalog. */
  const ensureCatalog = Effect.fn("UsageService.ensureCatalog")(function* () {
    const now = yield* Clock.currentTimeMillis;
    if (catalogFetchedAtMs !== null && now - catalogFetchedAtMs < RATES_TTL_MS) return;

    if (catalogFetchedAtMs === null) yield* loadCachedCatalog();
    if (catalogFetchedAtMs !== null && now - catalogFetchedAtMs < RATES_TTL_MS) return;

    const fetched = yield* fetchCatalog();
    if (fetched === null) {
      if (catalog.size > 0) catalogStatus = "cached";
      return;
    }

    catalog = fetched.parsed;
    catalogFetchedAtMs = now;
    catalogStatus = "fresh";

    yield* persistRates(catalogCachePath, now, fetched.document);
  });

  /**
   * Claude's config dir is the home itself when overridden, but a default
   * install nests transcripts under `~/.claude/projects`. Probe both.
   */
  const resolveClaudeTranscriptDir = (homePath: string) =>
    Effect.gen(function* () {
      const nested = path.join(homePath, ".claude", "projects");
      const nestedExists = yield* fileSystem
        .exists(nested)
        .pipe(Effect.catchCause(() => Effect.succeed(false)));
      return nestedExists ? nested : path.join(homePath, "projects");
    });

  /**
   * pi's agent directory, which holds `sessions/` alongside its settings and
   * credentials. Mirrors `PiLaunch.makePiEnvironment`: the configured
   * `agentDirPath` is what a spawned pi would see as `PI_CODING_AGENT_DIR`, and
   * `~/.pi/agent` is pi's own default.
   */
  const resolvePiAgentDir = (piSettings: { readonly agentDirPath: string }): string => {
    const agentDirPath = piSettings.agentDirPath.trim();
    return agentDirPath.length > 0
      ? path.resolve(expandHomePath(agentDirPath))
      : path.join(NodeOS.homedir(), ".pi", "agent");
  };

  /** Resolves the transcript directory for each provider. */
  const resolveTranscriptDirs = Effect.fn("UsageService.resolveTranscriptDirs")(function* () {
    // A settings failure must surface as an error: swallowing it here would
    // present "zero usage from every provider" as a valid answer.
    const settings = yield* settingsService.getSettings.pipe(
      Effect.catchCause(
        (cause) =>
          new UsageReadError({
            reason: "scanFailed",
            // Bounded description; the squashed failure travels as the cause.
            // Squashed, not the Cause tree: a full tree in a Defect field is
            // the unbounded wire payload the bounded detail exists to avoid.
            detail: "Server settings could not be read.",
            cause: Cause.squash(cause),
          }),
      ),
    );

    const claudeHome = yield* resolveClaudeHomePath(settings.providers.claudeAgent);
    const claudeDir = yield* resolveClaudeTranscriptDir(claudeHome);
    const codexLayout = yield* resolveCodexHomeLayout(settings.providers.codex);

    return [
      { provider: "claude" as const, dir: claudeDir },
      { provider: "codex" as const, dir: path.join(codexLayout.sharedHomePath, "sessions") },
      {
        provider: "pi" as const,
        dir: path.join(resolvePiAgentDir(settings.providers.pi), "sessions"),
      },
    ];
  });

  /**
   * Loads the persisted scan cache exactly once per process.
   *
   * `Effect.cached` makes concurrent first readers await the same load rather
   * than each seeing a "loaded" flag set before the read finished and cold
   * scanning against an empty cache.
   */
  const ensureScanCacheLoaded = yield* Effect.cached(
    Effect.gen(function* () {
      const document = yield* fileSystem.readFileString(scanCachePath).pipe(
        Effect.flatMap((raw) => decodeScanCacheFile(raw)),
        Effect.catchCause(() => Effect.succeed(null)),
      );
      if (document === null) return;
      for (const [path, entry] of decodeScanCache(document)) fileCache.set(path, entry);
    }),
  );

  const persistScanCache = Effect.fn("UsageService.persistScanCache")(function* () {
    if (!cacheDirty) return;
    // Cleared only after the write lands, so a failed persist is retried on
    // the next scan instead of leaving disk permanently stale.
    yield* encodeScanCacheFile(encodeScanCache(fileCache)).pipe(
      Effect.flatMap((serialized) => fileSystem.writeFileString(scanCachePath, serialized)),
      Effect.map(() => {
        cacheDirty = false;
      }),
      // A cache we cannot write is a slower next start, not a failed read.
      Effect.catchCause(() => Effect.void),
    );
  });

  /** Parses one transcript, reusing the cached result when it is unchanged. */
  const readFileRecords = (
    filePath: string,
    size: number,
    mtimeMs: number,
    provider: UsageProviderKind,
  ): Effect.Effect<readonly UsageRecord[]> =>
    Effect.gen(function* () {
      const cached = fileCache.get(filePath);
      // Provider is part of the identity: if both providers were ever pointed
      // at one directory, a hit parsed by the other parser must not be reused.
      if (
        cached &&
        cached.size === size &&
        cached.mtimeMs === mtimeMs &&
        cached.provider === provider
      ) {
        return cached.records;
      }

      const parsed = yield* Effect.promise(() => readTranscriptRecords(filePath, provider));
      // A read failure is not an empty transcript: caching it under this
      // (size, mtime) would silently drop the file's usage until it changes.
      if (parsed === null) return [];
      // Stored already de-duplicated within the file, which is 99% of all
      // duplicates. The aggregator still runs the cross-file dedupe pass.
      const records = dedupeWithinFile(parsed);

      fileCache.set(filePath, { size, mtimeMs, provider, records });
      cacheDirty = true;
      return records;
    });

  const readSummary = Effect.fn("UsageService.readSummary")(function* (input: UsageSummaryInput) {
    if (input.sinceDay > input.untilDay) {
      return yield* new UsageReadError({
        reason: "invalidWindow",
        detail: `sinceDay '${input.sinceDay}' is after untilDay '${input.untilDay}'`,
      });
    }

    const startedAtMs = yield* Clock.currentTimeMillis;
    yield* ensureRates();
    yield* ensureCatalog();
    yield* ensureScanCacheLoaded;

    const hostId = NodeOS.hostname();
    // The home resolvers ask for `Path` themselves; satisfy them from the
    // instance we already hold so `readSummary` stays context-free.
    const dirs = yield* resolveTranscriptDirs().pipe(Effect.provideService(Path.Path, path));
    const windowStart = DateTime.make(`${input.sinceDay}T00:00:00Z`);
    if (Option.isNone(windowStart)) {
      return yield* new UsageReadError({
        reason: "invalidWindow",
        detail: `sinceDay '${input.sinceDay}' is not a valid date`,
      });
    }
    const windowStartMs = DateTime.toEpochMillis(windowStart.value) - MTIME_SLACK_MS;

    const aggregator = new UsageAggregator({
      timeZone: input.timeZone,
      sinceDay: input.sinceDay,
      untilDay: input.untilDay,
      pricer: new UsagePricer({ rates, catalog, aliases: input.modelAliases }),
    });

    const sources: UsageSource[] = [];
    const livePaths = new Set<string>();
    const walkedRoots: string[] = [];

    for (const { provider, dir } of dirs) {
      const volumeId = yield* Effect.promise(() => readDirectoryVolumeId(dir));
      const exists = yield* fileSystem
        .exists(dir)
        .pipe(Effect.catchCause(() => Effect.succeed(false)));

      if (!exists) {
        sources.push({
          fingerprint: { hostId, provider, resolvedHomePath: dir, volumeId },
          status: "missing",
          scannedFiles: 0,
          skippedFiles: 0,
          malformedRecords: 0,
          distinctSessions: 0,
          message: "No transcript directory on this environment.",
        });
        continue;
      }

      walkedRoots.push(dir);
      const files = yield* Effect.promise(() => listTranscriptFiles(dir, windowStartMs));
      let scannedFiles = 0;
      let skippedFiles = 0;
      // Distinct per directory. Buckets carry per-cell session counts, but a
      // session spans days and models, so clients total this figure instead.
      const sessionIds = new Set<string>();

      for (const file of files) {
        livePaths.add(file.path);
        const records = yield* readFileRecords(file.path, file.size, file.mtimeMs, provider);
        if (records.length === 0) {
          skippedFiles += 1;
          continue;
        }
        scannedFiles += 1;
        for (const record of records) {
          // Only sessions that contributed in-window count: the mtime slack
          // admits boundary files whose records fall outside the range.
          if (aggregator.add(record) && record.sessionId.length > 0) {
            sessionIds.add(record.sessionId);
          }
        }
      }

      sources.push({
        fingerprint: { hostId, provider, resolvedHomePath: dir, volumeId },
        status: "ok",
        scannedFiles,
        skippedFiles,
        malformedRecords: 0,
        distinctSessions: sessionIds.size,
        message: null,
      });
    }

    const pruned = pruneScanCache(fileCache, {
      livePaths,
      walkedRoots,
      windowStartMs,
      retentionCutoffMs: startedAtMs - CACHE_RETENTION_DAYS * 24 * 60 * 60 * 1000,
    });
    if (pruned > 0) cacheDirty = true;
    yield* persistScanCache();

    const aggregated = aggregator.finish();
    const readAt = yield* DateTime.now;
    const finishedAtMs = yield* Clock.currentTimeMillis;

    return {
      contractVersion: USAGE_CONTRACT_VERSION,
      readAt: DateTime.formatIso(readAt),
      timeZone: input.timeZone,
      sinceDay: input.sinceDay,
      untilDay: input.untilDay,
      buckets: aggregated.buckets,
      sources,
      pricing: {
        // Two tables, one figure: a page that priced half its rows from a stale
        // snapshot has stale pricing, so the weaker of the two wins.
        status: weakerPricingStatus(ratesStatus, catalogStatus),
        source: `${LITELLM_RATES_URL} + ${MODELS_DEV_CATALOG_URL}`,
        fetchedAt: formatOldestFetch(ratesFetchedAtMs, catalogFetchedAtMs),
        knownModels: rates.size + catalog.size,
      },
      scanDurationMs: Math.max(0, finishedAtMs - startedAtMs),
    } satisfies UsageSummary;
  });

  const searchModels = Effect.fn("UsageService.searchModels")(function* (
    input: UsageModelSearchInput,
  ) {
    yield* ensureCatalog();
    return {
      models: searchCatalog(catalog, input.query, MODEL_SEARCH_LIMIT).map(
        (model) =>
          ({
            id: model.id,
            providerName: model.providerName,
            modelName: model.modelName,
            inputCostPerMillion: model.inputCostPerMillion,
            outputCostPerMillion: model.outputCostPerMillion,
          }) as UsageCatalogModel,
      ),
    } satisfies UsageModelSearchResult;
  });

  return { readSummary, searchModels } as const;
});

const PRICING_STATUS_RANK: Record<UsageSummary["pricing"]["status"], number> = {
  unavailable: 0,
  cached: 1,
  fresh: 2,
};

function weakerPricingStatus(
  a: UsageSummary["pricing"]["status"],
  b: UsageSummary["pricing"]["status"],
): UsageSummary["pricing"]["status"] {
  return PRICING_STATUS_RANK[a] <= PRICING_STATUS_RANK[b] ? a : b;
}

/** The staler of the two snapshots, which is the age the page can claim. */
function formatOldestFetch(a: number | null, b: number | null): string | null {
  const known = [a, b].filter((value): value is number => value !== null);
  if (known.length === 0) return null;
  return DateTime.formatIso(DateTime.makeUnsafe(Math.min(...known)));
}

export const layer = Layer.effect(UsageService, make);
