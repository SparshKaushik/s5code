package club.touchtech.s5code.kotlin.app

import android.app.Application
import androidx.compose.runtime.Immutable
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import club.touchtech.s5code.kotlin.cloud.CloudAuth
import club.touchtech.s5code.kotlin.cloud.CloudEnvironments
import club.touchtech.s5code.kotlin.cloud.CloudPublicConfig
import club.touchtech.s5code.kotlin.cloud.DpopKey
import club.touchtech.s5code.kotlin.cloud.RelayClient
import club.touchtech.s5code.kotlin.cloud.RelayEnvironmentAuthorizer
import club.touchtech.s5code.kotlin.data.ClientStateStore
import club.touchtech.s5code.kotlin.data.EnvironmentStore
import club.touchtech.s5code.kotlin.data.IncomingShareDestination
import club.touchtech.s5code.kotlin.data.IncomingShareDraft
import club.touchtech.s5code.kotlin.data.IncomingShareStore
import club.touchtech.s5code.kotlin.data.IncomingShareAttachmentType
import club.touchtech.s5code.kotlin.data.hasIncomingShareContent
import club.touchtech.s5code.kotlin.data.incomingShareIdFor
import club.touchtech.s5code.kotlin.data.selectIncomingShareAttachments
import club.touchtech.s5code.kotlin.data.RuntimePreferences
import club.touchtech.s5code.kotlin.data.StoredDraft
import club.touchtech.s5code.kotlin.data.StoredNewTaskDraft
import club.touchtech.s5code.kotlin.data.StoredRecentThread
import club.touchtech.s5code.kotlin.data.storedApprovalPolicy
import club.touchtech.s5code.kotlin.data.storedProvider
import club.touchtech.s5code.kotlin.data.storedProviderOptions
import club.touchtech.s5code.kotlin.data.storedRuntimeMode
import club.touchtech.s5code.kotlin.data.storedWorkspaceMode
import club.touchtech.s5code.kotlin.data.toRuntime
import club.touchtech.s5code.kotlin.data.toRuntimeThreadSettings
import club.touchtech.s5code.kotlin.data.toStoredThreadSettings
import club.touchtech.s5code.kotlin.data.toStored
import club.touchtech.s5code.kotlin.data.LiveWorkspaceGateway
import club.touchtech.s5code.kotlin.data.QueuedThreadMessage
import club.touchtech.s5code.kotlin.data.ThreadOutboxStore
import club.touchtech.s5code.kotlin.data.WorkspaceGateway
import club.touchtech.s5code.kotlin.data.WorkspaceSnapshotStore
import club.touchtech.s5code.kotlin.data.acceptComposerImages
import club.touchtech.s5code.kotlin.data.duplicateCreationAcknowledgesDelivery
import club.touchtech.s5code.kotlin.data.newQueuedThreadMessage
import club.touchtech.s5code.kotlin.data.queuedCreationAlreadyExists
import club.touchtech.s5code.kotlin.data.threadOutboxRetryDelayMillis
import club.touchtech.s5code.kotlin.design.theme.S5ThemeMode
import club.touchtech.s5code.kotlin.model.ActionProgress
import club.touchtech.s5code.kotlin.model.ActionProgressPhase
import club.touchtech.s5code.kotlin.model.AppErrorNotice
import club.touchtech.s5code.kotlin.model.ApprovalPolicy
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import club.touchtech.s5code.kotlin.model.ComposerImageCandidate
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.ModelFavorite
import club.touchtech.s5code.kotlin.model.ProjectGrouping
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.QuestionAttachment
import club.touchtech.s5code.kotlin.model.QuestionAttachmentStatus
import club.touchtech.s5code.kotlin.model.RuntimeMode
import club.touchtech.s5code.kotlin.model.ThreadFilter
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadSettings
import club.touchtech.s5code.kotlin.model.ThreadSort
import club.touchtech.s5code.kotlin.model.WorkspaceMode
import club.touchtech.s5code.kotlin.platform.StagedQuestionFile
import club.touchtech.s5code.kotlin.platform.notifications.AndroidLiveUpdateNotifications
import club.touchtech.s5code.kotlin.platform.buildIncomingShare
import club.touchtech.s5code.kotlin.platform.materializeComposerImages
import club.touchtech.s5code.kotlin.platform.notifications.PushRegistrationCoordinator
import club.touchtech.s5code.kotlin.platform.notifications.PushRegistrationStatus
import club.touchtech.s5code.kotlin.platform.notifications.PushRuntime
import club.touchtech.s5code.kotlin.platform.updates.AppUpdateManager
import club.touchtech.s5code.kotlin.transport.DirectEnvironmentAuthorizer
import club.touchtech.s5code.kotlin.transport.EnvironmentHttp
import club.touchtech.s5code.kotlin.transport.PairingClient
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.launchIn
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.onEach
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * Preferences that the client applies (theme, density, grouping).
 *
 * Aliased rather than redeclared: the persisted mapping in
 * `data/ClientStateMapping.kt` owns this shape so it can be tested without a
 * `ViewModel`, and two copies of the same record is how the two drift.
 */
typealias Preferences = RuntimePreferences

/** Home list filter/search state. */
@Immutable
data class HomeUiState(
    val query: String = "",
    val filter: ThreadFilter = ThreadFilter.All,
    val environmentId: EnvironmentId? = null,
    val projectKey: String? = null,
    val snoozedExpanded: Boolean = false,
    val settledExpanded: Boolean = false,
)

/** New-task draft, shared across the whole new-task flow. */
@Immutable
data class NewTaskDraft(
    val environmentId: EnvironmentId = EnvironmentId(""),
    val projectKey: String = "",
    val prompt: String = "",
    val attachments: List<ComposerAttachment> = emptyList(),
    val branch: String = "",
    val workspaceMode: WorkspaceMode = WorkspaceMode.CurrentCheckout,
    /**
     * An existing worktree to run the task in ("New thread on <branch>" from a
     * thread row), matching the RN draft's `worktreePath` prefill.
     */
    val worktreePath: String? = null,
    val settings: ThreadSettings = ThreadSettings(),
    /**
     * Share ids already merged into this draft. The draft persists between
     * process death and the next picker visit, so the receipt travels with it:
     * re-entering must not merge the same share's text a second time.
     */
    val importedShareIds: List<String> = emptyList(),
)

/** Per-thread composer draft: prompt text plus its pending attachments. */
@Immutable
data class ThreadDraft(
    val text: String = "",
    val attachments: List<ComposerAttachment> = emptyList(),
    /**
     * Existing-thread settings are staged with the composer, matching RN. They
     * become authoritative when the next turn is sent, so choosing a model does
     * not race the live thread projection and snap back before the user types.
     */
    val settings: ThreadSettings? = null,
)

/**
 * Single app-scoped store.
 *
 * It owns the three things every screen needs and nothing else: the paired
 * environments (through [workspace]), the S5 Connect account (through [cloud]),
 * and the UI-local drafts and preferences that no server knows about.
 */
class AppStore(application: Application) : AndroidViewModel(application) {

    private val http = EnvironmentHttp(EnvironmentHttp.defaultClient())
    private val client = EnvironmentHttp.defaultClient()
    private val workspaceSnapshots = WorkspaceSnapshotStore(application)

    private val environmentStore = EnvironmentStore(application)

    /** Pairing runs outside the gateway: it creates the environments it manages. */
    val pairing = PairingClient(http, environmentStore)

    /**
     * The device's relay proof key, or null when the Keystore refuses to generate
     * one. Non-fatal: a device without it can still pair directly, which is why
     * every Connect surface reads [cloudEnvironments] being null as "not
     * available" rather than crashing at startup.
     */
    private val dpopKey = runCatching { DpopKey.loadOrCreate() }.getOrNull()

    // Explicitly typed: the relay reads tokens from `cloud`, and `cloud` clears
    // the relay on sign-out, so inference would chase its own tail here.
    private val relay: RelayClient? =
        CloudPublicConfig.fromBuildConfig().relayUrl?.let { relayUrl ->
            dpopKey?.let { key ->
                RelayClient(
                    relayUrl = relayUrl,
                    client = client,
                    key = key,
                    clerkToken = { cloud.readRelayToken() },
                )
            }
        }

    /** S5 Connect account state, backed by Clerk. */
    val cloud: CloudAuth =
        CloudAuth(
            application,
            viewModelScope,
            onSignOut = {
                pushRegistration.signOut()
                relay?.reset()
                // Relay-managed rows belong to the account that linked them, so
                // they leave with it (RN `removeCloudEnvironments`). The DPoP key
                // deliberately survives: it is this install's device identity,
                // and deleting it mid-process would leave the relay client and
                // every live authorizer signing with a key nothing can recreate.
                environmentStore.environments.value
                    .filter { it.relayManaged }
                    .forEach { unpair(EnvironmentId(it.environmentId)) }
            },
        )

    /** Managed environments, or null in a build with no relay configured. */
    val cloudEnvironments =
        relay?.let { CloudEnvironments(viewModelScope, it, http, environmentStore) }

    val workspace: WorkspaceGateway =
        LiveWorkspaceGateway(
            context = application,
            scope = viewModelScope,
            store = environmentStore,
            http = http,
            client = client,
            snapshotStore = workspaceSnapshots,
            authorizerFor = { environment ->
                val relayClient = relay
                val key = dpopKey
                if (environment.relayManaged && relayClient != null && key != null) {
                    RelayEnvironmentAuthorizer(
                        relay = relayClient,
                        http = http,
                        key = key,
                        // The registered device id lets the relay attribute this
                        // session and gate its alerts by this device's link.
                        deviceId = { PushRuntime.deviceId(application) },
                        deviceLabel = PairingClient.deviceLabel(),
                        onEndpointResolved = { id, httpBaseUrl, wsBaseUrl ->
                            environmentStore.updateEndpoint(id, httpBaseUrl, wsBaseUrl)
                        },
                    )
                } else {
                    DirectEnvironmentAuthorizer(environmentStore)
                }
            },
        )

    /** In-app self-updater querying GitHub releases directly. */
    val updates =
        AppUpdateManager(
            context = application,
            client = client,
            scope = viewModelScope,
        )

    private val _preferences = MutableStateFlow(Preferences())
    val preferences: StateFlow<Preferences> = _preferences.asStateFlow()

    private val pushRegistration: PushRegistrationCoordinator by lazy {
        PushRegistrationCoordinator(
            context = application,
            scope = viewModelScope,
            relay = relay,
            account = cloud.state,
            preferences = preferences,
        )
    }

    val pushRuntime = PushRuntime.state

    private val _home = MutableStateFlow(HomeUiState())
    val home: StateFlow<HomeUiState> = _home.asStateFlow()

    private val _draft = MutableStateFlow(NewTaskDraft())
    val draft: StateFlow<NewTaskDraft> = _draft.asStateFlow()

    private val _threadDrafts = MutableStateFlow<Map<String, ThreadDraft>>(emptyMap())
    val threadDrafts: StateFlow<Map<String, ThreadDraft>> = _threadDrafts.asStateFlow()

    private val shareStore = IncomingShareStore(application)

    private val outboxStore = ThreadOutboxStore(application)
    private val outboxMutation = Mutex()
    private val outboxDrain = Mutex()
    private val _outbox = MutableStateFlow<List<QueuedThreadMessage>>(emptyList())
    val outbox: StateFlow<List<QueuedThreadMessage>> = _outbox.asStateFlow()

    /** Draft keys match RN's environment-scoped thread identity. */
    private fun threadDraftKey(environmentId: String, threadId: String): String =
        "$environmentId/$threadId"

    fun threadDraft(environmentId: String, threadId: String): ThreadDraft =
        _threadDrafts.value[threadDraftKey(environmentId, threadId)]
            ?: _threadDrafts.value[threadId] // migrate pre-scope records lazily
            ?: ThreadDraft()

    private fun updateThreadDraft(
        environmentId: String,
        threadId: String,
        transform: (ThreadDraft) -> ThreadDraft,
    ) {
        _threadDrafts.update { drafts ->
            val key = threadDraftKey(environmentId, threadId)
            val current = drafts[key] ?: drafts[threadId] ?: ThreadDraft()
            (drafts - threadId) + (key to transform(current))
        }
    }

    /**
     * Most recent attachment rejection, shown once and cleared. Paste and pick
     * share it so the composer only ever renders a single notice.
     */
    private val _attachmentError = MutableStateFlow<String?>(null)
    val attachmentError: StateFlow<String?> = _attachmentError.asStateFlow()

    private val _actionProgress = MutableStateFlow<ActionProgress?>(null)
    val actionProgress: StateFlow<ActionProgress?> = _actionProgress.asStateFlow()

    private val _globalError = MutableStateFlow<AppErrorNotice?>(null)
    val globalError: StateFlow<AppErrorNotice?> = _globalError.asStateFlow()

    private var nextNoticeId = 0L

    /**
     * True when at least one environment is saved. Derived rather than set, so a
     * pair or an unpair moves the shell without anyone remembering to call a
     * setter.
     */
    val paired: StateFlow<Boolean> =
        environmentStore.environments
            .map { it.isNotEmpty() }
            .stateIn(viewModelScope, SharingStarted.Eagerly, false)

    /**
     * False until the cold-start restore finishes. The bootstrap destination waits
     * on this so a launch cannot flash the onboarding hero at a paired device (or
     * the home list at an unpaired one) before the real answer arrives.
     */
    val sessionRestored: StateFlow<Boolean> = environmentStore.loaded

    private val clientState = ClientStateStore(application)

    private val _recentThreads = MutableStateFlow<List<StoredRecentThread>>(emptyList())

    /** Recently opened threads, in launcher-shortcut order. */
    val recentThreads: StateFlow<List<StoredRecentThread>> = _recentThreads.asStateFlow()

    /**
     * A route an external entry point asked for, waiting to be consumed once the
     * shell is on screen. Held as state rather than navigated immediately because
     * the intent can arrive before the nav host exists.
     */
    private val _pendingLink = MutableStateFlow<DeepLink?>(null)
    val pendingLink: StateFlow<DeepLink?> = _pendingLink.asStateFlow()

    init {
        viewModelScope.launch { environmentStore.load() }
        // A share can be ingested and then orphaned by process death before the
        // user picked a project; republishing the inbox on launch is what lets
        // the pending-share watcher pick it back up.
        viewModelScope.launch { shareStore.refresh() }
        viewModelScope.launch {
            val loaded = clientState.load()
            _preferences.value = loaded.preferences.toRuntime()
            _threadDrafts.value =
                loaded.threadDrafts.mapValues { (_, draft) ->
                    ThreadDraft(
                        text = draft.text,
                        attachments = draft.attachments.map { it.toRuntime() },
                        settings = draft.settings?.toRuntimeThreadSettings(),
                    )
                }
            _draft.value = loaded.newTask.toNewTaskDraft()
            _home.value =
                _home.value.copy(
                    snoozedExpanded = loaded.preferences.snoozedThreadsExpanded,
                    settledExpanded = loaded.preferences.settledThreadsExpanded,
                )
            // Persist only after the restore has landed: saving before it would
            // write the empty defaults over the real file. The availability watch
            // waits for the same reason — it would otherwise correct the defaults
            // and then have the restore put the stale selection back.
            observePersistence()
            observeDraftProviderAvailability()
            restoreAndDrainOutbox()
            pushRegistration.start()
            // Check for app updates in the background on launch
            updates.checkForUpdates()
        }
    }

    /**
     * Follows the selected machine's catalog changes to keep the draft pointing at
     * an agent and model that can actually run on it.
     */
    private fun observeDraftProviderAvailability() {
        combine(
            _draft.map { it.environmentId }.distinctUntilChanged(),
            workspace.providerCatalogs,
            workspace.providerCatalog,
        ) { environmentId, catalogs, globalCatalog ->
            catalogs[environmentId]?.takeIf { it.isNotEmpty() } ?: globalCatalog
        }
            .onEach { catalog ->
                if (catalog.isEmpty()) return@onEach
                updateDraft { draft ->
                    val current =
                        catalog.firstOrNull {
                            it.instance.instanceId == draft.settings.provider.instanceId
                        }
                    if (current != null) {
                        // The instance is fine; the model may still be stale.
                        if (current.models.isEmpty() || draft.settings.model in current.models) {
                            draft
                        } else {
                            draft.copy(
                                settings =
                                    draft.settings.copy(
                                        model = current.models.first(),
                                        // Option ids belong to the model that
                                        // advertised them.
                                        options = emptyList(),
                                    )
                            )
                        }
                    } else {
                        val fallback = catalog.first()
                        draft.copy(
                            settings =
                                draft.settings.copy(
                                    provider = fallback.instance,
                                    model =
                                        fallback.models.firstOrNull() ?: draft.settings.model,
                                    options = emptyList(),
                                )
                        )
                    }
                }
            }
            .launchIn(viewModelScope)
    }

    /**
     * Writes client state whenever it changes, debounced.
     *
     * The debounce is what makes per-keystroke draft persistence affordable: a
     * fast typist produces a write every 400ms rather than one per character, and
     * `conflate` means a burst collapses instead of queueing. 400ms is short
     * enough that a process death loses at most a word.
     */
    private fun observePersistence() {
        combine(_preferences, _threadDrafts, _draft, _recentThreads) { prefs, drafts, newTask, recents
                ->
                Snapshot(prefs, drafts, newTask, recents)
            }
            .debounce(PERSIST_DEBOUNCE_MILLIS)
            .onEach { snapshot ->
                clientState.save(
                    preferencesValue = snapshot.preferences.toStored(),
                    threadDrafts =
                        snapshot.threadDrafts.mapValues { (_, draft) ->
                            StoredDraft(
                                text = draft.text,
                                attachments = draft.attachments.map { it.toStored() },
                                settings = draft.settings?.toStoredThreadSettings(),
                            )
                        },
                    newTask = snapshot.newTask.toStored(),
                    recentThreads = snapshot.recentThreads,
                )
            }
            .launchIn(viewModelScope)
    }

    private data class Snapshot(
        val preferences: Preferences,
        val threadDrafts: Map<String, ThreadDraft>,
        val newTask: NewTaskDraft,
        val recentThreads: List<StoredRecentThread>,
    )

    /**
     * Records an opened thread for launcher shortcuts. The title is only
     * overwritten by a non-blank one: the shell that carries titles loads after
     * navigation, so the first record of a thread often has none.
     */
    fun recordRecentThread(environmentId: String, threadId: String, title: String) {
        _recentThreads.update { current ->
            val existing =
                current.firstOrNull { it.environmentId == environmentId && it.threadId == threadId }
            val resolved = title.trim().ifBlank { existing?.title.orEmpty() }
            if (current.firstOrNull() === existing && existing?.title == resolved) return@update current
            (listOf(StoredRecentThread(environmentId, threadId, resolved)) +
                    current.filterNot { it === existing })
                .take(ClientStateStore.MAX_RECENT_THREADS)
        }
    }

    /**
     * Queues an external navigation request. A share first materializes into the
     * incoming-share inbox — the `content:` grants lapse when the sender's
     * activity finishes, so the draft must never hold those URIs — and only then
     * navigates to the project picker, matching the RN client's pending-share
     * presentation.
     */
    fun openDeepLink(link: DeepLink) {
        if (link is DeepLink.Share) {
            viewModelScope.launch {
                val ingested = ingestIncomingShare(link)
                // Only navigate once the payload is durable: arriving at the
                // picker with nothing ingested would strand the user in the
                // new-task flow for a share that failed to copy.
                if (ingested != null) _pendingLink.value = link
            }
            return
        }
        _pendingLink.value = link
    }

    /* ── Incoming shares ────────────────────────────────────────────── */

    /**
     * Sharesheet payloads waiting for a project, newest first. Consumed at
     * import, so a draft only lingers while it is genuinely unclaimed.
     */
    val incomingShareDrafts: StateFlow<List<IncomingShareDraft>> = shareStore.drafts

    /**
     * The share the new-task flow should present next, or null. A share that is
     * already merged into the draft stays out of this slot even if its inbox
     * record survived an interrupted import.
     */
    val pendingShare: StateFlow<IncomingShareDraft?> =
        combine(incomingShareDrafts, _draft) { drafts, draft ->
                drafts.firstOrNull { it.id !in draft.importedShareIds }
            }
            .stateIn(viewModelScope, SharingStarted.Eagerly, null)

    /**
     * Copies a share's payloads into the inbox and writes the draft record.
     * Returns the share id, or null when nothing survived materialization —
     * mirroring the RN inbox, which drops an all-warnings share rather than
     * re-presenting an unactionable picker on every foreground.
     */
    private suspend fun ingestIncomingShare(link: DeepLink.Share): String? {
        val shareId = incomingShareIdFor(link.text, link.mimeType, link.uris)
        if (shareStore.draftsNow().any { it.id == shareId }) return shareId
        val draft =
            buildIncomingShare(
                context = getApplication(),
                text = link.text,
                intentMimeType = link.mimeType,
                uris = link.uris,
                nowIso = java.time.Instant.now().toString(),
            ) ?: run {
                shareStore.consume(shareId)
                return null
            }
        if (!hasIncomingShareContent(draft)) {
            shareStore.consume(shareId)
            showError(draft.warnings.firstOrNull() ?: "The shared content is not supported.")
            return null
        }
        shareStore.write(draft)
        return shareId
    }

    /**
     * Merges [shareId] into the new-task draft for the chosen destination.
     *
     * Order matters: reserve first so a failure or process death mid-import
     * leaves the share pinned to this project rather than free-floating, then
     * merge, then consume — the durable draft is the commit point, and the
     * attachments it records are composer-cache copies, not inbox files, so the
     * share directory can be deleted.
     */
    suspend fun importIncomingShare(shareId: String, destination: IncomingShareDestination) {
        if (shareId in _draft.value.importedShareIds) return
        val share = shareStore.draftsNow().firstOrNull { it.id == shareId }
        if (share == null) {
            showError("The shared content is no longer in the inbox.")
            return
        }
        // A stale reservation for another project is released before this one
        // takes; the share belongs to the project the user just picked.
        share.destination?.takeIf { it != destination }?.let {
            shareStore.releaseReservation(shareId, it)
        }
        val reserved = shareStore.reserve(shareId, destination)
        if (reserved == null) {
            showError("The shared content is reserved for another draft.")
            return
        }

        // The destination server's file support decides which attachments can
        // ride along; the rest become warnings, matching RN's
        // `selectIncomingShareAttachmentsForServer`.
        val environment =
            workspace.environments.value.firstOrNull {
                it.id.value == destination.environmentId
            }
        val maxFileBytes =
            environment
                ?.capabilities
                ?.takeIf { it.attachmentUploads }
                ?.fileAttachments
                ?.maxUploadBytes
        val (kept, selectionWarnings) =
            selectIncomingShareAttachments(share.attachments, maxFileBytes)
        val (images, droppedFiles) = kept.partition {
            it.type == IncomingShareAttachmentType.Image
        }
        val fileWarnings =
            droppedFiles.map {
                "'${it.name}' was skipped because this client does not support file attachments yet."
            }

        // Images pass through the same intake as a picker pick, so the draft
        // holds composer-cache copies that outlive the inbox entry.
        val candidates =
            images.map {
                ComposerImageCandidate(
                    uri = it.uri,
                    mimeType = it.mimeType,
                    name = it.name,
                    sizeBytes = it.sizeBytes,
                )
            }
        val materialized = materializeComposerImages(getApplication(), candidates)
        var acceptError: String? = null
        _draft.update { draft ->
            if (shareId in draft.importedShareIds) return@update draft
            val result = acceptComposerImages(draft.attachments, materialized)
            acceptError = result.error
            val separator = if (draft.prompt.isBlank() || share.text.isBlank()) "" else "\n\n"
            draft.copy(
                prompt = draft.prompt + separator + share.text,
                attachments = draft.attachments + result.attachments,
                importedShareIds = draft.importedShareIds + shareId,
            )
        }

        shareStore.consume(shareId)
        val warnings = share.warnings + selectionWarnings + fileWarnings + listOfNotNull(acceptError)
        if (warnings.isNotEmpty()) {
            _attachmentError.value = warnings.joinToString("\n")
        }
    }

    fun consumePendingLink() {
        _pendingLink.value = null
    }

    private fun StoredNewTaskDraft.toNewTaskDraft(): NewTaskDraft =
        NewTaskDraft(
            environmentId = EnvironmentId(environmentId),
            projectKey = projectKey,
            prompt = prompt,
            attachments = attachments.map { it.toRuntime() },
            branch = branch,
            workspaceMode = storedWorkspaceMode(workspaceMode),
            settings =
                ThreadSettings(
                    provider = storedProvider(provider, providerDriver),
                    model = model.ifBlank { ThreadSettings().model },
                    runtimeMode = storedRuntimeMode(runtimeMode),
                    approvalPolicy = storedApprovalPolicy(approvalPolicy),
                    options = storedProviderOptions(options),
                ),
            importedShareIds = importedShareIds,
        )

    private fun NewTaskDraft.toStored(): StoredNewTaskDraft =
        StoredNewTaskDraft(
            environmentId = environmentId.value,
            projectKey = projectKey,
            prompt = prompt,
            attachments = attachments.map { it.toStored() },
            branch = branch,
            workspaceMode = workspaceMode.name,
            provider = settings.provider.instanceId,
            providerDriver = settings.provider.driver,
            model = settings.model,
            runtimeMode = settings.runtimeMode.name,
            approvalPolicy = settings.approvalPolicy.name,
            options = settings.options.associate { it.toStored() },
            importedShareIds = importedShareIds,
        )

    /**
     * Home UI state, mirroring the two shelf toggles into preferences so they
     * survive process death. The rest of [HomeUiState] (query, filter, scope) is
     * deliberately not persisted: reopening the app inside someone's stale search
     * is worse than reopening on the full list.
     */
    fun updateHome(transform: (HomeUiState) -> HomeUiState) {
        _home.update(transform)
        _preferences.update {
            it.copy(
                snoozedThreadsExpanded = _home.value.snoozedExpanded,
                settledThreadsExpanded = _home.value.settledExpanded,
            )
        }
    }

    fun updatePreferences(transform: (Preferences) -> Preferences) {
        _preferences.update(transform)
        pushRegistration.refresh()
    }

    fun refreshPushRegistration() = pushRegistration.refresh()

    fun armLiveUpdate(threadTitle: String, projectTitle: String) {
        // Agent-awareness chrome is best-effort. Accepting a durable turn/task is
        // the primary action; a platform notification failure must never unwind
        // that acceptance and strand navigation on the loading thread screen.
        runCatching { pushRegistration.arm(threadTitle, projectTitle) }
            .onFailure {
                PushRuntime.publish(
                    PushRegistrationStatus.Failed,
                    it.message ?: "The Live Update could not be started.",
                )
            }
    }

    fun updateDraft(transform: (NewTaskDraft) -> NewTaskDraft) = _draft.update(transform)

    fun setThreadDraft(environmentId: String, threadId: String, text: String) =
        updateThreadDraft(environmentId, threadId) { it.copy(text = text) }

    /** Adds a structured review comment to the durable thread composer draft. */
    fun appendThreadDraft(
        environmentId: String,
        threadId: String,
        text: String,
        attachments: List<ComposerAttachment>,
    ) = updateThreadDraft(environmentId, threadId) { draft ->
        val separator = if (draft.text.isBlank()) "" else "\n\n"
        draft.copy(
            text = draft.text + separator + text,
            attachments = draft.attachments + attachments,
        )
    }

    fun clearThreadDraftContent(environmentId: String, threadId: String) =
        _threadDrafts.update { drafts ->
            val key = threadDraftKey(environmentId, threadId)
            val draft = drafts[key] ?: drafts[threadId] ?: return@update drafts
            val retained = draft.copy(text = "", attachments = emptyList())
            val migrated = drafts - threadId
            if (retained.settings == null) migrated - key else migrated + (key to retained)
        }

    fun setThreadDraftSettings(
        environmentId: String,
        threadId: String,
        settings: ThreadSettings,
    ) = updateThreadDraft(environmentId, threadId) { it.copy(settings = settings) }

    fun clearThreadDraft(environmentId: String, threadId: String) =
        _threadDrafts.update { it - threadDraftKey(environmentId, threadId) - threadId }

    /** Queues first, then clears the draft only after the durable record exists. */
    suspend fun enqueueThreadMessage(
        environmentId: String,
        threadId: String,
        text: String,
        attachments: List<ComposerAttachment>,
        settings: ThreadSettings,
    ) {
        val message =
            newQueuedThreadMessage(
                environmentId = EnvironmentId(environmentId),
                text = text.trim(),
                attachments = attachments,
                settings = settings,
                threadId = ThreadId(threadId),
            )
        val durable = outboxStore.enqueue(message)
        outboxMutation.withLock {
            _outbox.update { current ->
                (current.filterNot { it.delivery.messageId == durable.delivery.messageId } + durable)
                    .sortedBy { it.delivery.createdAt }
            }
        }
        clearThreadDraftContent(environmentId, threadId)
        val thread = workspace.threads.value.firstOrNull {
            it.environmentId.value == environmentId && it.id.value == threadId
        }
        val project = thread?.let { summary -> workspace.projects.value.firstOrNull { it.id == summary.projectId } }
        armLiveUpdate(thread?.title.orEmpty(), project?.title.orEmpty())
    }

    /** Creates a durable pending task and lets the same drain create its thread. */
    suspend fun enqueueNewTask(draft: NewTaskDraft): ThreadId {
        // RN's resolvePendingTaskInteractionMode: a persisted Plan choice does
        // not bypass a disabled planMode preference.
        val settings =
            if (preferences.value.planModeEnabled) {
                draft.settings
            } else {
                draft.settings.copy(runtimeMode = RuntimeMode.Default)
            }
        val message =
            newQueuedThreadMessage(
                environmentId = draft.environmentId,
                text = draft.prompt.trim(),
                attachments = draft.attachments,
                settings = settings,
                creation =
                    club.touchtech.s5code.kotlin.data.StoredQueuedThreadCreation(
                        projectKey = draft.projectKey,
                        branch = draft.branch,
                        newWorktree = draft.workspaceMode == WorkspaceMode.NewWorktree,
                        worktreePath = draft.worktreePath,
                    ),
            )
        val durable = outboxStore.enqueue(message)
        if (durable.creation != null) workspace.setPendingThreadCreations(setOf(durable.key))
        outboxMutation.withLock {
            _outbox.update { current -> (current + durable).sortedBy { it.delivery.createdAt } }
        }
        updateDraft { it.copy(prompt = "", attachments = emptyList(), importedShareIds = emptyList()) }
        val project = workspace.projects.value.firstOrNull {
            it.environmentId == draft.environmentId && it.id.value == draft.projectKey
        }
        armLiveUpdate(
            draft.prompt.lineSequence().firstOrNull().orEmpty().take(80),
            project?.title.orEmpty(),
        )
        return durable.threadId
    }

    fun queuedMessageCount(environmentId: String, threadId: String): Int =
        _outbox.value.count {
            it.environmentId.value == environmentId && it.threadId.value == threadId
        }

    private fun restoreAndDrainOutbox() {
        viewModelScope.launch {
            val restored = outboxStore.load()
            workspace.setPendingThreadCreations(
                restored.asSequence()
                    .filter { it.creation != null }
                    .mapTo(mutableSetOf(), QueuedThreadMessage::key)
            )
            _outbox.value = restored
            combine(workspace.environments, workspace.threads, _outbox) { environments, threads, queued ->
                    Triple(environments, threads, queued)
                }
                .collect { (environments, threads, queued) ->
                    val connected = environments.filter { it.state == club.touchtech.s5code.kotlin.model.ConnectionState.Connected }.map { it.id }.toSet()
                    val next =
                        queued.sortedBy { it.delivery.createdAt }.firstOrNull {
                            val creationReady =
                                it.creation == null || workspace.projects.value.any { project ->
                                    project.environmentId == it.environmentId &&
                                        project.id.value == it.creation.projectKey
                                }
                            creationReady && it.environmentId in connected
                        }
                    if (next != null) drainQueuedMessage(next)
                }
        }
    }

    private suspend fun drainQueuedMessage(message: QueuedThreadMessage) {
        outboxDrain.withLock {
            if (_outbox.value.none { it.delivery.messageId == message.delivery.messageId }) return
            // The server may have accepted creation just before Android killed the
            // process, leaving the durable record behind. A restored shell row for
            // this generated id is the acknowledgement in that recovery window;
            // replaying the bootstrap command can only produce "already exists".
            if (
                queuedCreationAlreadyExists(
                    message,
                    workspace.threads.value.mapTo(mutableSetOf()) {
                        "${it.environmentId.value}/${it.id.value}"
                    },
                )
            ) {
                completeQueuedMessage(message)
                return
            }
            var attempt = 0
            while (true) {
                try {
                    if (message.creation != null) {
                        workspace.createThread(
                            environmentId = message.environmentId,
                            projectKey = message.creation.projectKey,
                            prompt = message.text,
                            settings = message.settings,
                            branch = message.creation.branch,
                            newWorktree = message.creation.newWorktree,
                            attachments = message.attachments,
                            worktreePath = message.creation.worktreePath,
                            threadId = message.threadId,
                            delivery = message.delivery,
                        )
                    } else {
                        workspace.sendMessage(
                            environmentId = message.environmentId,
                            id = message.threadId,
                            text = message.text,
                            attachments = message.attachments,
                            settings = message.settings,
                            delivery = message.delivery,
                        )
                    }
                    completeQueuedMessage(message)
                    return
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (error: Exception) {
                    // A thread id in a pending creation is random and belongs only
                    // to this outbox record. The server's duplicate invariant means
                    // the original command crossed the acknowledgement gap before
                    // the process/transport died, so this conflict is success—not a
                    // permanent send failure to show the user.
                    if (duplicateCreationAcknowledgesDelivery(message, error)) {
                        completeQueuedMessage(message)
                        return
                    }
                    if (!isTransientOutboxFailure(error)) {
                        completeQueuedMessage(message, creationAccepted = false)
                        showError(error.message ?: "A queued message could not be sent.")
                        return
                    }
                    attempt += 1
                    delay(threadOutboxRetryDelayMillis(attempt))
                    val environment = workspace.environments.value.firstOrNull { it.id == message.environmentId }
                    if (environment?.state != club.touchtech.s5code.kotlin.model.ConnectionState.Connected) return
                }
            }
        }
    }

    private suspend fun completeQueuedMessage(
        message: QueuedThreadMessage,
        creationAccepted: Boolean = true,
    ) {
        // Disk first preserves at-least-once delivery if the process dies during
        // cleanup. A surviving in-memory copy is harmless and disappears on the
        // next update; a surviving disk copy would otherwise replay after restart.
        outboxStore.remove(message)
        // An accepted creation remains "expected" until the authoritative shell
        // projects it. Clearing here would stop a detail retry in the dispatch→shell
        // gap and strand the just-opened screen. Only a rejected creation is no
        // longer expected to appear.
        if (message.creation != null && !creationAccepted) {
            workspace.discardPendingThreadCreation(message.key)
        }
        outboxMutation.withLock {
            _outbox.update { queued ->
                queued.filterNot { it.delivery.messageId == message.delivery.messageId }
            }
        }
    }

    private fun isTransientOutboxFailure(error: Throwable): Boolean {
        if (error is club.touchtech.s5code.kotlin.transport.RpcTransportClosed) return true
        val message = error.message.orEmpty().lowercase()
        return listOf("socket", "connection", "network", "timeout", "closed", "offline")
            .any(message::contains)
    }

    /**
     * Validates images against the send-turn limits before they enter the
     * thread draft, and records the reason for anything refused. Shared by the
     * paste gesture, the explicit paste action, drops, and the photo picker.
     *
     * Validation runs inside the atomic update so two intakes landing together
     * (a paste while a pick is still copying) cannot both spend the last slot.
     */
    fun addThreadDraftImages(
        environmentId: String,
        threadId: String,
        candidates: List<ComposerImageCandidate>,
    ) {
        var error: String? = null
        updateThreadDraft(environmentId, threadId) { draft ->
            val result = acceptComposerImages(draft.attachments, candidates)
            error = result.error
            if (result.attachments.isEmpty()) draft
            else draft.copy(attachments = draft.attachments + result.attachments)
        }
        _attachmentError.value = error
    }

    fun removeThreadDraftImage(
        environmentId: String,
        threadId: String,
        attachmentId: String,
    ) = updateThreadDraft(environmentId, threadId) { draft ->
        draft.copy(attachments = draft.attachments.filterNot { it.id == attachmentId })
    }

    /** Same validation for the new-task draft, which has its own attachments. */
    fun addNewTaskDraftImages(candidates: List<ComposerImageCandidate>) {
        var error: String? = null
        _draft.update { draft ->
            val result = acceptComposerImages(draft.attachments, candidates)
            error = result.error
            if (result.attachments.isEmpty()) draft
            else draft.copy(attachments = draft.attachments + result.attachments)
        }
        _attachmentError.value = error
    }

    fun removeNewTaskDraftImage(attachmentId: String) =
        updateDraft { draft ->
            draft.copy(attachments = draft.attachments.filterNot { it.id == attachmentId })
        }

    fun clearAttachmentError() {
        _attachmentError.value = null
    }

    /* ── Question attachments ────────────────────────────────────────── */

    /**
     * Files staged on pending user-input questions, keyed
     * `envId/threadId/requestId/questionId` — the same scope RN's
     * `questionAttachmentDraftKey` gives its composer drafts. Staging uploads
     * eagerly: by submit time the bytes are already on the server, so answering
     * is a metadata write rather than a transfer.
     */
    private val _questionAttachments =
        MutableStateFlow<Map<String, List<QuestionAttachment>>>(emptyMap())
    val questionAttachments: StateFlow<Map<String, List<QuestionAttachment>>> =
        _questionAttachments.asStateFlow()

    private fun questionAttachmentKey(
        environmentId: String,
        threadId: String,
        requestId: String,
        questionId: String,
    ): String = "$environmentId/$threadId/$requestId/$questionId"

    /**
     * The attachments staged on every question of one request, re-keyed by
     * question id for the card that renders them.
     */
    fun questionAttachmentsFor(
        environmentId: String,
        threadId: String,
        requestId: String,
    ): Map<String, List<QuestionAttachment>> {
        val prefix = questionAttachmentKey(environmentId, threadId, requestId, "")
        return _questionAttachments.value.entries
            .filter { it.key.startsWith(prefix) }
            .associate { it.key.removePrefix(prefix) to it.value }
    }

    /**
     * Appends materialized files to one question's staging and starts their
     * uploads. The send-turn cap counts attachments on the whole request, not
     * per question — matching `appendComposerDraftAttachments`'s `maxAttachments`
     * computation in RN, which subtracts the siblings' count.
     */
    fun stageQuestionAttachments(
        environmentId: String,
        threadId: String,
        requestId: String,
        questionId: String,
        files: List<StagedQuestionFile>,
    ) {
        if (files.isEmpty()) return
        val key = questionAttachmentKey(environmentId, threadId, requestId, questionId)
        val requestPrefix = questionAttachmentKey(environmentId, threadId, requestId, "")
        var staged: List<QuestionAttachment> = emptyList()
        var overflow = false
        _questionAttachments.update { current ->
            val requestCount =
                current.entries
                    .filter { it.key.startsWith(requestPrefix) }
                    .sumOf { it.value.size }
            val accepted = files.take((ComposerAttachmentLimits.MAX_ATTACHMENTS - requestCount).coerceAtLeast(0))
            overflow = accepted.size < files.size
            staged =
                accepted.map { file ->
                    QuestionAttachment(
                        localId = file.localPath,
                        name = file.name,
                        mimeType = file.mimeType,
                        sizeBytes = file.sizeBytes,
                        kind = file.kind,
                        localPath = file.localPath,
                        status = QuestionAttachmentStatus.Uploading,
                    )
                }
            if (staged.isEmpty()) current
            else current + (key to (current[key].orEmpty() + staged))
        }
        if (overflow) {
            _attachmentError.value =
                "You can attach up to ${ComposerAttachmentLimits.MAX_ATTACHMENTS} files per message."
        }
        // Files refused by the cap never enter the draft, so their cache copies
        // are freed immediately — the same cleanup RN runs on rejected picks.
        files.drop(staged.size).forEach { file -> java.io.File(file.localPath).delete() }
        staged.forEach { attachment ->
            viewModelScope.launch { uploadQuestionAttachment(environmentId, key, attachment) }
        }
    }

    /**
     * Re-runs the upload for a failed staged attachment. The local file is still
     * in cache, so retry is a new `createUploadUrl` + POST, not a re-pick.
     */
    fun retryQuestionAttachment(
        environmentId: String,
        threadId: String,
        requestId: String,
        questionId: String,
        localId: String,
    ) {
        val key = questionAttachmentKey(environmentId, threadId, requestId, questionId)
        val attachment =
            _questionAttachments.value[key]?.firstOrNull { it.localId == localId } ?: return
        if (attachment.status != QuestionAttachmentStatus.Failed) return
        updateQuestionAttachment(key, localId) {
            it.copy(status = QuestionAttachmentStatus.Uploading, error = null)
        }
        viewModelScope.launch { uploadQuestionAttachment(environmentId, key, attachment) }
    }

    private suspend fun uploadQuestionAttachment(
        environmentId: String,
        key: String,
        attachment: QuestionAttachment,
    ) {
        try {
            val uploadId =
                workspace.uploadPendingAttachment(
                    environmentId = EnvironmentId(environmentId),
                    type = attachment.kind.wireValue,
                    name = attachment.name,
                    mimeType = attachment.mimeType,
                    file = java.io.File(attachment.localPath),
                )
            // The entry may have been removed while the POST was in flight; its
            // upload then belongs to nobody and is deleted rather than leaked.
            val stillStaged =
                _questionAttachments.value[key]?.any { it.localId == attachment.localId } == true
            if (stillStaged) {
                updateQuestionAttachment(key, attachment.localId) {
                    it.copy(status = QuestionAttachmentStatus.Ready, uploadedAttachmentId = uploadId)
                }
            } else {
                runCatching { workspace.deletePendingAttachment(EnvironmentId(environmentId), uploadId) }
            }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (error: Exception) {
            updateQuestionAttachment(key, attachment.localId) {
                it.copy(
                    status = QuestionAttachmentStatus.Failed,
                    error = error.message ?: "The upload failed.",
                )
            }
        }
    }

    private fun updateQuestionAttachment(
        key: String,
        localId: String,
        transform: (QuestionAttachment) -> QuestionAttachment,
    ) {
        _questionAttachments.update { current ->
            val list = current[key] ?: return@update current
            current + (key to list.map { if (it.localId == localId) transform(it) else it })
        }
    }

    /**
     * Removes one staged attachment: its local copy, and its pending upload on
     * the server when one was minted. The delete is best-effort — the server
     * expires pending uploads on its own, so a failed delete must not keep the
     * chip.
     */
    fun removeQuestionAttachment(
        environmentId: String,
        threadId: String,
        requestId: String,
        questionId: String,
        localId: String,
    ) {
        val key = questionAttachmentKey(environmentId, threadId, requestId, questionId)
        var removed: QuestionAttachment? = null
        _questionAttachments.update { current ->
            val list = current[key] ?: return@update current
            removed = list.firstOrNull { it.localId == localId }
            val next = list.filterNot { it.localId == localId }
            if (next.isEmpty()) current - key else current + (key to next)
        }
        releaseStagedQuestionAttachment(EnvironmentId(environmentId), removed)
    }

    /**
     * Drops every staged attachment on one request. Called when the request is
     * answered or dismissed — the attachments were either consumed by the
     * submission or belong to a draft that no longer exists.
     */
    fun releaseQuestionAttachments(
        environmentId: String,
        threadId: String,
        requestId: String,
    ) {
        releaseQuestionAttachmentsMatching(
            EnvironmentId(environmentId),
            questionAttachmentKey(environmentId, threadId, requestId, ""),
        )
    }

    /**
     * Drops staged attachments on the thread that no live request still owns —
     * the sweep RN runs off `questionAttachmentDraftPrefix` when the pending
     * set changes, which is what frees files staged on a request resolved from
     * another client.
     */
    fun releaseStaleQuestionAttachments(
        environmentId: String,
        threadId: String,
        retainedRequestIds: Set<String>,
    ) {
        val threadPrefix = "$environmentId/$threadId/"
        val retained =
            retainedRequestIds.map { questionAttachmentKey(environmentId, threadId, it, "") }
        val released = mutableListOf<QuestionAttachment>()
        _questionAttachments.update { current ->
            val doomed =
                current.filterKeys { key ->
                    key.startsWith(threadPrefix) && retained.none { key.startsWith(it) }
                }
            doomed.values.forEach { released += it }
            current - doomed.keys
        }
        val env = EnvironmentId(environmentId)
        released.forEach { releaseStagedQuestionAttachment(env, it) }
    }

    private fun releaseQuestionAttachmentsMatching(environmentId: EnvironmentId, prefix: String) {
        val released = mutableListOf<QuestionAttachment>()
        _questionAttachments.update { current ->
            val doomed = current.filterKeys { it.startsWith(prefix) }
            doomed.values.forEach { released += it }
            current - doomed.keys
        }
        released.forEach { releaseStagedQuestionAttachment(environmentId, it) }
    }

    /** Frees the cache copy and the pending upload a staged attachment holds. */
    private fun releaseStagedQuestionAttachment(
        environmentId: EnvironmentId,
        attachment: QuestionAttachment?,
    ) {
        if (attachment == null) return
        viewModelScope.launch(Dispatchers.IO) { java.io.File(attachment.localPath).delete() }
        val uploadId = attachment.uploadedAttachmentId ?: return
        viewModelScope.launch {
            runCatching { workspace.deletePendingAttachment(environmentId, uploadId) }
        }
    }

    fun beginAction(label: String, description: String? = null): Long {
        val id = ++nextNoticeId
        _actionProgress.value =
            ActionProgress(
                id = id,
                phase = ActionProgressPhase.Running,
                label = label,
                description = description,
            )
        return id
    }

    fun updateAction(id: Long, label: String, description: String? = null) {
        val current = _actionProgress.value ?: return
        if (current.id != id || current.phase != ActionProgressPhase.Running) return
        _actionProgress.value = current.copy(label = label, description = description)
    }

    fun finishAction(
        id: Long,
        label: String,
        description: String? = null,
        linkUrl: String? = null,
    ) {
        if (_actionProgress.value?.id != id) return
        _actionProgress.value =
            ActionProgress(id, ActionProgressPhase.Success, label, description, linkUrl)
    }

    fun failAction(id: Long, label: String, description: String) {
        if (_actionProgress.value?.id != id) return
        _actionProgress.value =
            ActionProgress(id, ActionProgressPhase.Error, label, description)
        showError(description)
    }

    fun dismissActionProgress() {
        _actionProgress.value = null
    }

    fun showError(message: String) {
        _globalError.value = AppErrorNotice(++nextNoticeId, message)
    }

    fun dismissGlobalError() {
        _globalError.value = null
    }

    /** Refreshes all transports when Android returns the existing process to foreground. */
    fun refreshConnections(backgroundedMillis: Long = Long.MAX_VALUE) {
        workspace.refreshConnections(backgroundedMillis)
        // Registration is not a pure no-op: the relay replays the current card
        // aggregate to this device on every accepted registration, repairing
        // pushes that could not be delivered while the app was away. Deduped
        // to one replay per minute inside the coordinator.
        pushRegistration.onForeground()
    }

    /** Retries one environment's connection, for the connections screen. */
    fun retryEnvironment(environmentId: EnvironmentId) {
        // A switched-off environment has no session to retry; the switch is the
        // retry affordance.
        val saved =
            environmentStore.environments.value.firstOrNull {
                it.environmentId == environmentId.value
            }
        if (saved != null && !saved.enabled) return
        (workspace as? LiveWorkspaceGateway)?.retry(environmentId)
    }

    /**
     * Switches an environment on or off. The row stays paired either way — off
     * keeps the credential but tears the session down; on reconnects in place.
     * The gateway reconciles sessions off the store's flow, so this write is the
     * whole mutation.
     */
    fun setEnvironmentEnabled(environmentId: EnvironmentId, enabled: Boolean) {
        viewModelScope.launch { environmentStore.setEnabled(environmentId.value, enabled) }
    }

    /**
     * Edits a direct environment's label and URL — RN's `updateBearerConnection`
     * in `ConnectionEnvironmentRow`. Only direct rows are editable: a cloud
     * environment's endpoint is owned by the relay and rewriting it here would
     * be overwritten by the next credential mint anyway.
     *
     * A URL change means the live socket is pointed at the old host, so the
     * session restarts immediately rather than waiting for the next drop.
     */
    fun updateEnvironment(
        environmentId: EnvironmentId,
        label: String,
        httpBaseUrl: String,
    ) {
        viewModelScope.launch {
            try {
                val saved =
                    environmentStore.environments.value.firstOrNull {
                        it.environmentId == environmentId.value
                    } ?: return@launch
                if (saved.relayManaged) return@launch
                environmentStore.updateDirectEndpoint(
                    environmentId.value,
                    label.ifBlank { saved.label },
                    httpBaseUrl.ifBlank { saved.httpBaseUrl },
                )
                (workspace as? LiveWorkspaceGateway)?.retry(environmentId)
            } catch (error: Exception) {
                showError(error.message ?: "The environment could not be updated.")
            }
        }
    }

    /**
     * Stars or unstars a model in the picker. Favorites are a client-local
     * preference (the other clients keep theirs in client settings too), so
     * this only touches [RuntimePreferences] and persistence does the rest.
     */
    fun toggleModelFavorite(instanceId: String, model: String) {
        _preferences.update { preferences ->
            val existing =
                preferences.modelFavorites.indexOfFirst {
                    it.instanceId == instanceId && it.model == model
                }
            preferences.copy(
                modelFavorites =
                    if (existing >= 0) {
                        preferences.modelFavorites.filterIndexed { index, _ -> index != existing }
                    } else {
                        preferences.modelFavorites + ModelFavorite(instanceId, model)
                    }
            )
        }
    }

    /** In-flight model-catalog refresh per environment, for the picker's spinner. */
    private val _catalogRefreshing = MutableStateFlow<Set<String>>(emptySet())
    val catalogRefreshing: StateFlow<Set<String>> = _catalogRefreshing.asStateFlow()

    /**
     * Re-asks one environment's server for its provider catalog, including a
     * model rediscovery pass (`refreshModels`). A slow discovery should not read
     * as a hung sheet, so the spinner tracks the call and a failure lands on the
     * global banner rather than inside the picker.
     */
    fun refreshProviderCatalog(environmentId: EnvironmentId?) {
        if (environmentId == null) return
        if (environmentId.value in _catalogRefreshing.value) return
        _catalogRefreshing.update { it + environmentId.value }
        viewModelScope.launch {
            try {
                // The socket layer waits for a live connection, so an offline
                // environment would spin forever without a ceiling.
                val refreshed =
                    withTimeoutOrNull(PROVIDER_REFRESH_TIMEOUT_MILLIS) {
                        workspace.refreshProviders(environmentId)
                    } != null
                if (!refreshed) showError("The model list refresh timed out.")
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (error: Exception) {
                showError(error.message ?: "Could not refresh the model list.")
            } finally {
                _catalogRefreshing.update { it - environmentId.value }
            }
        }
    }

    suspend fun unpair(environmentId: EnvironmentId) {
        outboxDrain.withLock {
            outboxMutation.withLock {
                outboxStore.clear(environmentId)
                _outbox.value
                    .asSequence()
                    .filter { it.environmentId == environmentId && it.creation != null }
                    .forEach { workspace.discardPendingThreadCreation(it.key) }
                _outbox.update { messages -> messages.filterNot { it.environmentId == environmentId } }
            }
        }
        workspaceSnapshots.clear(environmentId)
        environmentStore.remove(environmentId.value)
    }

    suspend fun renameEnvironment(environmentId: EnvironmentId, label: String) {
        environmentStore.rename(environmentId.value, label)
    }

    fun setProvider(provider: ProviderInstance) =
        updateDraft { draft ->
            val models = modelsFor(provider, draft.environmentId)
            draft.copy(
                settings =
                    draft.settings.copy(
                        provider = provider,
                        model = models.firstOrNull() ?: draft.settings.model,
                        options = emptyList(),
                    )
            )
        }

    fun setModel(model: String) =
        updateDraft {
            it.copy(
                settings =
                    it.settings.copy(
                        model = model,
                        options = if (model == it.settings.model) it.settings.options else emptyList(),
                    )
            )
        }

    fun setRuntimeMode(mode: RuntimeMode) =
        updateDraft { it.copy(settings = it.settings.copy(runtimeMode = mode)) }

    fun setApprovalPolicy(policy: ApprovalPolicy) =
        updateDraft { it.copy(settings = it.settings.copy(approvalPolicy = policy)) }

    /**
     * Models the given instance offers. Empty while nothing is connected, which the
     * settings sheet renders as "the current selection only" rather than as a
     * provider with no models.
     */
    fun modelsFor(provider: ProviderInstance, environmentId: EnvironmentId? = null): List<String> {
        val catalog = if (environmentId != null && environmentId.value.isNotEmpty()) {
            workspace.providerCatalogs.value[environmentId]
                ?: emptyList()
        } else {
            workspace.providerCatalog.value
        }
        return catalog
            .firstOrNull { it.instance.instanceId == provider.instanceId }
            ?.models
            ?: emptyList()
    }

    private companion object {
        const val PERSIST_DEBOUNCE_MILLIS = 400L

        /** Discovery shells out to each provider CLI, so the ceiling is generous. */
        const val PROVIDER_REFRESH_TIMEOUT_MILLIS = 30_000L
    }
}
