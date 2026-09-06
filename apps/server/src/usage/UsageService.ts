/**
 * UsageService - scans provider transcripts and returns priced usage buckets.
 *
 * The scan reads the provider CLIs' own session files (Claude Code, Codex, and
 * Grok Build) rather than T3 Code's orchestration projections, so usage covers
 * turns driven outside T3 Code too. This is the approach `ccusage` takes.
 *
 * Transcripts are append-only, so parsed records are memoised per file by
 * `(size, mtime)`. A cold 30-day scan of ~1.4 GB lands around 2-3 seconds; warm
 * scans only reparse files that changed, and a file that merely grew resumes
 * from its cached parse position so only the appended bytes are read.
 *
 * @module UsageService
 */
import * as NodeOS from "node:os";

import {
  USAGE_CONTRACT_VERSION,
  type ServerSettings as ServerSettingsValue,
  type UsageCatalogModel,
  type UsageModelSearchInput,
  type UsageModelSearchResult,
  type UsagePricing,
  type UsageProviderKind,
  type UsageSource,
  type UsageSummary,
  type UsageSummaryInput,
  UsageReadError,
} from "@t3tools/contracts";
import { HostProcessEnvironment, HostProcessPlatform } from "@t3tools/shared/hostProcess";
import * as Cause from "effect/Cause";
import * as Clock from "effect/Clock";
import * as Context from "effect/Context";
import * as DateTime from "effect/DateTime";
import * as Deferred from "effect/Deferred";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";
import * as Semaphore from "effect/Semaphore";
import { HttpClient, HttpClientRequest, HttpClientResponse } from "effect/unstable/http";

import { ServerConfig } from "../config.ts";
import { expandHomePath } from "../pathExpansion.ts";
import * as ServerSettings from "../serverSettings.ts";
import { resolveClaudeHomePath } from "../provider/Drivers/ClaudeHome.ts";
import { resolveCodexHomeLayout } from "../provider/Drivers/CodexHomeLayout.ts";
import {
  CURSOR_USAGE_MAX_PAGES,
  CURSOR_USAGE_PAGE_SIZE,
  CURSOR_USAGE_URL,
  parseCursorUsageEvent,
  parseCursorUsagePage,
  readCursorAppSession,
  reconcileCursorUsagePages,
  resolveCursorAuthDatabasePath,
  type CursorUsagePage,
} from "./usageCursor.ts";
import { UsageAggregator } from "./usageAggregation.ts";
import type { UsageRecord } from "./usageTranscripts.ts";
import {
  EMPTY_CATALOG,
  parseModelCatalog,
  searchCatalog,
  type ModelCatalog,
} from "./usageModelCatalog.ts";
import {
  createOverrideRateTable,
  parseRateTable,
  UsagePricer,
  type RateTable,
} from "./usagePricing.ts";
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
const LITELLM_RATES_URL =
  "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

/**
 * models.dev's catalog, which is provider-scoped and therefore the only one
 * that can price a gateway's model names. Used for pi and for every user tag.
 */
const MODELS_DEV_CATALOG_URL = "https://models.dev/api.json";

const CURSOR_USAGE_TTL_MS = 60 * 60 * 1000;

/** Most a catalog search returns. The picker is a list, not a database browser. */
const MODEL_SEARCH_LIMIT = 50;

/** Rates move rarely; a day-old table keeps the page working offline. */
const RATES_TTL_MS = 24 * 60 * 60 * 1000;

/** An explicit refresh ignores the TTL, but not a table fetched this recently. */
const RATES_REFRESH_FLOOR_MS = 60 * 1000;

/**
 * Files are filtered by mtime before opening. The slack covers a session whose
 * last write lands just before local midnight on the window's first day.
 */
const MTIME_SLACK_MS = 36 * 60 * 60 * 1000;
const MAX_HOURLY_WINDOW_MS = 24 * 60 * 60 * 1000;

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
    /** Refetches the rate table ahead of its TTL. See `ensureRates`. */
    readonly refreshRates: Effect.Effect<UsagePricing>;
  }
>()("t3/usage/UsageService") {}

const EMPTY_PRICING: UsagePricing = {
  status: "unavailable",
  source: LITELLM_RATES_URL,
  fetchedAt: null,
  knownModels: 0,
};

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
        pricing: EMPTY_PRICING,
        scanDurationMs: 0,
      }),
    searchModels: () => Effect.succeed({ models: [] }),
    refreshRates: Effect.succeed(EMPTY_PRICING),
  }),
);

export const make = Effect.gen(function* () {
  const fileSystem = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const config = yield* ServerConfig;
  const settingsService = yield* ServerSettings.ServerSettingsService;
  const httpClient = yield* HttpClient.HttpClient;
  const hostPlatform = yield* HostProcessPlatform;
  const hostEnvironment = yield* HostProcessEnvironment;

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
  const cursorUsageCache = new Map<
    string,
    {
      readonly fetchedAtMs: number;
      readonly records: readonly UsageRecord[];
      readonly malformedRecords: number;
    }
  >();

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

  // One fetch at a time. A burst of refreshes from several clients waits on
  // the first fetch and then sees a table young enough to skip its own.
  const ratesLock = yield* Semaphore.make(1);

  const pricing = (): UsagePricing => ({
    status: ratesStatus,
    source: LITELLM_RATES_URL,
    fetchedAt:
      ratesFetchedAtMs === null ? null : DateTime.formatIso(DateTime.makeUnsafe(ratesFetchedAtMs)),
    knownModels: rates.size,
  });

  /**
   * Loads the LiteLLM rate table, preferring a fresh copy and falling back to
   * the on-disk snapshot. With neither, every model reports as unpriced rather
   * than the page failing. `force` refetches inside the TTL so a model that
   * LiteLLM added since the last fetch gets priced now.
   */
  const loadRates = Effect.fn("UsageService.loadRates")(function* (force: boolean) {
    const now = yield* Clock.currentTimeMillis;
    const maxAgeMs = force ? RATES_REFRESH_FLOOR_MS : RATES_TTL_MS;
    if (ratesFetchedAtMs !== null && now - ratesFetchedAtMs < maxAgeMs) return;

    if (ratesFetchedAtMs === null) yield* loadCachedRates();
    if (ratesFetchedAtMs !== null && now - ratesFetchedAtMs < maxAgeMs) return;

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

  const ensureRates = (force: boolean) => ratesLock.withPermit(loadRates(force));

  const refreshRates = ensureRates(true).pipe(
    Effect.map(pricing),
    Effect.withSpan("UsageService.refreshRates"),
  );

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

  const resolvePiAgentDir = (piSettings: { readonly agentDirPath: string }): string => {
    const agentDirPath = piSettings.agentDirPath.trim();
    return agentDirPath.length > 0
      ? path.resolve(expandHomePath(agentDirPath))
      : path.join(NodeOS.homedir(), ".pi", "agent");
  };

  // A settings failure must not silently discard custom rates or transcript homes.
  const readSettings = settingsService.getSettings.pipe(
    Effect.catchCause(
      (cause) =>
        new UsageReadError({
          reason: "scanFailed",
          detail: "Server settings could not be read.",
          cause: Cause.squash(cause),
        }),
    ),
  );

  /** Resolves the transcript directory for each provider. */
  const resolveTranscriptDirs = Effect.fn("UsageService.resolveTranscriptDirs")(function* (
    settings: ServerSettingsValue,
  ) {
    const claudeHome = yield* resolveClaudeHomePath(settings.providers.claudeAgent);
    const claudeDir = yield* resolveClaudeTranscriptDir(claudeHome);
    const codexLayout = yield* resolveCodexHomeLayout(settings.providers.codex);
    // Grok Settings only expose the binary path; home is `$GROK_HOME` or `~/.grok`.
    // Empty/whitespace GROK_HOME must fall back: coalescing alone would scan cwd.
    const grokHomeEnv = hostEnvironment["GROK_HOME"]?.trim() ?? "";
    const grokHome =
      grokHomeEnv.length > 0
        ? path.resolve(expandHomePath(grokHomeEnv))
        : path.join(NodeOS.homedir(), ".grok");

    return [
      { provider: "claude" as const, dir: claudeDir },
      { provider: "codex" as const, dir: path.join(codexLayout.sharedHomePath, "sessions") },
      {
        provider: "grok" as const,
        dir: path.join(grokHome, "sessions"),
        fileName: "updates.jsonl",
      },
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

  /**
   * Parses one transcript, reusing the cached result when it is unchanged.
   *
   * A file that only grew re-parses from the cached position, so an actively
   * written multi-hundred-megabyte rollout costs its appended bytes per scan
   * rather than a full re-read. The reader verifies the position's guard bytes
   * and silently restarts from byte 0 when they no longer match.
   */
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
        return cached.tailRecords.length === 0
          ? cached.records
          : [...cached.records, ...cached.tailRecords];
      }

      // Only a strictly grown file may resume. Same size with a new mtime, or
      // a shrunken file, means rewritten content; re-parse it whole.
      const resumeFrom =
        cached !== undefined && cached.provider === provider && size > cached.size
          ? cached.position
          : undefined;

      const parsed = yield* Effect.promise(() =>
        readTranscriptRecords(filePath, provider, resumeFrom),
      );
      // A read failure is not an empty transcript: caching it under this
      // (size, mtime) would silently drop the file's usage until it changes.
      if (parsed === null) return [];

      // Stored already de-duplicated within the file, which is 99% of all
      // duplicates. The aggregator still runs the cross-file dedupe pass. One
      // seen set spans the cached base, the new lines, and the tail so a
      // resumed parse dedupes exactly like a full one.
      const base = parsed.resumed && cached !== undefined ? cached.records : [];
      const seen = new Set<string>();
      const records = dedupeWithinFile([...base, ...parsed.records], seen);
      const tailRecords = dedupeWithinFile(parsed.tailRecords, seen);

      fileCache.set(filePath, {
        size,
        mtimeMs,
        provider,
        records,
        tailRecords,
        position: parsed.position,
      });
      cacheDirty = true;
      return tailRecords.length === 0 ? records : [...records, ...tailRecords];
    });

  const readCursorUsage = Effect.fn("UsageService.readCursorUsage")(
    function* (input: UsageSummaryInput, nowMs: number) {
      const databasePath = resolveCursorAuthDatabasePath(
        hostPlatform,
        NodeOS.homedir(),
        hostEnvironment,
      );
      const session = yield* Effect.promise(() => readCursorAppSession(nowMs, databasePath));
      if (session === null) {
        return {
          records: [] as readonly UsageRecord[],
          source: {
            fingerprint: {
              hostId: NodeOS.hostname(),
              provider: "cursor" as const,
              resolvedHomePath: databasePath,
              volumeId: "",
            },
            status: "missing" as const,
            scannedFiles: 0,
            skippedFiles: 0,
            malformedRecords: 0,
            distinctSessions: 0,
            message: "Cursor is not installed, signed in, or its session has expired.",
          } satisfies UsageSource,
        };
      }

      const sourceId = `cursor-account:${session.accountFingerprint}`;
      const cacheKey = `${session.accountFingerprint}\u0000${input.sinceDay}\u0000${input.untilDay}`;
      const cached = cursorUsageCache.get(cacheKey);
      let records: readonly UsageRecord[];
      let malformedRecords: number;

      if (cached !== undefined && nowMs - cached.fetchedAtMs < CURSOR_USAGE_TTL_MS) {
        records = cached.records;
        malformedRecords = cached.malformedRecords;
      } else {
        const pages: CursorUsagePage[] = [];
        let completed = false;
        for (let page = 1; page <= CURSOR_USAGE_MAX_PAGES; page += 1) {
          const document = yield* HttpClientRequest.post(CURSOR_USAGE_URL).pipe(
            HttpClientRequest.setHeaders({
              accept: "application/json",
              cookie: session.cookieHeader,
              origin: "https://cursor.com",
            }),
            HttpClientRequest.bodyJsonUnsafe({
              page,
              pageSize: CURSOR_USAGE_PAGE_SIZE,
              startDate: String(Date.parse(`${input.sinceDay}T00:00:00Z`) - MTIME_SLACK_MS),
              endDate: String(
                Date.parse(`${input.untilDay}T00:00:00Z`) +
                  24 * 60 * 60 * 1000 -
                  1 +
                  MTIME_SLACK_MS,
              ),
            }),
            httpClient.execute,
            Effect.flatMap(HttpClientResponse.filterStatusOk),
            Effect.flatMap((response) => response.json),
            Effect.timeout(30_000),
          );
          const parsed = parseCursorUsagePage(document);
          if (parsed === null) {
            return yield* new UsageReadError({
              reason: "scanFailed",
              detail: "Cursor returned an unrecognised usage response.",
            });
          }
          pages.push(parsed);
          if (parsed.events.length < CURSOR_USAGE_PAGE_SIZE) {
            completed = true;
            break;
          }
        }

        const events = yield* Effect.try({
          try: () => reconcileCursorUsagePages(pages, completed),
          catch: (cause) =>
            new UsageReadError({
              reason: "scanFailed",
              detail: "Cursor usage pagination was incomplete.",
              cause,
            }),
        });
        const parsedRecords: UsageRecord[] = [];
        malformedRecords = 0;
        for (const event of events) {
          const parsed = parseCursorUsageEvent(event);
          if (parsed._tag === "record") parsedRecords.push(parsed.record);
          if (parsed._tag === "malformed") malformedRecords += 1;
        }
        records = parsedRecords;
        cursorUsageCache.set(cacheKey, { fetchedAtMs: nowMs, records, malformedRecords });
      }

      return {
        records,
        source: {
          fingerprint: {
            hostId: NodeOS.hostname(),
            provider: "cursor" as const,
            resolvedHomePath: sourceId,
            volumeId: "",
          },
          status: "ok" as const,
          scannedFiles: 0,
          skippedFiles: 0,
          malformedRecords,
          distinctSessions: 0,
          message: "Account-wide usage from cursor.com.",
        } satisfies UsageSource,
      };
    },
    Effect.catchCause((cause) =>
      Effect.succeed({
        records: [] as readonly UsageRecord[],
        source: {
          fingerprint: {
            hostId: NodeOS.hostname(),
            provider: "cursor" as const,
            resolvedHomePath: resolveCursorAuthDatabasePath(
              hostPlatform,
              NodeOS.homedir(),
              hostEnvironment,
            ),
            volumeId: "",
          },
          status: "failed" as const,
          scannedFiles: 0,
          skippedFiles: 0,
          malformedRecords: 0,
          distinctSessions: 0,
          message: `Cursor usage could not be read: ${String(Cause.squash(cause)).slice(0, 180)}`,
        } satisfies UsageSource,
      }),
    ),
  );

  /** One provider directory's walk and parse, before rates are involved. */
  interface ScannedDir {
    readonly provider: UsageProviderKind;
    readonly dir: string;
    readonly volumeId: string;
    /** Parsed records per file, or `null` when the directory does not exist. */
    readonly files:
      | readonly { readonly path: string; readonly records: readonly UsageRecord[] }[]
      | null;
  }

  const collectDirs = Effect.fn("UsageService.collectDirs")(function* (
    windowStartMs: number,
    settings: ServerSettingsValue,
  ) {
    // The home resolvers ask for `Path` themselves; satisfy them from the
    // instance we already hold so the scan stays context-free.
    const dirs = yield* resolveTranscriptDirs(settings).pipe(
      Effect.provideService(Path.Path, path),
    );
    const scanned: ScannedDir[] = [];
    for (const { provider, dir, fileName } of dirs) {
      const volumeId = yield* Effect.promise(() => readDirectoryVolumeId(dir));
      const exists = yield* fileSystem
        .exists(dir)
        .pipe(Effect.catchCause(() => Effect.succeed(false)));
      if (!exists) {
        scanned.push({ provider, dir, volumeId, files: null });
        continue;
      }
      const files = yield* Effect.promise(() =>
        listTranscriptFiles(dir, windowStartMs, fileName === undefined ? undefined : { fileName }),
      );
      const parsedFiles: { path: string; records: readonly UsageRecord[] }[] = [];
      for (const file of files) {
        const records = yield* readFileRecords(file.path, file.size, file.mtimeMs, provider);
        parsedFiles.push({ path: file.path, records });
      }
      scanned.push({ provider, dir, volumeId, files: parsedFiles });
    }
    return scanned;
  });

  const scanSummary = Effect.fn("UsageService.scanSummary")(function* (
    input: UsageSummaryInput,
    settings: ServerSettingsValue,
  ) {
    if (input.sinceDay > input.untilDay) {
      return yield* new UsageReadError({
        reason: "invalidWindow",
        detail: `sinceDay '${input.sinceDay}' is after untilDay '${input.untilDay}'`,
      });
    }

    let hourlyWindow: { readonly sinceTimeMs: number; readonly untilTimeMs: number } | null = null;
    if (input.resolution === "hour") {
      const sinceTime =
        input.sinceTime === undefined ? Option.none() : DateTime.make(input.sinceTime);
      const untilTime =
        input.untilTime === undefined ? Option.none() : DateTime.make(input.untilTime);
      if (Option.isNone(sinceTime) || Option.isNone(untilTime)) {
        return yield* new UsageReadError({
          reason: "invalidWindow",
          detail: "Hourly usage requires valid sinceTime and untilTime instants",
        });
      }
      const sinceTimeMs = DateTime.toEpochMillis(sinceTime.value);
      const untilTimeMs = DateTime.toEpochMillis(untilTime.value);
      const durationMs = untilTimeMs - sinceTimeMs;
      if (durationMs <= 0 || durationMs > MAX_HOURLY_WINDOW_MS) {
        return yield* new UsageReadError({
          reason: "invalidWindow",
          detail: "Hourly usage window must be greater than zero and at most 24 hours",
        });
      }
      hourlyWindow = { sinceTimeMs, untilTimeMs };
    }

    const startedAtMs = yield* Clock.currentTimeMillis;
    if (input.modelAliases.length > 0) {
      yield* ensureCatalog();
    }
    yield* ensureScanCacheLoaded;

    const hostId = NodeOS.hostname();
    const windowStart = DateTime.make(`${input.sinceDay}T00:00:00Z`);
    if (Option.isNone(windowStart)) {
      return yield* new UsageReadError({
        reason: "invalidWindow",
        detail: `sinceDay '${input.sinceDay}' is not a valid date`,
      });
    }
    const windowStartMs =
      (hourlyWindow?.sinceTimeMs ?? DateTime.toEpochMillis(windowStart.value)) - MTIME_SLACK_MS;

    // Pricing only matters once records are aggregated, so the rate table
    // loads while transcripts stream instead of gating them: a cold rates
    // fetch on a slow network no longer delays the scan by its own timeout.
    const [, scannedDirs] = yield* Effect.all(
      [ensureRates(false), collectDirs(windowStartMs, settings)],
      { concurrency: 2 },
    );

    const aggregator = new UsageAggregator({
      timeZone: input.timeZone,
      sinceDay: input.sinceDay,
      untilDay: input.untilDay,
      pricer: new UsagePricer({
        rates,
        catalog,
        aliases: input.modelAliases,
        priceOverrides: createOverrideRateTable(settings.usagePriceOverrides),
      }),
      resolution: input.resolution ?? "day",
      ...hourlyWindow,
    });

    const sources: UsageSource[] = [];
    const cursor = yield* readCursorUsage(input, startedAtMs);
    const cursorSessionIds = new Set<string>();
    for (const record of cursor.records) {
      if (aggregator.add(record) && record.sessionId.length > 0) {
        cursorSessionIds.add(record.sessionId);
      }
    }
    sources.push({ ...cursor.source, distinctSessions: cursorSessionIds.size });
    const livePaths = new Set<string>();
    const walkedRoots: string[] = [];

    for (const { provider, dir, volumeId, files } of scannedDirs) {
      if (files === null) {
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
      let scannedFiles = 0;
      let skippedFiles = 0;
      // Distinct per directory. Buckets carry per-cell session counts, but a
      // session spans days and models, so clients total this figure instead.
      const sessionIds = new Set<string>();

      for (const file of files) {
        livePaths.add(file.path);
        if (file.records.length === 0) {
          skippedFiles += 1;
          continue;
        }
        scannedFiles += 1;
        for (const record of file.records) {
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
      pricing:
        input.modelAliases.length > 0 && catalogFetchedAtMs !== null
          ? {
              status: weakerPricingStatus(ratesStatus, catalogStatus),
              source: `${LITELLM_RATES_URL} + ${MODELS_DEV_CATALOG_URL}`,
              fetchedAt: formatOldestFetch(ratesFetchedAtMs, catalogFetchedAtMs),
              knownModels: rates.size + catalog.size,
            }
          : pricing(),
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

  /**
   * In-flight scans by window and custom prices, so concurrent identical requests (the usage
   * page open on two clients at once) share one scan instead of racing over
   * the same corpus twice.
   */
  const inflightScans = new Map<string, Deferred.Deferred<UsageSummary, UsageReadError>>();

  const scanKey = (
    input: UsageSummaryInput,
    priceOverrides: ServerSettingsValue["usagePriceOverrides"],
  ): string =>
    JSON.stringify([
      input.timeZone,
      input.sinceDay,
      input.untilDay,
      input.resolution ?? "day",
      input.sinceTime ?? null,
      input.untilTime ?? null,
      priceOverrides,
      input.modelAliases,
    ]);

  const readSummary = Effect.fn("UsageService.readSummary")(function* (input: UsageSummaryInput) {
    const settings = yield* readSettings;
    const key = scanKey(input, settings.usagePriceOverrides);
    const deferred = yield* Effect.uninterruptible(
      Effect.gen(function* () {
        const existing = inflightScans.get(key);
        if (existing !== undefined) return existing;

        // Enrollment and detached-fiber creation must be atomic. Otherwise a
        // canceled first caller can leave a Deferred with no scan to finish it.
        const created = Deferred.makeUnsafe<UsageSummary, UsageReadError>();
        inflightScans.set(key, created);
        // Detached so one departing client cannot tear the scan out from under
        // the fibers awaiting it; a finished scan warms the cache either way.
        yield* scanSummary(input, settings).pipe(
          Effect.onExit((exit) =>
            Effect.sync(() => inflightScans.delete(key)).pipe(
              Effect.andThen(Deferred.done(created, exit)),
            ),
          ),
          Effect.forkDetach,
        );
        return created;
      }),
    );
    // Waiting stays interruptible. The detached scan continues for other
    // callers and still warms the cache if this caller leaves.
    return yield* Deferred.await(deferred);
  });

  return { readSummary, searchModels, refreshRates } as const;
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
