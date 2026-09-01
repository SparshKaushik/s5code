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
import club.touchtech.s5code.kotlin.model.ComposerImageCandidate
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.ProjectGrouping
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.RuntimeMode
import club.touchtech.s5code.kotlin.model.ThreadFilter
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadSettings
import club.touchtech.s5code.kotlin.model.ThreadSort
import club.touchtech.s5code.kotlin.model.WorkspaceMode
import club.touchtech.s5code.kotlin.platform.notifications.AndroidLiveUpdateNotifications
import club.touchtech.s5code.kotlin.platform.notifications.PushRegistrationCoordinator
import club.touchtech.s5code.kotlin.platform.notifications.PushRegistrationStatus
import club.touchtech.s5code.kotlin.platform.notifications.PushRuntime
import club.touchtech.s5code.kotlin.transport.DirectEnvironmentAuthorizer
import club.touchtech.s5code.kotlin.transport.EnvironmentHttp
import club.touchtech.s5code.kotlin.transport.PairingClient
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.launchIn
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.onEach
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
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
    val settings: ThreadSettings = ThreadSettings(),
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
 * Add-project draft. Separate from [NewTaskDraft] because adding a project and
 * starting a task are different flows that happen to be reachable from the same
 * screen, and merging them made "which environment" mean two things.
 */
@Immutable
data class AddProjectDraft(
    val environmentId: EnvironmentId = EnvironmentId(""),
    /** `owner/name` chosen on the repository step, when cloning. */
    val repository: String? = null,
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
                // The device key is the account's enrollment, so it goes too. The
                // next sign-in enrolls a fresh device rather than inheriting one.
                DpopKey.clear()
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
                        deviceId = { null },
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

    private val _projectDraft = MutableStateFlow(AddProjectDraft())
    val projectDraft: StateFlow<AddProjectDraft> = _projectDraft.asStateFlow()

    private val _threadDrafts = MutableStateFlow<Map<String, ThreadDraft>>(emptyMap())
    val threadDrafts: StateFlow<Map<String, ThreadDraft>> = _threadDrafts.asStateFlow()

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
        }
    }

    /**
     * Snaps the new-task draft onto a usable agent whenever the catalog says its
     * current one is not.
     *
     * A draft's provider outlives the config that justified it: the default is
     * `codex`, which a server running only pi does not have, and a restored draft
     * can name an instance the user has since removed or signed out of. Sending
     * either is a refusal from the server, so the draft follows the catalog the way
     * `resolveDefaultableModelSelection` does in the RN client. An empty catalog
     * changes nothing — nothing is connected yet, which is not evidence the
     * selection is wrong.
     */
    private fun observeDraftProviderAvailability() {
        workspace.providerCatalog
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
     * Queues an external navigation request. A share also lands its payload in the
     * new-task draft here rather than at the screen, so the draft is durable before
     * anything is rendered.
     */
    fun openDeepLink(link: DeepLink) {
        if (link is DeepLink.Share) {
            _draft.update { draft ->
                val separator = if (draft.prompt.isBlank()) "" else "\n\n"
                draft.copy(
                    prompt = draft.prompt + separator + link.text.orEmpty().trim(),
                )
            }
            if (link.imageUris.isNotEmpty()) {
                addNewTaskDraftImages(
                    link.imageUris.map { ComposerImageCandidate(uri = it, mimeType = null) }
                )
            }
        }
        _pendingLink.value = link
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

    fun updateProjectDraft(transform: (AddProjectDraft) -> AddProjectDraft) =
        _projectDraft.update(transform)

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
        val message =
            newQueuedThreadMessage(
                environmentId = draft.environmentId,
                text = draft.prompt.trim(),
                attachments = draft.attachments,
                settings = draft.settings,
                creation =
                    club.touchtech.s5code.kotlin.data.StoredQueuedThreadCreation(
                        projectKey = draft.projectKey,
                        branch = draft.branch,
                        newWorktree = draft.workspaceMode == WorkspaceMode.NewWorktree,
                    ),
            )
        val durable = outboxStore.enqueue(message)
        if (durable.creation != null) workspace.setPendingThreadCreations(setOf(durable.key))
        outboxMutation.withLock {
            _outbox.update { current -> (current + durable).sortedBy { it.delivery.createdAt } }
        }
        updateDraft { it.copy(prompt = "", attachments = emptyList()) }
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
                    val busy =
                        threads.filter { it.status == club.touchtech.s5code.kotlin.model.ThreadStatus.Working }
                            .map { "${it.environmentId.value}/${it.id.value}" }
                            .toSet()
                    val next =
                        queued.sortedBy { it.delivery.createdAt }.firstOrNull {
                            val creationReady =
                                it.creation == null || workspace.projects.value.any { project ->
                                    project.environmentId == it.environmentId &&
                                        project.id.value == it.creation.projectKey
                                }
                            creationReady && it.environmentId in connected &&
                                (it.creation != null || it.key !in busy)
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
                    val thread = workspace.threads.value.firstOrNull {
                        it.environmentId == message.environmentId && it.id == message.threadId
                    }
                    if (environment?.state != club.touchtech.s5code.kotlin.model.ConnectionState.Connected ||
                        (message.creation == null &&
                            thread?.status == club.touchtech.s5code.kotlin.model.ThreadStatus.Working)
                    ) return
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
    fun refreshConnections() {
        workspace.refreshConnections()
    }

    /** Retries one environment's connection, for the connections screen. */
    fun retryEnvironment(environmentId: EnvironmentId) {
        (workspace as? LiveWorkspaceGateway)?.retry(environmentId)
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
            val models = modelsFor(provider)
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
    fun modelsFor(provider: ProviderInstance): List<String> =
        workspace.providerCatalog.value
            .firstOrNull { it.instance.instanceId == provider.instanceId }
            ?.models
            ?: emptyList()

    private companion object {
        const val PERSIST_DEBOUNCE_MILLIS = 400L
    }
}
