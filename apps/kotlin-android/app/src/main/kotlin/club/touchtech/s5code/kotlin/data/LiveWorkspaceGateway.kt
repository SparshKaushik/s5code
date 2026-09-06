package club.touchtech.s5code.kotlin.data

import android.content.Context
import club.touchtech.s5code.kotlin.model.BranchRef
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.model.Environment
import club.touchtech.s5code.kotlin.model.EnvironmentCapabilities
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.EnvironmentKind
import club.touchtech.s5code.kotlin.model.FileNode
import club.touchtech.s5code.kotlin.model.GitStatus
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.ProviderCatalogEntry
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.PullRequestRef
import club.touchtech.s5code.kotlin.model.PullRequestState
import club.touchtech.s5code.kotlin.model.RepositoryRef
import club.touchtech.s5code.kotlin.model.ReviewFile
import club.touchtech.s5code.kotlin.model.RuntimeMode
import club.touchtech.s5code.kotlin.model.SlashCommand
import club.touchtech.s5code.kotlin.model.SourceFile
import club.touchtech.s5code.kotlin.model.TerminalSession
import club.touchtech.s5code.kotlin.model.TerminalStatus
import club.touchtech.s5code.kotlin.model.ThreadDetail
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadSearchMatch
import club.touchtech.s5code.kotlin.model.ThreadSettings
import club.touchtech.s5code.kotlin.model.ThreadSummary
import club.touchtech.s5code.kotlin.model.ThreadSyncPhase
import club.touchtech.s5code.kotlin.model.Usage
import club.touchtech.s5code.kotlin.model.UsageDay
import club.touchtech.s5code.kotlin.model.UsageModelBreakdown
import club.touchtech.s5code.kotlin.model.UsageProviderBreakdown
import club.touchtech.s5code.kotlin.model.UsageTotals
import club.touchtech.s5code.kotlin.model.UsageWindow
import club.touchtech.s5code.kotlin.model.UserInputAnswer
import club.touchtech.s5code.kotlin.model.WorkspaceAsset
import club.touchtech.s5code.kotlin.transport.EnvironmentHttp
import club.touchtech.s5code.kotlin.transport.EnvironmentAuthorizer
import club.touchtech.s5code.kotlin.transport.DirectEnvironmentAuthorizer
import club.touchtech.s5code.kotlin.transport.EnvironmentSession
import club.touchtech.s5code.kotlin.transport.SessionPhase
import club.touchtech.s5code.kotlin.transport.ThreadReduction
import club.touchtech.s5code.kotlin.transport.WsMethods
import club.touchtech.s5code.kotlin.transport.applyShellStreamItem
import club.touchtech.s5code.kotlin.transport.hasCacheableWorkspaceContent
import club.touchtech.s5code.kotlin.transport.applyThreadEvent
import club.touchtech.s5code.kotlin.transport.wire.AssetUrlResultDto
import club.touchtech.s5code.kotlin.transport.wire.DispatchResultDto
import club.touchtech.s5code.kotlin.transport.wire.GitActionProgressEventDto
import club.touchtech.s5code.kotlin.transport.wire.ProjectListEntriesResultDto
import club.touchtech.s5code.kotlin.transport.wire.ProjectReadFileResultDto
import club.touchtech.s5code.kotlin.transport.wire.ReviewDiffPreviewResultDto
import club.touchtech.s5code.kotlin.transport.wire.SearchThreadsResultDto
import club.touchtech.s5code.kotlin.transport.wire.ShellSnapshotDto
import club.touchtech.s5code.kotlin.transport.wire.ShellStreamItemDto
import club.touchtech.s5code.kotlin.transport.wire.SourceControlCloneResultDto
import club.touchtech.s5code.kotlin.transport.wire.SourceControlRepositoryDto
import club.touchtech.s5code.kotlin.transport.wire.TerminalSnapshotDto
import club.touchtech.s5code.kotlin.transport.wire.TerminalStreamEventDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadStreamItemDto
import club.touchtech.s5code.kotlin.transport.wire.UsageSummaryDto
import club.touchtech.s5code.kotlin.transport.wire.VcsListRefsResultDto
import club.touchtech.s5code.kotlin.transport.wire.VcsStatusDto
import club.touchtech.s5code.kotlin.transport.wire.FilesystemBrowseResultDto
import java.util.UUID
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.launchIn
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.onEach
import kotlinx.coroutines.flow.retryWhen
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.supervisorScope
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.OkHttpClient

/**
 * The real workspace: several environments, merged.
 *
 * One session per paired environment, each with its own shell subscription. The
 * merge is what makes this a workspace rather than a list of servers — a user
 * with a laptop and a desktop sees one inbox, and the environment only shows up
 * as a label on the row and a health dot.
 *
 * Independence is the property that matters most here: a sleeping desktop must
 * not delay or empty the laptop's threads. Every derived list is built from
 * whatever snapshots exist right now, so an environment that is down simply
 * contributes nothing.
 */
class LiveWorkspaceGateway(
    private val context: Context,
    private val scope: CoroutineScope,
    private val store: EnvironmentStore,
    private val http: EnvironmentHttp,
    private val client: OkHttpClient,
    private val snapshotStore: WorkspaceSnapshotStore = WorkspaceSnapshotStore(context),
    /**
     * Resolves the credential for one saved environment. Direct rows read the
     * stored token; cloud rows mint one through the relay. The gateway asks for an
     * authorizer per row rather than owning the choice, so the relay slice stays
     * out of the merge logic.
     */
    private val authorizerFor: (SavedEnvironment) -> EnvironmentAuthorizer = {
        DirectEnvironmentAuthorizer(store)
    },
) : WorkspaceGateway {

    private class Connected(
        val session: EnvironmentSession,
        val shell: MutableStateFlow<ShellSnapshotDto?>,
        var job: Job? = null,
        var stateJob: Job? = null,
    )

    private val sessions = MutableStateFlow<Map<String, Connected>>(emptyMap())

    /**
     * Ticks once a minute so relative timestamps ("4m", "2h") and the queued-turn
     * grace window stay current. One shared tick rather than a timer per row: the
     * repo's performance rules call out high-frequency timers, and a hundred rows
     * with their own clock is exactly that.
     */
    private val clock = MutableStateFlow(System.currentTimeMillis())

    private val _environments = MutableStateFlow<List<Environment>>(emptyList())
    override val environments: StateFlow<List<Environment>> = _environments.asStateFlow()

    private val _projects = MutableStateFlow<List<Project>>(emptyList())
    override val projects: StateFlow<List<Project>> = _projects.asStateFlow()

    private val _threads = MutableStateFlow<List<ThreadSummary>>(emptyList())
    override val threads: StateFlow<List<ThreadSummary>> = _threads.asStateFlow()

    private val _archived = MutableStateFlow<List<ThreadSummary>>(emptyList())
    override val archived: StateFlow<List<ThreadSummary>> = _archived.asStateFlow()

    private val _pendingRequests = MutableStateFlow<List<HomePendingRequest>>(emptyList())
    override val pendingRequests: StateFlow<List<HomePendingRequest>> =
        _pendingRequests.asStateFlow()

    private val _providerCatalog = MutableStateFlow<List<ProviderCatalogEntry>>(emptyList())
    override val providerCatalog: StateFlow<List<ProviderCatalogEntry>> =
        _providerCatalog.asStateFlow()

    /** Live thread details, keyed by environment and thread. */
    private val details = mutableMapOf<String, MutableStateFlow<ThreadDetail?>>()
    private val detailSyncPhases = mutableMapOf<String, MutableStateFlow<ThreadSyncPhase>>()
    /**
     * A client-generated thread can be opened before its queued bootstrap reaches
     * the server. Its detail subscription gets a normal not-found response in
     * that window and must retry without failing the ViewModel's parent scope.
     */
    private val pendingCreationKeys = MutableStateFlow<Set<String>>(emptySet())
    private val detailJobs = mutableMapOf<String, Job>()
    /** Threads subscribed solely because their shell says Home needs an actionable gate. */
    private val homePendingDetailKeys = mutableSetOf<String>()
    /** Explicit thread screens own subscriptions independently from Home's temporary interest. */
    private val explicitDetailKeys = mutableSetOf<String>()

    private class CachedIconUrl(val url: String?, val expiresAtMillis: Long)

    private data class CachedReview(
        val files: List<ReviewFile>,
        val cachedAtMillis: Long,
    )

    /** Parsed review previews avoid a second fetch/parse across prewarm, review, and comment. */
    private val reviewCache = java.util.concurrent.ConcurrentHashMap<String, CachedReview>()

    /** Source reads and signed preview URLs shared by tree prewarm and destination screens. */
    private val workspaceFileCache = WorkspaceFileCache()

    /** Signed project-icon URLs, keyed by environment and workspace root. */
    private val projectIconUrls = java.util.concurrent.ConcurrentHashMap<String, CachedIconUrl>()

    /**
     * Slash commands for a provider instance, merged across environments and
     * de-duplicated by name. Two machines running the same instance advertise the
     * same commands, and the composer should offer each once.
     */
    override fun slashCommands(provider: ProviderInstance): List<SlashCommand> =
        sessions.value.values
            .flatMap { it.session.providers.value }
            .filter { it.instanceId == provider.instanceId }
            .flatMap { it.slashCommands }
            .distinctBy { it.name }
            .map { SlashCommand(name = "/${it.name.removePrefix("/")}", description = it.description.orEmpty()) }
            .sortedBy { it.name }

    init {
        scope.launch {
            while (true) {
                kotlinx.coroutines.delay(60_000)
                clock.value = System.currentTimeMillis()
                publish()
            }
        }
        // Sessions follow the saved list: pairing starts one, unpairing stops one.
        store.environments
            .onEach { reconcile(it) }
            .launchIn(scope)
    }

    /* ── Session lifecycle ───────────────────────────────────────────── */

    private fun reconcile(saved: List<SavedEnvironment>) {
        val savedIds = saved.map { it.environmentId }.toSet()
        val current = sessions.value

        current.filterKeys { it !in savedIds }.forEach { (id, connected) ->
            connected.job?.cancel()
            connected.stateJob?.cancel()
            connected.session.stop()
            val prefix = "$id/"
            detailJobs.keys.filter { it.startsWith(prefix) }.forEach(::dropDetailSubscription)
            pendingCreationKeys.update { keys -> keys.filterNotTo(mutableSetOf()) { it.startsWith(prefix) } }
            homePendingDetailKeys.removeAll { it.startsWith(prefix) }
            explicitDetailKeys.removeAll { it.startsWith(prefix) }
            sessions.update { it - id }
        }

        saved.forEach { environment ->
            if (current.containsKey(environment.environmentId)) return@forEach
            val session =
                EnvironmentSession(
                    environmentId = environment.environmentId,
                    initialLabel = environment.label,
                    scope = scope,
                    http = http,
                    client = client,
                    authorizer = authorizerFor(environment),
                    saved = {
                        store.environments.value.firstOrNull {
                            it.environmentId == environment.environmentId
                        }
                    },
                    initialPhase = SessionPhase.Connecting,
                )
            val connected = Connected(session, MutableStateFlow(null))
            sessions.update { it + (environment.environmentId to connected) }
            // Session phase changes are independent of shell frames. Publish
            // them immediately so Home/outbox see reconnecting and connected
            // even before the replacement shell snapshot arrives.
            connected.stateJob = session.state.onEach { publish() }.launchIn(scope)
            // Start the live attempt immediately. The cache read then races only
            // as a fallback and compareAndSet keeps a fresh socket snapshot newer.
            session.start()
            connected.job = scope.launch { subscribeShell(connected) }
            scope.launch {
                snapshotStore.loadShell(EnvironmentId(environment.environmentId))?.let { cached ->
                    // Never replace a fresher live snapshot if the disk read loses
                    // the race with a fast local connection.
                    connected.shell.compareAndSet(expect = null, update = cached)
                    publish()
                }
            }
        }
        publish()
    }

    /**
     * Keeps one environment's shell snapshot current.
     *
     * The subscription re-issues itself on reconnect (see
     * [EnvironmentSession.subscribe]), and each new subscription opens with a
     * fresh snapshot frame, so the reducer rebuilds from authoritative state
     * rather than trusting deltas across a gap.
     */
    private suspend fun subscribeShell(connected: Connected) {
        connected.session
            .subscribe(
                WsMethods.OrchestrationSubscribeShell,
                buildJsonObject { put("requestCompletionMarker", true) },
                ShellStreamItemDto.serializer(),
            )
            .collect { item ->
                connected.shell.update { current ->
                    applyShellStreamItem(current ?: ShellSnapshotDto(), item)
                }
                // Persist the fully reduced snapshot on every shell frame. A
                // reconnect can fail before "synchronized", and that must not
                // discard project/thread updates already acknowledged locally.
                connected.shell.value?.takeIf { it.hasCacheableWorkspaceContent() }?.let { snapshot ->
                    snapshotStore.saveShell(
                        EnvironmentId(connected.session.environmentId),
                        snapshot,
                    )
                }
                publish()
            }
    }

    /**
     * Recomputes the merged lists. Called on every shell change and clock tick.
     *
     * Rebuilding whole lists is the right trade here: the home list is bounded by
     * how many threads a person has, the projection is pure, and Compose diffs by
     * stable key so unchanged rows do not recompose. Incremental merging would add
     * a second source of truth for ordering.
     */
    private fun publish() {
        val entries = sessions.value
        val now = clock.value

        _environments.value =
            entries.values.map { connected ->
                val state = connected.session.state.value
                val saved =
                    store.environments.value.firstOrNull {
                        it.environmentId == connected.session.environmentId
                    }
                Environment(
                    id = EnvironmentId(connected.session.environmentId),
                    label = connected.session.label.value,
                    host =
                        saved
                            ?.httpBaseUrl
                            ?.removePrefix("http://")
                            ?.removePrefix("https://")
                            ?.trimEnd('/')
                            .orEmpty(),
                    kind =
                        if (saved?.relayManaged == true) EnvironmentKind.Cloud
                        else EnvironmentKind.Direct,
                    state =
                        when (state.phase) {
                            SessionPhase.Connected -> ConnectionState.Connected
                            SessionPhase.Connecting -> ConnectionState.Connecting
                            SessionPhase.Backoff -> ConnectionState.Recovering
                            SessionPhase.Unauthorized -> ConnectionState.AuthRequired
                            SessionPhase.Idle -> ConnectionState.Offline
                        },
                    lastSeenLabel = state.lastError ?: "",
                    serverVersion = state.serverVersion.orEmpty(),
                    capabilities =
                        EnvironmentCapabilities(
                            threadSettlement = state.capabilities.threadSettlement,
                            threadSnooze = state.capabilities.threadSnooze,
                            threadPinning = state.capabilities.threadPinning,
                            threadTitleRegeneration =
                                state.capabilities.threadTitleRegeneration,
                        ),
                )
            }

        val allProjects = mutableListOf<Project>()
        val active = mutableListOf<ThreadSummary>()
        val archivedThreads = mutableListOf<ThreadSummary>()

        entries.forEach { (environmentId, connected) ->
            val snapshot = connected.shell.value ?: return@forEach
            val id = EnvironmentId(environmentId)
            snapshot.projects.forEach { allProjects += projectFrom(id, it) }
            snapshot.threads.forEach { shell ->
                val summary = threadSummaryFrom(id, shell, ::instanceFor, now)
                if (isArchived(shell)) archivedThreads += summary else active += summary
            }
        }

        _projects.value = allProjects
        _threads.value = active
        _archived.value = archivedThreads
        val visibleThreadKeys =
            (active.asSequence() + archivedThreads.asSequence())
                .mapTo(mutableSetOf()) { "${it.environmentId.value}/${it.id.value}" }
        pendingCreationKeys.update { expected -> expected - visibleThreadKeys }
        reconcileHomePendingSubscriptions()
        publishPendingRequests()

        // The catalog is keyed by provider *instance*, mirroring `buildModelOptions`
        // in the RN client: the user picks an instance and a model, and the instance
        // id is what a turn start routes on. Only instances a turn start would
        // actually be accepted for are offered — an unavailable driver, a disabled
        // instance, an uninstalled CLI, or a signed-out account are all refusals, so
        // showing them means offering a row that cannot work.
        _providerCatalog.value =
            entries.values
                .flatMap { it.session.providers.value }
                .filter { provider ->
                    provider.enabled &&
                        provider.installed &&
                        provider.availability != "unavailable" &&
                        provider.auth.status != "unauthenticated"
                }
                // Two machines running the same instance id offer the union of their
                // models, because the user picks an agent, not a machine.
                .groupBy { it.instanceId }
                .map { (instanceId, providers) ->
                    val first = providers.first()
                    val models = providers.flatMap { it.models }
                    ProviderCatalogEntry(
                        instance =
                            ProviderInstance(
                                instanceId = instanceId,
                                driver = first.driver,
                                displayName = first.displayName,
                            ),
                        models = models.map { it.slug }.distinct(),
                        // Per model, because two models of the same provider do not
                        // offer the same knobs: Claude Opus 5 has a context window
                        // and Opus 4.8 does not. First wins on a duplicate slug, for
                        // the same reason the model list dedupes.
                        optionDescriptors =
                            models
                                .associate { it.slug to optionDescriptorsFrom(it.capabilities) }
                                .filterValues { it.isNotEmpty() },
                    )
                }
                .sortedBy { it.instance.label.lowercase() }
    }

    /**
     * Names an instance id from whatever a connected server says about it, falling
     * back to the id's own shape. A thread outlives the config that explained it,
     * so an instance the user removed still has to render with something sane.
     */
    private fun instanceFor(instanceId: String): ProviderInstance {
        sessions.value.values.forEach { connected ->
            connected.session.providers.value
                .firstOrNull { it.instanceId == instanceId }
                ?.let {
                    return ProviderInstance(
                        instanceId = it.instanceId,
                        driver = it.driver,
                        displayName = it.displayName,
                    )
                }
        }
        return providerInstanceForId(instanceId)
    }

    private fun sessionFor(environmentId: EnvironmentId): EnvironmentSession =
        sessions.value[environmentId.value]?.session
            ?: error("That environment is no longer paired.")

    /**
     * Subscribes only to attention rows. The shell gives Home the cheap boolean;
     * the detail stream gives it the actual request needed to draw controls.
     */
    private fun reconcileHomePendingSubscriptions() {
        val wanted =
            _threads.value
                .asSequence()
                .filter {
                    it.status == club.touchtech.s5code.kotlin.model.ThreadStatus.AwaitingApproval ||
                        it.status == club.touchtech.s5code.kotlin.model.ThreadStatus.AwaitingInput
                }
                .map { "${it.environmentId.value}/${it.id.value}" }
                .toSet()

        (homePendingDetailKeys - wanted).forEach { key ->
            homePendingDetailKeys.remove(key)
            if (key !in explicitDetailKeys) dropDetailSubscription(key)
        }
        (wanted - homePendingDetailKeys).forEach { key ->
            val separator = key.indexOf('/')
            if (separator < 0) return@forEach
            homePendingDetailKeys += key
            ensureThreadSubscription(
                EnvironmentId(key.substring(0, separator)),
                ThreadId(key.substring(separator + 1)),
            )
        }
    }

    private fun publishPendingRequests() {
        _pendingRequests.value =
            homePendingDetailKeys.mapNotNull { key ->
                val detail = details[key]?.value ?: return@mapNotNull null
                val summary = detail.summary
                detail.approval?.let { approval ->
                    HomePendingRequest(
                        environmentId = summary.environmentId,
                        threadId = summary.id,
                        approval = approval,
                    )
                } ?: detail.userInput?.let { input ->
                    HomePendingRequest(
                        environmentId = summary.environmentId,
                        threadId = summary.id,
                        userInput = input,
                    )
                }
            }
    }

    /** Forces every paired environment onto a fresh socket after app foreground. */
    override fun refreshConnections() {
        sessions.value.values.forEach { it.session.refreshAfterForeground() }
        clock.value = System.currentTimeMillis()
        publish()
    }

    /** Retries one environment's connection now, for the connections screen. */
    fun retry(environmentId: EnvironmentId) {
        sessions.value[environmentId.value]?.session?.retryNow()
    }

    /* ── Thread detail ───────────────────────────────────────────────── */

    override fun thread(environmentId: EnvironmentId, id: ThreadId): StateFlow<ThreadDetail?> {
        val key = "${environmentId.value}/${id.value}"
        explicitDetailKeys += key
        return ensureThreadSubscription(environmentId, id)
    }

    override fun threadSyncPhase(
        environmentId: EnvironmentId,
        id: ThreadId,
    ): StateFlow<ThreadSyncPhase> {
        val key = "${environmentId.value}/${id.value}"
        return detailSyncPhases.getOrPut(key) { MutableStateFlow(ThreadSyncPhase.Loading) }.asStateFlow()
    }

    override fun setPendingThreadCreations(keys: Set<String>) {
        val visible =
            (threads.value.asSequence() + archived.value.asSequence())
                .mapTo(mutableSetOf()) { "${it.environmentId.value}/${it.id.value}" }
        pendingCreationKeys.update { expected -> (expected + keys) - visible }
    }

    override fun discardPendingThreadCreation(key: String) {
        pendingCreationKeys.update { it - key }
    }

    private fun ensureThreadSubscription(
        environmentId: EnvironmentId,
        id: ThreadId,
    ): StateFlow<ThreadDetail?> {
        val key = "${environmentId.value}/${id.value}"
        val flow = details.getOrPut(key) { MutableStateFlow(null) }
        val sync = detailSyncPhases.getOrPut(key) { MutableStateFlow(ThreadSyncPhase.Loading) }
        if (flow.value == null) {
            scope.launch {
                snapshotStore.loadThread(environmentId, id.value)?.let { cached ->
                    if (flow.compareAndSet(
                            expect = null,
                            update = threadDetailFrom(
                                environmentId,
                                cached.thread,
                                ::instanceFor,
                                clock.value,
                            ).copy(syncPhase = ThreadSyncPhase.Syncing),
                        )) {
                        sync.value = ThreadSyncPhase.Syncing
                    }
                }
            }
        }
        if (detailJobs[key]?.isActive != true) {
            detailJobs[key] = scope.launch { subscribeThread(environmentId, id, key, flow) }
        }
        return flow.asStateFlow()
    }

    private fun dropDetailSubscription(key: String) {
        detailJobs.remove(key)?.cancel()
        details.remove(key)
        detailSyncPhases.remove(key)
    }

    /**
     * Streams one thread. The wire snapshot is kept alongside the projected detail
     * because events fold onto the DTO, not onto presentation models: re-deriving
     * "what the transcript looks like" from a mutated view model would lose the
     * fields the UI does not render but the reducer needs.
     */
    private suspend fun subscribeThread(
        environmentId: EnvironmentId,
        id: ThreadId,
        key: String,
        target: MutableStateFlow<ThreadDetail?>,
    ) {
        var snapshot: ThreadDto? = null
        var page: club.touchtech.s5code.kotlin.transport.wire.ThreadDetailPageDto? = null
        val sync = detailSyncPhases.getOrPut(key) { MutableStateFlow(ThreadSyncPhase.Loading) }
        // A fresh subscription is either loading from scratch or reconciling the
        // cached/live detail already visible from the prior connection.
        sync.value = if (target.value == null) ThreadSyncPhase.Loading else ThreadSyncPhase.Syncing
        combine(
                sessionFor(environmentId).subscribe(
                    WsMethods.OrchestrationSubscribeThread,
                    buildJsonObject {
                        put("threadId", id.value)
                        put("requestCompletionMarker", true)
                    },
                    ThreadStreamItemDto.serializer(),
                ),
                clock,
            ) { item, now ->
                item to now
            }
            .retryWhen { cause, _ ->
                // Navigation intentionally races the durable outbox drain. Until
                // bootstrap creates this generated id, subscribeThread answers
                // "not found"; that is expected queue latency, not a fatal
                // coroutine failure. Retry on shell projection, or finish quietly
                // if dispatch rejects the creation and the outbox drops ownership.
                if (key !in pendingCreationKeys.value || !isPendingCreationMissingThread(cause, id)) {
                    false
                } else {
                    combine(threads, pendingCreationKeys) { summaries, pending ->
                            summaries.any {
                                it.environmentId == environmentId && it.id == id
                            } to (key in pending)
                        }
                        .first { (visible, expected) -> visible || !expected }
                        .first
                }
            }
            .catch { cause ->
                // A malformed/stale deep link can also address a missing thread.
                // The screen's empty/loading state owns that outcome; never let a
                // read-only subscription failure cancel the app ViewModel scope.
                if (cause is CancellationException) throw cause
                if (!isPendingCreationMissingThread(cause, id)) return@catch
            }
            .collect { (item, now) ->
                when (item.kind) {
                    "snapshot" -> {
                        sync.value =
                            if (target.value == null) ThreadSyncPhase.Loading
                            else ThreadSyncPhase.Syncing
                        snapshot = item.snapshot?.thread
                        page = item.snapshot?.page
                        snapshot?.let { snapshotStore.saveThread(environmentId, it, page) }
                    }
                    "synchronized" -> sync.value = ThreadSyncPhase.Live
                    "event" -> {
                        val current = snapshot
                        val event = item.event
                        if (current != null && event != null) {
                            when (val result = applyThreadEvent(current, event)) {
                                is ThreadReduction.Updated -> snapshot = result.thread
                                ThreadReduction.Deleted -> {
                                    snapshot = null
                                    target.value = null
                                    snapshotStore.removeThread(environmentId, id.value)
                                }
                                ThreadReduction.Unchanged -> Unit
                            }
                        }
                    }
                }
                snapshot?.let { thread ->
                    if (item.kind == "event") snapshotStore.saveThread(environmentId, thread, page)
                    val projected = threadDetailFrom(environmentId, thread, ::instanceFor, now)
                    target.value = projected.copy(syncPhase = sync.value)
                    if (key in homePendingDetailKeys) publishPendingRequests()
                }
            }
    }

    /** The wire thread behind an open detail, for commands that need turn ids. */
    private fun latestTurnId(environmentId: EnvironmentId, id: ThreadId): String? =
        threads.value
            .firstOrNull { it.environmentId == environmentId && it.id == id }
            ?.let { summary ->
                sessions.value[environmentId.value]
                    ?.shell
                    ?.value
                    ?.threads
                    ?.firstOrNull { it.id == id.value }
                    ?.latestTurn
                    ?.turnId
            }

    /* ── Writes ──────────────────────────────────────────────────────── */

    private suspend fun dispatch(environmentId: EnvironmentId, command: JsonObject) {
        sessionFor(environmentId)
            .request(
                WsMethods.OrchestrationDispatchCommand,
                command,
                DispatchResultDto.serializer(),
            )
    }

    override suspend fun sendMessage(
        environmentId: EnvironmentId,
        id: ThreadId,
        text: String,
        attachments: List<ComposerAttachment>,
        settings: ThreadSettings?,
        delivery: TurnDeliveryMetadata?,
    ) {
        val detail = details["${environmentId.value}/${id.value}"]?.value
        val effective = settings ?: detail?.settings ?: ThreadSettings()
        val current = detail?.settings
        // Mirror RN's outbox drain: synchronize metadata first when the staged
        // composer model differs, then include that same selection on turn.start.
        if (settings != null && current != null && settings != current) {
            updateThreadSettings(
                environmentId = environmentId,
                id = id,
                settings = settings,
                commandIdPrefix = delivery?.commandId,
                createdAt = delivery?.createdAt,
            )
        }
        dispatch(
            environmentId,
            Commands.startTurn(
                threadId = id.value,
                text = text,
                attachments = attachments,
                attachmentDataUrls = encodeAttachments(context, attachments),
                settings = effective,
                commandId = delivery?.commandId ?: Commands.newCommandId(),
                messageId = delivery?.messageId ?: UUID.randomUUID().toString(),
                createdAt = delivery?.createdAt ?: java.time.Instant.now().toString(),
            ),
        )
    }

    override suspend fun cancelTurn(environmentId: EnvironmentId, id: ThreadId) {
        dispatch(environmentId, Commands.interruptTurn(id.value, latestTurnId(environmentId, id)))
    }

    override suspend fun respondToApproval(
        environmentId: EnvironmentId,
        id: ThreadId,
        approvalId: String,
        decision: ApprovalDecision,
    ) {
        dispatch(
            environmentId,
            Commands.respondToApproval(
                threadId = id.value,
                requestId = approvalId,
                decision =
                    when (decision) {
                        ApprovalDecision.AllowOnce -> "accept"
                        ApprovalDecision.AllowAlways -> "acceptForSession"
                        ApprovalDecision.Deny -> "decline"
                    },
            ),
        )
    }

    override suspend fun respondToInput(
        environmentId: EnvironmentId,
        id: ThreadId,
        inputId: String,
        answers: Map<String, UserInputAnswer>,
    ) {
        dispatch(
            environmentId,
            Commands.respondToUserInput(
                threadId = id.value,
                requestId = inputId,
                answers = answers,
            ),
        )
    }

    override suspend fun searchThreads(
        environmentIds: Set<EnvironmentId>,
        query: String,
        limitPerEnvironment: Int,
    ): List<ThreadSearchMatch> {
        val normalized = query.trim()
        if (normalized.length < 2) return emptyList()
        return supervisorScope {
            environmentIds.map { environmentId ->
                async {
                    runCatching {
                            sessionFor(environmentId)
                                .request(
                                    WsMethods.OrchestrationSearchThreads,
                                    buildJsonObject {
                                        put("query", normalized.take(200))
                                        put("limit", limitPerEnvironment.coerceIn(1, 50))
                                    },
                                    SearchThreadsResultDto.serializer(),
                                )
                                .matches
                                .map { match ->
                                    ThreadSearchMatch(
                                        environmentId = environmentId,
                                        threadId = ThreadId(match.threadId),
                                        projectId = club.touchtech.s5code.kotlin.model.ProjectId(match.projectId),
                                        source =
                                            if (match.source == "user") {
                                                club.touchtech.s5code.kotlin.model.SearchMatchSource.UserMessage
                                            } else {
                                                club.touchtech.s5code.kotlin.model.SearchMatchSource.AssistantMessage
                                            },
                                        snippet = match.snippet,
                                        messageCreatedAt = match.messageCreatedAt,
                                    )
                                }
                        }
                        // Preserve RN's compatibility behavior: one offline or old
                        // environment cannot erase local title/project matches from
                        // every other environment.
                        .getOrDefault(emptyList())
                }
            }.awaitAll().flatten()
        }
    }

    override suspend fun createThread(
        environmentId: EnvironmentId,
        projectKey: String,
        prompt: String,
        settings: ThreadSettings,
        branch: String,
        newWorktree: Boolean,
        attachments: List<ComposerAttachment>,
        threadId: ThreadId?,
        delivery: TurnDeliveryMetadata?,
    ): ThreadId {
        val project =
            projects.value.firstOrNull {
                it.environmentId == environmentId && it.id.value == projectKey
            } ?: error("That project is no longer available.")
        val resolvedThreadId = threadId?.value ?: UUID.randomUUID().toString()
        dispatch(
            environmentId,
            Commands.startTurnBootstrapping(
                threadId = resolvedThreadId,
                projectId = projectKey,
                projectCwd = project.workspaceRoot,
                title = titleFromPrompt(prompt),
                text = prompt,
                attachments = attachments,
                attachmentDataUrls = encodeAttachments(context, attachments),
                instanceId = settings.provider.instanceId,
                model = settings.model,
                options = settings.options,
                runtimeMode = settings.approvalPolicy.toRuntimeMode(),
                interactionMode = if (settings.runtimeMode == RuntimeMode.Plan) "plan" else "default",
                branch = branch.takeIf { it.isNotBlank() },
                newWorktree = newWorktree,
                commandId = delivery?.commandId ?: Commands.newCommandId(),
                messageId = delivery?.messageId ?: UUID.randomUUID().toString(),
                createdAt = delivery?.createdAt ?: java.time.Instant.now().toString(),
            ),
        )
        return ThreadId(resolvedThreadId)
    }

    override suspend fun updateThreadSettings(
        environmentId: EnvironmentId,
        id: ThreadId,
        settings: ThreadSettings,
    ) = updateThreadSettings(environmentId, id, settings, commandIdPrefix = null, createdAt = null)

    private suspend fun updateThreadSettings(
        environmentId: EnvironmentId,
        id: ThreadId,
        settings: ThreadSettings,
        commandIdPrefix: String?,
        createdAt: String?,
    ) {
        // Three separate commands because the server models them separately: the
        // model lives on thread metadata, the permission level is the runtime mode,
        // and plan vs default is the interaction mode.
        dispatch(
            environmentId,
            Commands.updateMeta(
                threadId = id.value,
                instanceId = settings.provider.instanceId,
                model = settings.model,
                options = settings.options,
                commandId = commandIdPrefix?.let { "$it:model-selection" } ?: Commands.newCommandId(),
            ),
        )
        dispatch(
            environmentId,
            Commands.setRuntimeMode(
                threadId = id.value,
                runtimeMode = settings.approvalPolicy.toRuntimeMode(),
                commandId = commandIdPrefix?.let { "$it:runtime-mode" } ?: Commands.newCommandId(),
                createdAt = createdAt ?: java.time.Instant.now().toString(),
            ),
        )
        dispatch(
            environmentId,
            Commands.setInteractionMode(
                threadId = id.value,
                interactionMode = if (settings.runtimeMode == RuntimeMode.Plan) "plan" else "default",
                commandId = commandIdPrefix?.let { "$it:interaction-mode" } ?: Commands.newCommandId(),
                createdAt = createdAt ?: java.time.Instant.now().toString(),
            ),
        )
    }

    override suspend fun regenerateTitle(environmentId: EnvironmentId, id: ThreadId) {
        dispatch(environmentId, Commands.updateMeta(threadId = id.value, regenerateTitle = true))
    }

    override suspend fun setPinned(environmentId: EnvironmentId, id: ThreadId, pinned: Boolean) {
        dispatch(
            environmentId,
            Commands.lifecycle(if (pinned) "thread.pin" else "thread.unpin", id.value),
        )
    }

    override suspend fun setArchived(environmentId: EnvironmentId, id: ThreadId, archived: Boolean) {
        dispatch(
            environmentId,
            Commands.lifecycle(if (archived) "thread.archive" else "thread.unarchive", id.value),
        )
    }

    override suspend fun setSettled(environmentId: EnvironmentId, id: ThreadId, settled: Boolean) {
        if (settled) {
            dispatch(environmentId, Commands.lifecycle("thread.settle", id.value))
        } else {
            dispatch(environmentId, Commands.lifecycleByUser("thread.unsettle", id.value))
        }
    }

    override suspend fun setSnoozed(
        environmentId: EnvironmentId,
        id: ThreadId,
        snoozed: Boolean,
        untilIso: String?,
    ) {
        if (snoozed) {
            dispatch(environmentId, Commands.snooze(id.value, untilIso ?: tomorrowMorningIso()))
        } else {
            dispatch(environmentId, Commands.lifecycleByUser("thread.unsnooze", id.value))
        }
    }

    override suspend fun deleteThread(environmentId: EnvironmentId, id: ThreadId) {
        dispatch(environmentId, Commands.lifecycle("thread.delete", id.value))
        val key = "${environmentId.value}/${id.value}"
        homePendingDetailKeys.remove(key)
        explicitDetailKeys.remove(key)
        dropDetailSubscription(key)
        snapshotStore.removeThread(environmentId, id.value)
    }

    override suspend fun revertToCheckpoint(
        environmentId: EnvironmentId,
        id: ThreadId,
        turnCount: Int,
    ) {
        dispatch(environmentId, Commands.revertCheckpoint(id.value, turnCount))
    }

    /* ── Tool reads ──────────────────────────────────────────────────── */

    /**
     * The directory a tool call runs in: the thread's worktree when it has one,
     * otherwise its project's root. Every git, file, review, and terminal RPC is
     * addressed this way rather than by thread id.
     */
    private fun cwdFor(environmentId: EnvironmentId, id: ThreadId): String {
        val shell =
            sessions.value[environmentId.value]?.shell?.value?.threads?.firstOrNull {
                it.id == id.value
            }
        shell?.worktreePath?.let { return it }
        val projectId = shell?.projectId
        return projects.value
            .firstOrNull { it.environmentId == environmentId && it.id.value == projectId }
            ?.workspaceRoot
            ?: error("That thread has no workspace on this environment.")
    }

    override suspend fun files(
        environmentId: EnvironmentId,
        id: ThreadId,
        path: String?,
    ): FileNode {
        val root = cwdFor(environmentId, id)
        val cwd = path?.let { "$root/${it.trimStart('/')}" } ?: root
        val result =
            sessionFor(environmentId)
                .request(
                    WsMethods.ProjectsListEntries,
                    buildJsonObject { put("cwd", cwd) },
                    ProjectListEntriesResultDto.serializer(),
                )
        // `projects.listEntries` is a flat recursive path index. Rebuild directory
        // ancestry once so expanding and filtering operate on a real tree.
        return buildFileTree(
            rootPath = path.orEmpty(),
            rootName = path?.substringAfterLast('/') ?: root.substringAfterLast('/'),
            entries = result.entries.map { it.path to (it.kind == "directory") },
            truncated = result.truncated,
        )
    }

    override suspend fun sourceFile(
        environmentId: EnvironmentId,
        id: ThreadId,
        path: String,
    ): SourceFile =
        workspaceFileCache.source(environmentId, id, path) {
            readSourceFile(environmentId, id, path)
        }

    private suspend fun readSourceFile(
        environmentId: EnvironmentId,
        id: ThreadId,
        path: String,
    ): SourceFile {
        val result =
            sessionFor(environmentId)
                .request(
                    WsMethods.ProjectsReadFile,
                    buildJsonObject {
                        put("cwd", cwdFor(environmentId, id))
                        put("relativePath", path)
                    },
                    ProjectReadFileResultDto.serializer(),
                )
        return SourceFile(
            path = result.relativePath,
            language = languageOf(result.relativePath),
            lines = result.contents.lines(),
            byteLength = result.byteLength,
            truncated = result.truncated,
        )
    }

    /**
     * Signs a URL for one workspace file.
     *
     * The relative URL the server returns is resolved against the endpoint the
     * session is actually connected to, not the saved one: a relay-managed tunnel
     * moves, and an image URL built from a stale host is a broken image.
     */
    override suspend fun asset(
        environmentId: EnvironmentId,
        id: ThreadId,
        path: String,
    ): WorkspaceAsset =
        workspaceFileCache.asset(environmentId, id, path) {
            createAsset(environmentId, id, path)
        }

    private suspend fun createAsset(
        environmentId: EnvironmentId,
        id: ThreadId,
        path: String,
    ): WorkspaceAsset {
        val result =
            sessionFor(environmentId)
                .request(
                    WsMethods.AssetsCreateUrl,
                    buildJsonObject {
                        put(
                            "resource",
                            buildJsonObject {
                                put("_tag", "workspace-file")
                                put("threadId", id.value)
                                put("path", path)
                            },
                        )
                    },
                    AssetUrlResultDto.serializer(),
                )
        val origin =
            sessions.value[environmentId.value]?.session?.httpBaseUrl
                ?: store.environments.value
                    .firstOrNull { it.environmentId == environmentId.value }
                    ?.httpBaseUrl
                ?: error("That environment is no longer paired.")
        return WorkspaceAsset(
            url = origin.trimEnd('/') + "/" + result.relativeUrl.trimStart('/'),
            expiresAtMillis = result.expiresAt,
            sourcePath = result.sourcePath,
        )
    }

    override fun prewarmFile(environmentId: EnvironmentId, id: ThreadId, path: String) {
        scope.launch {
            runCatching {
                if (isPreviewAssetPath(path)) {
                    val asset = asset(environmentId, id, path)
                    if (isImagePreviewPath(path)) WorkspaceImageCache.preload(context, asset.url)
                } else {
                    sourceFile(environmentId, id, path)
                }
            }
        }
    }
    override suspend fun attachmentUrl(
        environmentId: EnvironmentId,
        attachmentId: String,
    ): String {
        val result =
            sessionFor(environmentId)
                .request(
                    WsMethods.AssetsCreateUrl,
                    buildJsonObject {
                        put(
                            "resource",
                            buildJsonObject {
                                put("_tag", "attachment")
                                put("attachmentId", attachmentId)
                            },
                        )
                    },
                    AssetUrlResultDto.serializer(),
                )
        val origin =
            sessions.value[environmentId.value]?.session?.httpBaseUrl
                ?: store.environments.value
                    .firstOrNull { it.environmentId == environmentId.value }
                    ?.httpBaseUrl
                ?: error("That environment is no longer paired.")
        return origin.trimEnd('/') + "/" + result.relativeUrl.trimStart('/')
    }

    override suspend fun projectIconUrl(project: Project): String? {
        val key = "${project.environmentId.value}/${project.workspaceRoot}/${project.faviconPath}"
        // Cached because the home list asks per row, on every recomposition of a
        // scrolling list. The server buckets these tokens to a half-hour boundary
        // precisely so the URL is stable enough to cache; re-signing per row would
        // put an RPC round trip in the scroll path.
        projectIconUrls[key]?.let { cached ->
            if (cached.expiresAtMillis > System.currentTimeMillis()) return cached.url
        }
        val session = sessions.value[project.environmentId.value]?.session ?: return null
        val result =
            runCatching {
                    session.request(
                        WsMethods.AssetsCreateUrl,
                        buildJsonObject {
                            put(
                                "resource",
                                buildJsonObject {
                                    put("_tag", "project-favicon")
                                    put("cwd", project.workspaceRoot)
                                    project.faviconPath?.let { put("path", it) }
                                },
                            )
                        },
                        AssetUrlResultDto.serializer(),
                    )
                }
                // A project with no icon is the common case, and the server reports it
                // as an error rather than an empty result. A row must not lose its
                // icon slot over it, so this stays a null instead of a thrown failure.
                .getOrNull()
        val url =
            result
                // The server answers a missing icon with a marker filename rather than
                // a 404, so the fallback folder has to be chosen here too.
                ?.takeIf { it.relativeUrl.substringAfterLast('/') != PROJECT_ICON_MISSING_MARKER }
                ?.let { signed ->
                    session.httpBaseUrl?.let { origin ->
                        origin.trimEnd('/') + "/" + signed.relativeUrl.trimStart('/')
                    }
                }
    // A miss is cached too, on the token's own bucket: a project with no icon
        // would otherwise re-ask on every scroll. Successful URL loads are still
        // decoded and cached by Coil; this map only avoids repeated signing RPCs.
        projectIconUrls[key] =
            CachedIconUrl(
                url = url,
                expiresAtMillis =
                    result?.expiresAt?.takeIf { it > 0 }
                        ?: (System.currentTimeMillis() + MISSING_ICON_RETRY_MS),
            )
        return url
    }

    override suspend fun review(
        environmentId: EnvironmentId,
        id: ThreadId,
        refresh: Boolean,
    ): List<ReviewFile> {
        val key = "${environmentId.value}/${id.value}"
        if (!refresh) {
            reviewCache[key]
                ?.takeIf { System.currentTimeMillis() - it.cachedAtMillis < REVIEW_CACHE_TTL_MS }
                ?.let { return it.files }
        }
        val result =
            sessionFor(environmentId)
                .request(
                    WsMethods.ReviewGetDiffPreview,
                    buildJsonObject { put("cwd", cwdFor(environmentId, id)) },
                    ReviewDiffPreviewResultDto.serializer(),
                )
        // Working-tree changes first: on a thread the agent just worked on, that is
        // what the user came to review. The branch range is the fallback for a
        // thread whose work is already committed.
        val source =
            result.sources.firstOrNull { it.kind == "working-tree" && it.diff.isNotBlank() }
                ?: result.sources.firstOrNull { it.diff.isNotBlank() }
        val files = source?.let { parseUnifiedDiff(it.diff) } ?: emptyList()
        reviewCache[key] = CachedReview(files, System.currentTimeMillis())
        return files
    }

    override suspend fun gitStatus(environmentId: EnvironmentId, id: ThreadId): GitStatus {
        val cwd = cwdFor(environmentId, id)
        val status =
            sessionFor(environmentId)
                .request(
                    WsMethods.VcsRefreshStatus,
                    buildJsonObject { put("cwd", cwd) },
                    VcsStatusDto.serializer(),
                )
        val shell =
            sessions.value[environmentId.value]?.shell?.value?.threads?.firstOrNull {
                it.id == id.value
            }
        return GitStatus(
            branch = status.refName.orEmpty(),
            // The server reports whether this is the default ref rather than naming
            // it, so the base is only known when we are not on it.
            baseBranch = if (status.isDefaultRef) status.refName.orEmpty() else "",
            ahead = status.aheadCount,
            behind = status.behindCount,
            // Git's index state is not in this contract: the status is
            // working-tree-shaped, so changed files land in one list rather than
            // being split into staged and unstaged we cannot distinguish.
            staged = emptyList(),
            unstaged = status.workingTree.files.map { it.path },
            untracked = emptyList(),
            worktreePath = shell?.worktreePath,
            pullRequest =
                status.pr?.let {
                    PullRequestRef(
                        number = it.number,
                        state =
                            when (it.state) {
                                "merged" -> PullRequestState.Merged
                                "closed" -> PullRequestState.Closed
                                "draft" -> PullRequestState.Draft
                                else -> PullRequestState.Open
                            },
                        title = it.title,
                    )
                },
        )
    }

    override suspend fun branches(environmentId: EnvironmentId, id: ThreadId): List<BranchRef> =
        listRefs(environmentId, cwdFor(environmentId, id))

    override suspend fun projectBranches(
        environmentId: EnvironmentId,
        projectKey: String,
    ): List<BranchRef> {
        val project =
            projects.value.firstOrNull {
                it.environmentId == environmentId && it.id.value == projectKey
            } ?: return emptyList()
        return listRefs(environmentId, project.workspaceRoot)
    }

    private suspend fun listRefs(environmentId: EnvironmentId, cwd: String): List<BranchRef> {
        val result =
            sessionFor(environmentId)
                .request(
                    WsMethods.VcsListRefs,
                    buildJsonObject {
                        put("cwd", cwd)
                        put("refKind", "local")
                        put("limit", 100)
                    },
                    VcsListRefsResultDto.serializer(),
                )
        return result.refs.map { ref ->
            BranchRef(
                name = ref.name,
                current = ref.current,
                remote = ref.isRemote,
                // The refs contract carries no commit dates, and inventing one would
                // be a lie in a list people use to pick a base branch.
                ageLabel = if (ref.isDefault) "default" else "",
            )
        }
    }

    override fun terminal(
        environmentId: EnvironmentId,
        id: ThreadId,
        terminalId: String,
        cols: Int,
        rows: Int,
    ): Flow<TerminalSession> {
        // `terminal.attach` opens the session when there is none and reattaches when
        // there is, which is why there is no separate open call here. `cwd` is only
        // consulted on the opening path, but it is required then, so it is always
        // sent. `restartIfNotRunning` is what makes reopening a thread whose shell
        // exited give a live prompt rather than a dead transcript.
        val rawHistory = StringBuilder()
        var title = "Terminal"
        var cwd = ""
        var status = TerminalStatus.Starting
        var error: String? = null
        return flow {
            val location = terminalLocation(environmentId, id)
            sessionFor(environmentId)
                .subscribe(
                    WsMethods.TerminalAttach,
                    buildJsonObject {
                        put("threadId", id.value)
                        put("terminalId", terminalId)
                        put("cwd", location.first)
                        location.second?.let { put("worktreePath", it) }
                        put("cols", cols)
                        put("rows", rows)
                        put("restartIfNotRunning", true)
                    },
                    TerminalStreamEventDto.serializer(),
                )
                .collect { event ->
                    when (event.type) {
                        // A snapshot replaces the buffer outright: it is the whole
                        // scrollback, so folding it onto what is already there would
                        // double the transcript on every reconnect.
                        "snapshot",
                        "restarted" -> {
                            val snapshot = event.snapshot ?: return@collect
                            rawHistory.clear()
                            rawHistory.append(snapshot.history)
                            title = snapshot.label.ifBlank { "Terminal" }
                            cwd = snapshot.cwd
                            status = terminalStatusOf(snapshot.status)
                            error = null
                        }
                        "output" -> {
                            val data = event.data.orEmpty()
                            rawHistory.append(data)
                            if (status == TerminalStatus.Closed) status = TerminalStatus.Running
                            error = null
                        }
                        "cleared" -> {
                            rawHistory.clear()
                            error = null
                        }
                        "exited" -> status = TerminalStatus.Exited
                        "closed" -> status = TerminalStatus.Closed
                        "error" -> {
                            status = TerminalStatus.Error
                            error = event.message
                        }
                        // Activity only changes the label the server computes.
                        "activity" -> event.label?.takeIf { it.isNotBlank() }?.let { title = it }
                        else -> return@collect
                    }
                    emit(
                        TerminalSession(
                            id = terminalId,
                            title = title,
                            cwd = cwd,
                            status = status,
                            buffer = rawHistory.toString(),
                            lines = emptyList(),
                            error = error,
                        )
                    )
                }
        }
    }

    /**
     * Where a new shell starts, from `resolveTerminalOpenLocation` in
     * `apps/mobile/src/features/terminal/terminalLaunchContext.ts`: the thread's
     * worktree when it has one, otherwise the project root.
     */
    private fun terminalLocation(
        environmentId: EnvironmentId,
        id: ThreadId,
    ): Pair<String, String?> {
        val worktree =
            sessions.value[environmentId.value]
                ?.shell
                ?.value
                ?.threads
                ?.firstOrNull { it.id == id.value }
                ?.worktreePath
        return cwdFor(environmentId, id) to worktree
    }

    override suspend fun searchPaths(
        environmentId: EnvironmentId,
        id: ThreadId,
        query: String,
        limit: Int,
    ): List<String> {
        val result =
            runCatching {
                    sessionFor(environmentId)
                        .request(
                            WsMethods.ProjectsSearchEntries,
                            buildJsonObject {
                                put("cwd", cwdFor(environmentId, id))
                                put("query", query)
                                put("limit", limit)
                            },
                            ProjectListEntriesResultDto.serializer(),
                        )
                }
                .getOrNull() ?: return emptyList()
        return result.entries.map { it.path }
    }

    override suspend fun usage(window: UsageWindow): Usage {
        val zone = java.time.ZoneId.systemDefault()
        val now = java.time.Instant.now()
        val until = java.time.LocalDate.now(zone)
        // Calendar arithmetic on the local end day rather than subtracting fixed
        // milliseconds: around a DST change a fixed offset lands on the wrong day.
        val since = until.minusDays((window.days - 1).toLong())
        // The rolling day is minute-aligned so hour labels stay readable while the
        // window is still exactly 24 hours — the same bounds `makeWindow` builds.
        val untilTime = now.truncatedTo(java.time.temporal.ChronoUnit.MINUTES)
        val sinceTime = untilTime.minus(java.time.Duration.ofHours(24))
        val payload = buildJsonObject {
            if (window.hourly) {
                put("sinceDay", sinceTime.atZone(zone).toLocalDate().toString())
                put("untilDay", untilTime.atZone(zone).toLocalDate().toString())
                put("resolution", "hour")
                put("sinceTime", sinceTime.toString())
                put("untilTime", untilTime.toString())
            } else {
                put("sinceDay", since.toString())
                put("untilDay", until.toString())
                put("resolution", "day")
            }
            put("timeZone", zone.id)
        }

        // Usage is read per environment and merged. Providers scan their own
        // machine's transcripts, so two environments legitimately report different
        // work and the totals are a sum, not a max.
        val summaries =
            sessions.value.values.mapNotNull { connected ->
                if (connected.session.state.value.phase != SessionPhase.Connected) return@mapNotNull null
                runCatching {
                        connected.session.request(
                            WsMethods.ServerGetUsageSummary,
                            payload,
                            UsageSummaryDto.serializer(),
                        )
                    }
                    .getOrNull()
                    ?.let { connected.session.label.value to it }
            }

        val buckets = summaries.flatMap { it.second.buckets }
        val totals =
            UsageTotals(
                costUsd = buckets.sumOf { it.costUsd },
                inputTokens =
                    buckets.sumOf { it.totals.uncachedInputTokens + it.totals.cachedInputTokens },
                outputTokens = buckets.sumOf { it.totals.outputTokens },
                cacheReadTokens = buckets.sumOf { it.totals.cachedInputTokens },
                requests = buckets.sumOf { it.records },
            )
        val totalTokensAll = buckets.sumOf { it.tokenTotal() }.coerceAtLeast(1)

        return Usage(
            totals = totals,
            window = window,
            days = usageSeries(buckets, window, zone, now),
            providers =
                buckets
                    .groupBy { usageProviderFor(it.provider) }
                    .map { (provider, providerBuckets) ->
                        UsageProviderBreakdown(
                            provider = provider,
                            costUsd = providerBuckets.sumOf { it.costUsd },
                            tokens = providerBuckets.sumOf { it.tokenTotal() },
                            share =
                                (providerBuckets.sumOf { it.tokenTotal() }.toDouble() /
                                        totalTokensAll.toDouble())
                                    .toFloat(),
                            models =
                                providerBuckets
                                    .groupBy { it.model }
                                    .map { (model, modelBuckets) ->
                                        UsageModelBreakdown(
                                            model = model,
                                            costUsd = modelBuckets.sumOf { it.costUsd },
                                            tokens = modelBuckets.sumOf { it.tokenTotal() },
                                        )
                                    }
                                    .sortedByDescending { it.costUsd },
                        )
                    }
                    .sortedByDescending { it.costUsd },
            environments = summaries.map { it.first },
        )
    }

    override suspend fun repositories(
        environmentId: EnvironmentId,
        query: String,
    ): List<RepositoryRef> {
        val reference = query.trim()
        // There is no repository *search* RPC; lookup validates one reference. An
        // incomplete `owner/name` is not an error worth surfacing while typing.
        if (!reference.contains('/')) return emptyList()
        val info =
            runCatching {
                    sessionFor(environmentId)
                        .request(
                            WsMethods.SourceControlLookupRepository,
                            buildJsonObject {
                                put("provider", "github")
                                put("repository", reference)
                            },
                            SourceControlRepositoryDto.serializer(),
                        )
                }
                .getOrNull() ?: return emptyList()
        return listOf(
            RepositoryRef(fullName = info.nameWithOwner, description = info.url, private = false)
        )
    }

    override suspend fun remotePaths(
        environmentId: EnvironmentId,
        partialPath: String,
    ): List<String> {
        val result =
            runCatching {
                    sessionFor(environmentId)
                        .request(
                            WsMethods.FilesystemBrowse,
                            buildJsonObject {
                                put("partialPath", partialPath.ifBlank { "~/" })
                            },
                            FilesystemBrowseResultDto.serializer(),
                        )
                }
                .getOrNull() ?: return emptyList()
        return result.entries.map { it.fullPath }
    }

    /* ── Projects ────────────────────────────────────────────────────── */

    override suspend fun createProject(
        environmentId: EnvironmentId,
        title: String,
        workspaceRoot: String,
        createWorkspaceRootIfMissing: Boolean,
    ) {
        dispatch(
            environmentId,
            Commands.createProject(
                projectId = UUID.randomUUID().toString(),
                title = title,
                workspaceRoot = workspaceRoot,
                createWorkspaceRootIfMissing = createWorkspaceRootIfMissing,
            ),
        )
    }

    override suspend fun cloneProject(
        environmentId: EnvironmentId,
        repository: String,
        destinationPath: String,
    ): String {
        // Two steps, in this order: the clone has to exist before a project can
        // point at it, and the server reports where it actually landed.
        val clone =
            sessionFor(environmentId)
                .request(
                    WsMethods.SourceControlCloneRepository,
                    buildJsonObject {
                        put("provider", "github")
                        put("repository", repository)
                        put("destinationPath", destinationPath)
                    },
                    SourceControlCloneResultDto.serializer(),
                )
        createProject(
            environmentId = environmentId,
            title = repository.substringAfterLast('/'),
            workspaceRoot = clone.cwd,
        )
        return clone.cwd
    }

    /* ── Git writes ──────────────────────────────────────────────────── */

    /**
     * Git actions run through `git.runStackedAction`, a streaming RPC that reports
     * progress. The UI waits for the stream to end rather than showing progress
     * step by step, because these actions are seconds long and a progress log for
     * "commit" is more noise than information.
     */
    private suspend fun runStackedAction(
        environmentId: EnvironmentId,
        id: ThreadId,
        action: String,
        commitMessage: String? = null,
        onProgress: (GitActionProgress) -> Unit = {},
    ): GitActionResult {
        val payload = buildJsonObject {
            put("actionId", UUID.randomUUID().toString())
            put("cwd", cwdFor(environmentId, id))
            put("action", action)
            if (commitMessage != null) put("commitMessage", commitMessage)
        }
        var terminal: GitActionProgressEventDto? = null
        sessionFor(environmentId)
            .subscribe(
                WsMethods.GitRunStackedAction,
                payload,
                GitActionProgressEventDto.serializer(),
            )
            .collect { event ->
                when (event.kind) {
                    "action_started" ->
                        onProgress(
                            GitActionProgress(
                                action = event.action,
                                label = "Running ${gitActionLabel(event.action)}",
                            )
                        )
                    "phase_started" ->
                        onProgress(
                            GitActionProgress(
                                action = event.action,
                                label = event.label ?: gitPhaseLabel(event.phase),
                            )
                        )
                    "hook_started" ->
                        onProgress(
                            GitActionProgress(
                                action = event.action,
                                label = "Running ${event.hookName ?: "hook"}",
                            )
                        )
                    "hook_output" ->
                        onProgress(
                            GitActionProgress(
                                action = event.action,
                                label = event.hookName?.let { "Running $it" }
                                    ?: "Running ${gitActionLabel(event.action)}",
                                description = event.text,
                            )
                        )
                    "action_finished",
                    "action_failed" -> terminal = event
                }
            }
        val completed = terminal ?: error("Source control action ended without a result.")
        if (completed.kind == "action_failed") {
            error(completed.message ?: "Source control action failed.")
        }
        val result = completed.result ?: error("Source control action returned no result.")
        val prUrl = result.pr.url ?: result.toast.cta.url
        return GitActionResult(
            action = result.action.ifBlank { action },
            title = result.toast.title,
            description = result.toast.description,
            pullRequestUrl = prUrl,
        )
    }

    override suspend fun commit(
        environmentId: EnvironmentId,
        id: ThreadId,
        message: String,
        onProgress: (GitActionProgress) -> Unit,
    ): GitActionResult = runStackedAction(environmentId, id, "commit", message, onProgress)

    override suspend fun push(
        environmentId: EnvironmentId,
        id: ThreadId,
        onProgress: (GitActionProgress) -> Unit,
    ): GitActionResult = runStackedAction(environmentId, id, "push", onProgress = onProgress)

    override suspend fun createPullRequest(
        environmentId: EnvironmentId,
        id: ThreadId,
        onProgress: (GitActionProgress) -> Unit,
    ): GitActionResult = runStackedAction(environmentId, id, "create_pr", onProgress = onProgress)

    override suspend fun pull(environmentId: EnvironmentId, id: ThreadId) {
        sessionFor(environmentId)
            .request(
                WsMethods.VcsPull,
                buildJsonObject { put("cwd", cwdFor(environmentId, id)) },
                DispatchResultDto.serializer(),
            )
    }

    override suspend fun createBranch(environmentId: EnvironmentId, id: ThreadId, name: String) {
        sessionFor(environmentId)
            .request(
                WsMethods.VcsCreateRef,
                buildJsonObject {
                    put("cwd", cwdFor(environmentId, id))
                    put("refName", name)
                    put("switchRef", true)
                },
                DispatchResultDto.serializer(),
            )
    }

    override suspend fun switchBranch(environmentId: EnvironmentId, id: ThreadId, name: String) {
        sessionFor(environmentId)
            .request(
                WsMethods.VcsSwitchRef,
                buildJsonObject {
                    put("cwd", cwdFor(environmentId, id))
                    put("refName", name)
                },
                DispatchResultDto.serializer(),
            )
    }

    override suspend fun terminalWrite(
        environmentId: EnvironmentId,
        id: ThreadId,
        terminalId: String,
        data: String,
    ) {
        // No success value on this contract, so there is nothing to decode.
        sessionFor(environmentId)
            .execute(
                WsMethods.TerminalWrite,
                buildJsonObject {
                    put("threadId", id.value)
                    put("terminalId", terminalId)
                    put("data", data)
                },
            )
    }

    override suspend fun terminalResize(
        environmentId: EnvironmentId,
        id: ThreadId,
        terminalId: String,
        cols: Int,
        rows: Int,
    ) {
        sessionFor(environmentId)
            .execute(
                WsMethods.TerminalResize,
                buildJsonObject {
                    put("threadId", id.value)
                    put("terminalId", terminalId)
                    put("cols", cols)
                    put("rows", rows)
                },
            )
    }

    override suspend fun terminalClear(
        environmentId: EnvironmentId,
        id: ThreadId,
        terminalId: String,
    ) {
        sessionFor(environmentId)
            .execute(
                WsMethods.TerminalClear,
                buildJsonObject {
                    put("threadId", id.value)
                    put("terminalId", terminalId)
                },
            )
    }

    override suspend fun terminalRestart(
        environmentId: EnvironmentId,
        id: ThreadId,
        terminalId: String,
        cols: Int,
        rows: Int,
    ) {
        val location = terminalLocation(environmentId, id)
        sessionFor(environmentId)
            .request(
                WsMethods.TerminalRestart,
                buildJsonObject {
                    put("threadId", id.value)
                    put("terminalId", terminalId)
                    put("cwd", location.first)
                    location.second?.let { put("worktreePath", it) }
                    put("cols", cols)
                    put("rows", rows)
                },
                TerminalSnapshotDto.serializer(),
            )
    }

    override suspend fun terminalClose(
        environmentId: EnvironmentId,
        id: ThreadId,
        terminalId: String,
        deleteHistory: Boolean,
    ) {
        sessionFor(environmentId)
            .execute(
                WsMethods.TerminalClose,
                buildJsonObject {
                    put("threadId", id.value)
                    put("terminalId", terminalId)
                    put("deleteHistory", deleteHistory)
                },
            )
    }

    private companion object {

        /**
         * Filename the server signs when a project has no icon, from
         * `PROJECT_FAVICON_FALLBACK_MARKER` in `packages/shared/src/projectFavicon.ts`.
         */
        const val PROJECT_ICON_MISSING_MARKER = "project-favicon-missing"

        /** Cached long enough to cross navigation/prewarm, short enough for active edits. */
        const val REVIEW_CACHE_TTL_MS = 30_000L

        /** How long a project with no icon is remembered before asking again. */
        const val MISSING_ICON_RETRY_MS = 30 * 60 * 1_000L

        fun gitActionLabel(action: String): String =
            when (action) {
                "commit" -> "commit"
                "push" -> "push"
                "create_pr" -> "pull request"
                "commit_push" -> "commit and push"
                "commit_push_pr" -> "commit, push, and pull request"
                else -> "source control action"
            }

        fun gitPhaseLabel(phase: String?): String =
            when (phase) {
                "branch" -> "Preparing branch"
                "commit" -> "Creating commit"
                "push" -> "Pushing branch"
                "pr" -> "Creating pull request"
                else -> "Running source control action"
            }

        fun titleFromPrompt(prompt: String): String {
            val compact = prompt.trim().replace(Regex("\\s+"), " ")
            return when {
                compact.isEmpty() -> "New thread"
                compact.length <= 72 -> compact
                else -> compact.take(69).trimEnd() + "..."
            }
        }

        fun languageOf(path: String): String =
            when (path.substringAfterLast('.', "")) {
                "kt", "kts" -> "kotlin"
                "ts", "tsx" -> "typescript"
                "js", "jsx" -> "javascript"
                "swift" -> "swift"
                "py" -> "python"
                "rs" -> "rust"
                "go" -> "go"
                "json" -> "json"
                "md" -> "markdown"
                "sh", "bash" -> "bash"
                "yml", "yaml" -> "yaml"
                else -> "text"
            }

        /**
         * Usage buckets name providers by their own vocabulary, which is a driver
         * slug rather than an instance id. The bucket carries no instance, so the
         * slug stands in for one: it is only ever rendered, never routed on.
         */
        fun usageProviderFor(provider: String): ProviderInstance =
            ProviderInstance(instanceId = provider, driver = provider)
    }
}

internal fun buildFileTree(
    rootPath: String,
    rootName: String,
    entries: List<Pair<String, Boolean>>,
    truncated: Boolean = false,
): FileNode {
    data class MutableNode(
        val path: String,
        val name: String,
        var isDirectory: Boolean,
        val children: LinkedHashMap<String, MutableNode> = linkedMapOf(),
    )

    val normalizedRoot = rootPath.trim('/')
    val root = MutableNode(normalizedRoot, rootName, true)
    entries.forEach { (rawPath, directory) ->
        val normalized = rawPath.trim('/').takeIf(String::isNotEmpty) ?: return@forEach
        val relative =
            when {
                normalizedRoot.isBlank() -> normalized
                normalized == normalizedRoot -> return@forEach
                normalized.startsWith("$normalizedRoot/") ->
                    normalized.removePrefix("$normalizedRoot/")
                else -> normalized
            }
        val segments = relative.split('/').filter(String::isNotEmpty)
        var parent = root
        segments.forEachIndexed { index, segment ->
            val currentPath =
                listOf(normalizedRoot, segments.take(index + 1).joinToString("/"))
                    .filter(String::isNotEmpty)
                    .joinToString("/")
            parent =
                parent.children.getOrPut(segment) {
                    MutableNode(
                        path = currentPath,
                        name = segment,
                        isDirectory = index < segments.lastIndex || directory,
                    )
                }
            if (index < segments.lastIndex || directory) parent.isDirectory = true
        }
    }

    fun freeze(node: MutableNode): FileNode =
        FileNode(
            path = node.path,
            name = node.name,
            isDirectory = node.isDirectory,
            children =
                node.children.values
                    .sortedWith(
                        compareByDescending<MutableNode> { it.isDirectory }
                            .thenBy(String.CASE_INSENSITIVE_ORDER) { it.name }
                    )
                    .map(::freeze),
            truncated = node === root && truncated,
        )
    return freeze(root)
}

private fun isImagePreviewPath(path: String): Boolean =
    path.lowercase().let { lower ->
        listOf(".avif", ".bmp", ".gif", ".ico", ".jpeg", ".jpg", ".png", ".svg", ".webp")
            .any(lower::endsWith)
    }

private fun isPreviewAssetPath(path: String): Boolean =
    isImagePreviewPath(path) ||
        path.lowercase().let { it.endsWith(".htm") || it.endsWith(".html") || it.endsWith(".pdf") }

/**
 * The chart series for a window.
 *
 * Buckets are grouped by whichever key the window asked for — `hourStart`
 * for the rolling day, `day` otherwise — and every slot in the window is
 * emitted, zero-filled. That zero-filling is the point: a quiet Tuesday has
 * no bucket at all, and a chart built only from what came back silently
 * closes the gap and misreports the shape of the week.
 */
internal fun usageSeries(
    buckets: List<club.touchtech.s5code.kotlin.transport.wire.UsageBucketDto>,
    window: UsageWindow,
    zone: java.time.ZoneId,
    now: java.time.Instant,
): List<UsageDay> {
    val grouped =
        buckets.groupBy { bucket ->
            if (window.hourly) {
                bucket.hourStart?.let { parseInstant(it) }?.let { millis ->
                    java.time.Instant.ofEpochMilli(millis)
                        .truncatedTo(java.time.temporal.ChronoUnit.HOURS)
                        .toString()
                } ?: bucket.day
            } else {
                bucket.day
            }
        }
    val slots =
        if (window.hourly) {
            val end = now.truncatedTo(java.time.temporal.ChronoUnit.HOURS)
            (23 downTo 0).map { hoursAgo ->
                end.minus(java.time.Duration.ofHours(hoursAgo.toLong())).toString()
            }
        } else {
            val end = now.atZone(zone).toLocalDate()
            (window.days - 1 downTo 0).map { daysAgo -> end.minusDays(daysAgo.toLong()).toString() }
        }
    return slots.map { slot ->
        val slotBuckets = grouped[slot].orEmpty()
        UsageDay(
            label = usageSlotLabel(slot, window, zone),
            costUsd = slotBuckets.sumOf { it.costUsd },
            tokens = slotBuckets.sumOf { it.tokenTotal() },
        )
    }
}

/** Hour of day for the rolling window, `MM-DD` for the longer ones. */
private fun usageSlotLabel(
    slot: String,
    window: UsageWindow,
    zone: java.time.ZoneId,
): String =
    if (window.hourly) {
        parseInstant(slot)?.let { millis ->
            java.time.format.DateTimeFormatter.ofPattern("HH")
                .withZone(zone)
                .format(java.time.Instant.ofEpochMilli(millis))
        } ?: slot
    } else {
        slot.takeLast(5)
    }

private fun club.touchtech.s5code.kotlin.transport.wire.UsageBucketDto.tokenTotal(): Long =
    totals.uncachedInputTokens +
        totals.cachedInputTokens +
        totals.cacheCreationTokens +
        totals.outputTokens
