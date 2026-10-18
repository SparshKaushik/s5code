package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.EnvironmentSession
import club.touchtech.s5code.kotlin.transport.SessionPhase
import club.touchtech.s5code.kotlin.transport.TransportJson
import club.touchtech.s5code.kotlin.transport.WsMethods
import club.touchtech.s5code.kotlin.transport.pullRequestErrorMessage
import club.touchtech.s5code.kotlin.transport.wire.PullRequestActivityDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDetailDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDiffResultDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDiffStatDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestFilesViewedResultDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestLinkedThreadsResultDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestListEntryDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestListResultDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestListStatsResultDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestStackDto
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.KSerializer
import kotlinx.serialization.builtins.nullable
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/**
 * [PullRequestGateway] over the workspace's live sessions, with the cache
 * policy of `packages/client-runtime/src/state/pullRequests.ts`:
 *
 * - each read has a TTL and refetches when a screen subscribes after it;
 * - `pullRequests.subscribeRefreshes` ticks refetch subscribed reads, and a
 *   moved revision marks unsubscribed ones stale;
 * - a merged PR is final, so its reads never refetch on their own;
 * - persisted reads restore from disk first, and a failed refetch keeps the
 *   last value beside its error.
 *
 * Every request is gated on the `pullRequests` capability: a server that does
 * not know a method drops the socket.
 */
@OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)
class LivePullRequestGateway(
    private val scope: CoroutineScope,
    private val workspace: LiveWorkspaceGateway,
    private val cache: PullRequestCacheStore,
    private val clock: () -> Long = System::currentTimeMillis,
) : PullRequestGateway {

    private inner class Entry<T>(
        val environmentId: String,
        val key: String,
        val ttlMs: Long,
        val serializer: KSerializer<T>,
        /** Refetch on a refresh tick while subscribed. */
        val tick: Boolean,
        val persist: Boolean,
        /** Refetch interval while subscribed; 0 for none. */
        val pollMs: Long = 0,
        val isFinal: () -> Boolean = { false },
        val onValue: (T) -> Unit = {},
        val fetch: suspend (EnvironmentSession) -> T,
    ) {
        val state = MutableStateFlow(PullRequestQuery<T>())
        val subscribers = AtomicInteger(0)
        val mutex = Mutex()
        var restored = false
        var revision: Int? = null
        var attemptedAt = 0L

        fun observe(): Flow<PullRequestQuery<T>> = flow {
            subscribers.incrementAndGet()
            ensureWatcher(environmentId)
            try {
                coroutineScope {
                    launch { ensureFresh(this@Entry) }
                    if (pollMs > 0) {
                        launch {
                            while (true) {
                                delay(pollMs)
                                this@LivePullRequestGateway.fetch(this@Entry)
                            }
                        }
                    }
                    state.collect { emit(it) }
                }
            } finally {
                subscribers.decrementAndGet()
            }
        }
    }

    private val entries = ConcurrentHashMap<String, Entry<*>>()
    private val pages = MutableStateFlow<Map<String, PullRequestListPages>>(emptyMap())
    private val revisions = ConcurrentHashMap<String, Int>()
    private val watchers = ConcurrentHashMap<String, Job>()
    private val statsFlows = ConcurrentHashMap<String, MutableStateFlow<Map<String, PullRequestDiffStatDto>>>()
    private val statsRequested = ConcurrentHashMap.newKeySet<String>()

    private val listState = MutableStateFlow(PullRequestListState())
    override val listControls: StateFlow<Map<String, PullRequestListControls>> =
        listState.map { it.controls }.stateIn(scope, SharingStarted.Eagerly, emptyMap())
    override val listEnvironment: StateFlow<String?> =
        listState.map { it.environmentId }.stateIn(scope, SharingStarted.Eagerly, null)
    override val knownEnvironments: StateFlow<Set<String>> =
        listState.map { it.environments }.stateIn(scope, SharingStarted.Eagerly, emptySet())

    init {
        scope.launch {
            val saved = cache.readListState()
            // Picks made before the file loaded win over the saved ones.
            listState.update { held ->
                PullRequestListState(
                    environmentId = held.environmentId ?: saved.environmentId,
                    controls = saved.controls + held.controls,
                    environments = saved.environments + held.environments,
                )
            }
        }
    }

    private fun updateListState(transform: (PullRequestListState) -> PullRequestListState) {
        listState.update(transform)
        val snapshot = listState.value
        scope.launch { runCatching { cache.writeListState(snapshot) } }
    }

    override fun setListControls(scope: String, controls: PullRequestListControls) =
        updateListState { it.copy(controls = it.controls + (scope to controls)) }

    override fun setListEnvironment(environmentId: String?) = updateListState { it.copy(environmentId = environmentId) }

    private fun rememberEnvironment(environmentId: String) {
        if (environmentId !in listState.value.environments) {
            updateListState { it.copy(environments = it.environments + environmentId) }
        }
    }

    override fun connected(environmentId: String): Flow<Boolean> =
        workspace.sessionFlow(environmentId)
            .flatMapLatest { session -> session?.state?.map { it.phase == SessionPhase.Connected } ?: flowOf(false) }
            .distinctUntilChanged()

    /* ── Lists ───────────────────────────────────────────────────────── */

    private fun pagesKey(target: PullRequestListTarget) = "${target.environmentId}\u0000${target.key}"

    private fun listEntry(target: PullRequestListTarget): Entry<PullRequestListResultDto> =
        entry(
            target.environmentId,
            target.key,
            ttlMs = LIST_TTL_MS,
            serializer = PullRequestListResultDto.serializer(),
            tick = true,
            persist = true,
            onValue = { head ->
                pages.update { it + (pagesKey(target) to pullRequestListHead(it[pagesKey(target)], head)) }
            },
        ) { session ->
            session.request(WsMethods.PullRequestsList, target.input.toJson(), PullRequestListResultDto.serializer())
        }

    override fun list(targets: List<PullRequestListTarget>): Flow<List<PullRequestListSource>> {
        if (targets.isEmpty()) return flowOf(emptyList())
        val queries = targets.map { listEntry(it).observe() }
        return combine(combine(queries) { it.toList() }, pages) { results, loaded ->
            targets.mapIndexed { index, target ->
                PullRequestListSource(target, results[index], loaded[pagesKey(target)])
            }
        }
    }

    override suspend fun loadMore(targets: List<PullRequestListTarget>, retry: Boolean) {
        val failures =
            coroutineScope {
                targets.mapNotNull { target ->
                    val held = pages.value[pagesKey(target)] ?: return@mapNotNull null
                    val input = nextPullRequestListInput(held, target.input) ?: return@mapNotNull null
                    val session = readySession(target.environmentId) ?: return@mapNotNull null
                    async {
                        runCatching {
                            val page =
                                session.request(WsMethods.PullRequestsList, input.toJson(), PullRequestListResultDto.serializer())
                            if (page.errors.isNotEmpty()) error(page.errors.joinToString("\n") { it.message })
                            val next = appendPullRequestListPage(pages.value[pagesKey(target)] ?: held, page, input)
                            pages.update { it + (pagesKey(target) to next) }
                            val cursors = input.cursors
                            if (cursors != null && next.nextCursors.isNotEmpty() && cursors == next.nextCursors) {
                                error("Couldn't advance the list. Try again.")
                            }
                        }.exceptionOrNull()
                    }
                }.awaitAll()
            }.filterNotNull()
        failures.firstOrNull()?.let { if (it is CancellationException) throw it else error(pullRequestErrorMessage(it)) }
    }

    override suspend fun refreshList(targets: List<PullRequestListTarget>) {
        val failures =
            coroutineScope {
                targets.map { target ->
                    async {
                        val session = readySession(target.environmentId) ?: return@async null
                        runCatching {
                            session.execute(WsMethods.PullRequestsInvalidate, JsonObject(emptyMap()))
                        }.exceptionOrNull()?.let { return@async it }
                        val entry = listEntry(target)
                        fetch(entry, force = true)?.let { return@async it }
                        entry.state.value.value?.let { head ->
                            pages.update { it + (pagesKey(target) to pullRequestListHead(null, head)) }
                        }
                        null
                    }
                }.awaitAll()
            }.filterNotNull()
        failures.firstOrNull()?.let { if (it is CancellationException) throw it else error(pullRequestErrorMessage(it)) }
    }

    /* ── Diff stats ──────────────────────────────────────────────────── */

    private fun statsFlow(environmentId: String) =
        statsFlows.getOrPut(environmentId) {
            MutableStateFlow<Map<String, PullRequestDiffStatDto>>(emptyMap()).also { flow ->
                scope.launch {
                    val stored = cache.read(environmentId, STATS_KEY) ?: return@launch
                    val restored =
                        runCatching { TransportJson.decodeFromJsonElement(PullRequestListStatsResultDto.serializer(), stored.value) }
                            .getOrNull() ?: return@launch
                    flow.update { held -> restored.stats.associateBy(::statKey) + held }
                }
            }
        }

    override fun stats(environmentId: String): StateFlow<Map<String, PullRequestDiffStatDto>> =
        statsFlow(environmentId).asStateFlow()

    override fun loadStats(environmentId: String, references: List<PullRequestReference>) {
        val flow = statsFlow(environmentId)
        val wanted =
            references.filter {
                val key = statKey(it)
                key !in flow.value && statsRequested.add("$environmentId\u0000$key")
            }.take(MAX_STATS_REFS)
        if (wanted.isEmpty()) return
        scope.launch {
            val session = readySession(environmentId)
            val result =
                session?.let {
                    runCatching {
                        it.request(
                            WsMethods.PullRequestsListStats,
                            buildJsonObject { put("refs", buildJsonArray { wanted.forEach { ref -> add(ref.toJson()) } }) },
                            PullRequestListStatsResultDto.serializer(),
                        )
                    }.getOrNull()
                }
            if (result == null) {
                // Let the rows ask again once they are visible on a live socket.
                wanted.forEach { statsRequested.remove("$environmentId\u0000${statKey(it)}") }
                return@launch
            }
            flow.update { held -> (held + result.stats.associateBy(::statKey)).entries.toList().takeLast(MAX_STATS).associate { it.toPair() } }
            runCatching {
                cache.write(
                    environmentId,
                    STATS_KEY,
                    clock(),
                    TransportJson.encodeToJsonElement(
                        PullRequestListStatsResultDto.serializer(),
                        PullRequestListStatsResultDto(flow.value.values.toList()),
                    ),
                )
            }
        }
    }

    /* ── One pull request ────────────────────────────────────────────── */

    private fun detailEntry(environmentId: String, reference: PullRequestReference) =
        entry(
            environmentId,
            "detail:${reference.cacheKey}",
            ttlMs = READ_TTL_MS,
            serializer = PullRequestDetailDto.serializer(),
            tick = true,
            persist = true,
            isFinal = { isMerged(environmentId, reference) },
        ) { it.request(WsMethods.PullRequestsDetail, reference.toJson(), PullRequestDetailDto.serializer()) }

    private fun activityEntry(environmentId: String, reference: PullRequestReference) =
        entry(
            environmentId,
            "activity:${reference.cacheKey}",
            ttlMs = READ_TTL_MS,
            serializer = PullRequestActivityDto.serializer(),
            tick = true,
            persist = true,
            isFinal = { isMerged(environmentId, reference) },
        ) { it.request(WsMethods.PullRequestsActivity, reference.toJson(), PullRequestActivityDto.serializer()) }

    private fun stackEntry(environmentId: String, reference: PullRequestReference) =
        entry(
            environmentId,
            "stack:${reference.cacheKey}",
            ttlMs = READ_TTL_MS,
            serializer = PullRequestStackDto.serializer().nullable,
            tick = true,
            persist = true,
            isFinal = { isMerged(environmentId, reference) },
        ) { it.request(WsMethods.PullRequestsStack, reference.toJson(), PullRequestStackDto.serializer().nullable) }

    override fun detail(environmentId: String, reference: PullRequestReference) =
        detailEntry(environmentId, reference).observe()

    override fun activity(environmentId: String, reference: PullRequestReference) =
        activityEntry(environmentId, reference).observe()

    override fun stack(environmentId: String, reference: PullRequestReference) =
        stackEntry(environmentId, reference).observe()

    override fun linkedThreads(environmentId: String, reference: PullRequestReference) =
        entry(
            environmentId,
            "linkedThreads:${reference.cacheKey}",
            ttlMs = 0,
            serializer = PullRequestLinkedThreadsResultDto.serializer(),
            tick = true,
            persist = true,
            pollMs = LINKED_THREADS_POLL_MS,
        ) {
            it.request(WsMethods.PullRequestsLinkedThreads, reference.toJson(), PullRequestLinkedThreadsResultDto.serializer())
        }.observe()

    override fun filesViewed(environmentId: String, reference: PullRequestReference) =
        entry(
            environmentId,
            "filesViewed:${reference.cacheKey}",
            ttlMs = FILES_VIEWED_TTL_MS,
            serializer = PullRequestFilesViewedResultDto.serializer(),
            tick = false,
            persist = true,
        ) {
            it.request(WsMethods.PullRequestsFilesViewed, reference.toJson(), PullRequestFilesViewedResultDto.serializer())
        }.observe()

    override suspend fun diff(
        environmentId: String,
        reference: PullRequestReference,
        cursor: String?,
        commit: String?,
    ): PullRequestDiffResultDto {
        val session = workspace.session(environmentId) ?: error("That environment is no longer paired.")
        val payload =
            JsonObject(
                reference.toJson() +
                    buildJsonObject {
                        cursor?.let { put("cursor", it) }
                        commit?.let { put("commit", it) }
                    }
            )
        return session.postJson(DIFF_PATH, payload, PullRequestDiffResultDto.serializer())
    }

    override suspend fun refresh(environmentId: String, reference: PullRequestReference) {
        coroutineScope {
            listOf(
                    detailEntry(environmentId, reference),
                    activityEntry(environmentId, reference),
                    stackEntry(environmentId, reference),
                )
                .map { async { fetch(it, force = true) } }
                .awaitAll()
        }
    }

    override suspend fun refreshDetail(environmentId: String, reference: PullRequestReference) {
        val entry = detailEntry(environmentId, reference)
        if (!entry.isFinal()) fetch(entry, force = true)
    }

    override fun summary(environmentId: String, reference: PullRequestReference): PullRequestListEntryDto? =
        pages.value.entries
            .asSequence()
            .filter { it.key.startsWith("$environmentId\u0000") }
            .flatMap { it.value.entries }
            .firstOrNull {
                it.projectId == reference.projectId &&
                    it.number == reference.number &&
                    it.repository.equals(reference.repository, ignoreCase = true)
            }

    /* ── Cache machinery ─────────────────────────────────────────────── */

    @Suppress("UNCHECKED_CAST")
    private fun <T> entry(
        environmentId: String,
        key: String,
        ttlMs: Long,
        serializer: KSerializer<T>,
        tick: Boolean,
        persist: Boolean,
        pollMs: Long = 0,
        isFinal: () -> Boolean = { false },
        onValue: (T) -> Unit = {},
        fetch: suspend (EnvironmentSession) -> T,
    ): Entry<T> =
        entries.getOrPut("$environmentId\u0000$key") {
            Entry(environmentId, key, ttlMs, serializer, tick, persist, pollMs, isFinal, onValue, fetch)
        } as Entry<T>

    private fun isMerged(environmentId: String, reference: PullRequestReference): Boolean {
        val detail = entries["$environmentId\u0000detail:${reference.cacheKey}"]?.state?.value?.value
        return (detail as? PullRequestDetailDto)?.state == "merged"
    }

    private fun readySession(environmentId: String): EnvironmentSession? =
        workspace.session(environmentId)?.takeIf {
            val state = it.state.value
            state.phase == SessionPhase.Connected && state.capabilities.pullRequests
        }

    private suspend fun <T> restore(entry: Entry<T>) {
        if (!entry.persist || entry.restored) return
        entry.restored = true
        val stored = runCatching { cache.read(entry.environmentId, entry.key) }.getOrNull() ?: return
        val value = runCatching { TransportJson.decodeFromJsonElement(entry.serializer, stored.value) }.getOrElse { return }
        if (entry.state.value.hasValue) return
        entry.state.value = PullRequestQuery(value = value, hasValue = true, fetchedAt = stored.fetchedAt, fromCache = true)
        entry.onValue(value)
    }

    private suspend fun <T> ensureFresh(entry: Entry<T>) {
        entry.mutex.withLock { restore(entry) }
        val current = entry.state.value
        val stale =
            isPullRequestQueryStale(
                fetchedAt = current.fetchedAt.takeIf { current.hasValue },
                now = clock(),
                ttlMs = entry.ttlMs,
                fetchedRevision = entry.revision,
                currentRevision = revisions[entry.environmentId],
                final = current.hasValue && entry.isFinal(),
            )
        if (stale) fetch(entry)
    }

    /**
     * Fetches [entry] unless a fetch that began after this call was made has
     * already finished. Returns the failure, if any; the entry keeps its value.
     */
    private suspend fun <T> fetch(entry: Entry<T>, force: Boolean = false): Throwable? {
        val requestedAt = clock()
        return entry.mutex.withLock {
            restore(entry)
            if (!force && entry.attemptedAt > requestedAt) return@withLock null
            val session = readySession(entry.environmentId) ?: return@withLock null
            entry.attemptedAt = clock()
            val revision = revisions[entry.environmentId]
            entry.state.update { it.copy(loading = true) }
            val result = runCatching { entry.fetch(session) }
            result.fold(
                onSuccess = { value ->
                    val now = clock()
                    entry.revision = revision
                    entry.state.value = PullRequestQuery(value = value, hasValue = true, fetchedAt = now)
                    entry.onValue(value)
                    if (entry.persist) {
                        scope.launch {
                            runCatching {
                                cache.write(
                                    entry.environmentId,
                                    entry.key,
                                    now,
                                    TransportJson.encodeToJsonElement(entry.serializer, value),
                                )
                            }
                        }
                    }
                    null
                },
                onFailure = { error ->
                    if (error is CancellationException) {
                        entry.state.update { it.copy(loading = false) }
                        throw error
                    }
                    entry.state.update { it.copy(loading = false, error = pullRequestErrorMessage(error)) }
                    error
                },
            )
        }
    }

    /**
     * One refresh subscription per environment while it is connected with
     * pull request support. Revalidates stale subscribed reads on each
     * connect, and refetches subscribed reads when the revision moves.
     */
    private fun ensureWatcher(environmentId: String) {
        watchers.computeIfAbsent(environmentId) {
            scope.launch {
                workspace.sessionFlow(environmentId).collectLatest { session ->
                    if (session == null) return@collectLatest
                    session.state
                        .map { it.phase == SessionPhase.Connected && it.capabilities.pullRequests }
                        .distinctUntilChanged()
                        .collectLatest { ready ->
                            if (!ready) return@collectLatest
                            rememberEnvironment(environmentId)
                            subscribed(environmentId).forEach { scope.launch { ensureFresh(it) } }
                            session
                                .subscribe(WsMethods.PullRequestsSubscribeRefreshes, JsonObject(emptyMap()), Int.serializer())
                                .collect { revision ->
                                    val previous = revisions.put(environmentId, revision)
                                    if (previous != null && previous != revision) {
                                        subscribed(environmentId)
                                            .filter { it.tick && !(it.state.value.hasValue && it.isFinal()) }
                                            .forEach { scope.launch { fetch(it) } }
                                    }
                                }
                        }
                }
            }
        }
    }

    private fun subscribed(environmentId: String): List<Entry<*>> =
        entries.values.filter { it.environmentId == environmentId && it.subscribers.get() > 0 }

    private companion object {
        const val LIST_TTL_MS = 30_000L
        const val READ_TTL_MS = 60_000L
        const val FILES_VIEWED_TTL_MS = 15_000L
        const val LINKED_THREADS_POLL_MS = 10_000L
        const val MAX_STATS_REFS = 500
        const val MAX_STATS = 2_000
        const val STATS_KEY = "listStats"
        const val DIFF_PATH = "/api/pull-requests/diff"

        fun statKey(stat: PullRequestDiffStatDto) = "${stat.projectId}:${stat.repository}:${stat.number}"

        fun statKey(reference: PullRequestReference) = "${reference.projectId}:${reference.repository}:${reference.number}"
    }
}
