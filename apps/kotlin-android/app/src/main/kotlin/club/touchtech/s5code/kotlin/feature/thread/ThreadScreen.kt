package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.data.forProject
import club.touchtech.s5code.kotlin.model.ModelFavorite
import club.touchtech.s5code.kotlin.model.HandoffEndpoint
import club.touchtech.s5code.kotlin.model.ProviderCatalogEntry
import club.touchtech.s5code.kotlin.model.ProviderStatus
import club.touchtech.s5code.kotlin.model.formatProviderDriverName
import androidx.compose.material.icons.rounded.SwapHoriz
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material.icons.rounded.Public

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.ui.draw.clip
import club.touchtech.s5code.kotlin.design.component.S5BottomSheet
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.ArrowDownward
import androidx.compose.material.icons.rounded.Bolt
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.Groups
import androidx.compose.material.icons.rounded.Schedule
import androidx.compose.material.icons.rounded.BrokenImage
import androidx.compose.material.icons.rounded.Difference
import androidx.compose.material.icons.rounded.Folder
import androidx.compose.material.icons.rounded.History
import androidx.compose.material.icons.rounded.KeyboardArrowUp
import androidx.compose.material.icons.rounded.Source
import androidx.compose.material.icons.rounded.Stop
import androidx.compose.material.icons.rounded.Terminal
import androidx.compose.material.icons.rounded.TrackChanges
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.data.ThreadLinkRef
import club.touchtech.s5code.kotlin.data.parseThreadLink
import club.touchtech.s5code.kotlin.data.resolveThreadLink
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.app.ThreadDraft
import club.touchtech.s5code.kotlin.data.childInputsTitle
import club.touchtech.s5code.kotlin.data.clampFileAttachmentUploadBytes
import club.touchtech.s5code.kotlin.data.fileAttachmentTooLargeMessage
import club.touchtech.s5code.kotlin.data.formatComposerContextReference
import club.touchtech.s5code.kotlin.data.isUsageLimitsCommand
import club.touchtech.s5code.kotlin.data.persistPastedTextAttachment
import club.touchtech.s5code.kotlin.data.pullRequestComposerContext
import club.touchtech.s5code.kotlin.data.resolveVisibleWorktreeSetup
import club.touchtech.s5code.kotlin.data.threadDevicePreviews
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5FloatingAction
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.S5WaitState
import club.touchtech.s5code.kotlin.design.component.ScrollAnchor
import club.touchtech.s5code.kotlin.design.component.scrollAnchor
import club.touchtech.s5code.kotlin.design.component.scrollToAnchor
import club.touchtech.s5code.kotlin.design.component.rememberClipboardWriter
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.feature.connections.WaitNotice
import club.touchtech.s5code.kotlin.feature.connections.connectionPresentation
import club.touchtech.s5code.kotlin.feature.connections.showRetry
import club.touchtech.s5code.kotlin.feature.connections.waitNotice
import club.touchtech.s5code.kotlin.feature.connections.waitPillLabel
import club.touchtech.s5code.kotlin.feature.settings.TaskSettingsSheet
import club.touchtech.s5code.kotlin.feature.settings.ModelSearchScope
import club.touchtech.s5code.kotlin.feature.usage.ComposerUsageLimitsCard
import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.FeedEntry
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadStatus
import club.touchtech.s5code.kotlin.model.ThreadSummary
import club.touchtech.s5code.kotlin.model.ThreadSyncPhase
import club.touchtech.s5code.kotlin.platform.rememberComposerImageIntake
import club.touchtech.s5code.kotlin.platform.rememberComposerImagePicker
import club.touchtech.s5code.kotlin.platform.rememberQuestionFileIntake
import club.touchtech.s5code.kotlin.platform.rememberQuestionFilePicker
import club.touchtech.s5code.kotlin.platform.rememberQuestionMediaPicker
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.async
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

/**
 * Thread detail. Transcript, live follow with an escape hatch, pending
 * approval/input gates, composer, and the four tool destinations the header
 * carries (files, terminal, git, rewind).
 *
 * Thread lifecycle actions are deliberately absent. Pin, settle, snooze,
 * archive, delete, and title regeneration live on the home list's row menu,
 * which is where you act on threads as objects; this screen is where you work
 * inside one.
 */
@Composable
fun ThreadScreen(
    store: AppStore,
    environmentId: String,
    threadId: String,
    onBack: () -> Unit,
    onOpen: (String) -> Unit,
    /** "Work locally" replaces this thread with a new one — a full navigate, not a subroute. */
    onOpenThread: (environmentId: String, threadId: String) -> Unit = { _, _ -> },
    /** Navigates to a top-level route, such as an automation row's scheduled tasks. */
    onOpenRoute: (String) -> Unit = {},
) {
    val id = remember(threadId) { ThreadId(threadId) }
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    // Showing the thread is the open that feeds search ranking, whether or not
    // its shell has arrived; the first visit the server hears carries it
    // (`createThreadVisitTracker`). A Home search pick already sent it.
    var openPending by remember(environmentId, threadId) {
        mutableStateOf(club.touchtech.s5code.kotlin.data.ThreadOpens.claim(environmentId, threadId, System.currentTimeMillis()))
    }
    val context = androidx.compose.ui.platform.LocalContext.current
    // Subscribing is what starts this thread's stream, so it is keyed by both ids:
    // thread ids are only unique within one environment.
    val detail by
        remember(environmentId, threadId) { store.workspace.thread(env, id) }
            .collectAsStateWithLifecycle()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val childInputs by remember(env, id) { store.workspace.pendingChildInputs(env, id) }
        .collectAsStateWithLifecycle(emptyList())
    val syncPhase by
        remember(environmentId, threadId) { store.workspace.threadSyncPhase(env, id) }
            .collectAsStateWithLifecycle()
    val liveWorktreeSetup by
        remember(environmentId, threadId) { store.workspace.worktreeSetup(env, id) }
            .collectAsStateWithLifecycle()
    val threadDrafts by store.threadDrafts.collectAsStateWithLifecycle()
    val queuedMessages by store.outbox.collectAsStateWithLifecycle()
    val providerCatalog by store.workspace.providerCatalog.collectAsStateWithLifecycle()
    val providerCatalogs by store.workspace.providerCatalogs.collectAsStateWithLifecycle()
    val threadProjectId = detail?.summary?.projectId?.value
    val machineCatalog =
        remember(env, providerCatalogs, providerCatalog, threadProjectId) {
            (providerCatalogs[env]?.takeIf { it.isNotEmpty() } ?: providerCatalog).forProject(threadProjectId)
        }
    val attachmentError by store.attachmentError.collectAsStateWithLifecycle()
    val composerError =
        store.workspace.threadComposerErrors
            .collectAsStateWithLifecycle()
            .value["$environmentId:$threadId"]
    val stagedQuestionAttachments by store.questionAttachments.collectAsStateWithLifecycle()
    val preferences by store.preferences.collectAsStateWithLifecycle()
    val catalogRefreshing by store.catalogRefreshing.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val copy = rememberClipboardWriter()

    var settingsOpen by remember(threadId) { mutableStateOf(false) }
    var backgroundWorkOpen by remember(threadId) { mutableStateOf(false) }
    // One status action at a time, released when the command settles: an accepted
    // Stop can leave tasks running, so a retry must stay possible.
    var statusActionPending by remember(threadId) { mutableStateOf(false) }
    var following by remember(threadId) { mutableStateOf(true) }
    // The questionnaire owns the composer slot while a user-input request is
    // open, matching the RN client: expanded it is the full card, collapsed it
    // is a composer-style bar in the same place.
    var userInputCollapsed by remember(threadId) { mutableStateOf(false) }
    val draft = threadDrafts["$environmentId/$threadId"] ?: threadDrafts[threadId] ?: ThreadDraft()
    // Every image path (keyboard paste, drop, explicit paste, picker) goes
    // through intake so the draft holds a cached copy, not a lapsing grant.
    val addImages = rememberComposerImageIntake { candidates ->
        store.addThreadDraftImages(environmentId, threadId, candidates)
    }
    val pickImages = rememberComposerImagePicker(
        remaining = ComposerAttachmentLimits.MAX_ATTACHMENTS - draft.attachments.size,
        onImages = addImages,
    )

    val listState = rememberLazyListState()
    val current = detail
    val environment = remember(environments, environmentId) {
        environments.firstOrNull { it.id.value == environmentId }
    }
    val health = environment?.let { connectionPresentation(it.state) }
    // Whether this thread has ever produced a snapshot in this session. Without it,
    // "no detail" is ambiguous: it is the moment before the first snapshot arrives
    // and also the moment after the thread is deleted, and those want opposite
    // screens. A spinner on a deleted thread never stops.
    var everLoaded by remember(threadId) { mutableStateOf(false) }
    LaunchedEffect(current != null) { if (current != null) everLoaded = true }

    if (current == null) {
        // No snapshot yet. That is a wait, not a missing thread, until the connection
        // is live and still has nothing to say about this id.
        val opening =
            if (everLoaded) null
            else
                waitNotice(
                    states = listOfNotNull(environment?.state),
                    environmentLabel = environment?.label,
                    resourceName = "transcript",
                    hasContent = false,
                    // Only a synchronized detail stream can declare this thread
                    // missing; a connected environment still mid-subscribe is
                    // "Loading", not "not available".
                    loaded = syncPhase == ThreadSyncPhase.Live,
                )
        S5Screen(title = "Thread", onBack = onBack) { padding ->
            if (opening != null) {
                S5WaitState(
                    title = opening.title,
                    detail = opening.detail,
                    icon = health?.icon ?: Icons.Rounded.Difference,
                    spinning = opening.spinning,
                    actionLabel = if (opening.showRetry) "Retry now" else null,
                    onAction = { store.retryEnvironment(env) },
                    modifier = Modifier.padding(padding),
                )
            } else {
                S5EmptyState(
                    icon = Icons.Rounded.Difference,
                    title = "Thread unavailable",
                    detail = "This thread was deleted or is no longer available.",
                    actionLabel = "Back to home",
                    onAction = onBack,
                    modifier = Modifier.padding(padding),
                )
            }
        }
        return
    }

    val summary = current.summary
    var lastVisitSentAt by remember(env, id) { mutableStateOf(0L) }
    var lastVisitWatermark by remember(env, id) { mutableStateOf(0L) }
    LaunchedEffect(env, id, summary.updatedAtMillis, summary.lastVisitedAtMillis,
        summary.latestCompletedAtMillis, summary.wokeAtMillis, environment?.capabilities?.threadVisitedTracking, syncPhase) {
        if (environment?.capabilities?.threadVisitedTracking != true || syncPhase != ThreadSyncPhase.Live) return@LaunchedEffect
        // A scheduled wake can happen without a newer server update, so the wake
        // itself is the watermark when it is the latest thing to have happened.
        val wokeAt = summary.wokeAtMillis
        val wakeIsLatest = wokeAt != null && wokeAt >= summary.updatedAtMillis
        val watermark = maxOf(summary.updatedAtMillis, wokeAt ?: 0)
        if (openPending && watermark > 0) {
            openPending = false
            // Sent outside this effect, which restarts on every streamed update.
            store.sendThreadOpen(env, id, watermark)
            lastVisitWatermark = watermark
            lastVisitSentAt = System.currentTimeMillis()
            return@LaunchedEffect
        }
        if (watermark <= 0 || watermark <= (summary.lastVisitedAtMillis ?: 0) || watermark <= lastVisitWatermark) return@LaunchedEffect
        val unseenCompletion = (summary.latestCompletedAtMillis ?: 0) > (summary.lastVisitedAtMillis ?: 0)
        // Like a completion, a wake the user is looking at must not wait out the
        // throttle: leaving within it would cancel the visit and keep Woke on the row.
        if (!unseenCompletion && !wakeIsLatest) kotlinx.coroutines.delay((10_000 - (System.currentTimeMillis() - lastVisitSentAt)).coerceAtLeast(0))
        try {
            store.workspace.visitThread(env, id, watermark)
            lastVisitWatermark = watermark
            lastVisitSentAt = System.currentTimeMillis()
        } catch (cancelled: kotlinx.coroutines.CancellationException) { throw cancelled }
        catch (_: Exception) { /* Read tracking is retried with the next live watermark. */ }
    }
    // The bootstrap worktree card: the live stream wins while it is at least as
    // fresh as the recorded activity, and the whole row hides once a follow-up
    // turn makes the setup history (`resolveVisibleWorktreeSetup` in
    // client-runtime). A running setup before the thread exists is covered by
    // the gateway's gate on pendingCreationKeys.
    val worktreeSetup =
        remember(liveWorktreeSetup, current.recordedWorktreeSetup, current.latestTurn, current.feed, queuedMessages) {
            resolveVisibleWorktreeSetup(
                live = liveWorktreeSetup,
                recorded = current.recordedWorktreeSetup,
                turnStarted = current.latestTurn?.startedAtMillis != null,
                followUpSent =
                    current.feed.count { it is FeedEntry.UserMessage } +
                        queuedMessages.count {
                            it.environmentId == env && it.threadId == id
                        } > 1,
            )
        }
    // Files staged on a request that resolved elsewhere (another client, a
    // dismissed card) are released once the pending set no longer retains them —
    // RN's `questionAttachmentDraftPrefix` sweep.
    LaunchedEffect(environmentId, threadId, current.userInput?.id) {
        store.releaseStaleQuestionAttachments(
            environmentId,
            threadId,
            setOfNotNull(current.userInput?.id),
        )
    }
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val project =
        remember(projects, summary.projectId, environmentId) {
            projects.firstOrNull {
                it.environmentId.value == environmentId && it.id == summary.projectId
            }
        }
    val workspaceRoot =
        remember(projects, summary.projectId, environmentId, current) {
            current.workspaceRoot ?: project?.workspaceRoot
        }
    // `/compact` only makes sense once there is a conversation to compact; a
    // paged-out history counts the same way RN's loadEarlier check does.
    val hasCompactableConversation =
        remember(current.feed, current.page?.hasMore) {
            current.feed.any { entry ->
                entry is FeedEntry.UserMessage &&
                    (entry.attachments.isNotEmpty() ||
                        entry.text.trim().lowercase() != "/compact")
            } || current.page?.hasMore == true
        }
    val effectiveSettings = draft.settings ?: current.settings
    val settingsAreLive =
        syncPhase == ThreadSyncPhase.Live &&
            environment?.state == club.touchtech.s5code.kotlin.model.ConnectionState.Connected
    LaunchedEffect(draft.settings, current.settings, settingsAreLive) {
        if (settingsAreLive && draft.settings != null) {
            store.releaseMatchingThreadDraftSettings(environmentId, threadId, current.settings)
        }
    }
    // `pendingHandoff`: the composer's selection against the model the thread
    // last ran on. The catalog key stands in for provider config changes.
    val pendingHandoff =
        remember(current.handoffSource, effectiveSettings.provider.instanceId, effectiveSettings.model, machineCatalog, summary.projectId) {
            store.workspace.pendingHandoff(
                env,
                current.handoffSource,
                HandoffEndpoint(effectiveSettings.provider.instanceId, effectiveSettings.model),
                projectId = summary.projectId.value,
            )
        }
    val skills by
        store.workspace
            .providerSkills(env, effectiveSettings.provider, workspaceRoot)
            .collectAsStateWithLifecycle()
    // RN's canAttach for folded pastes: uploads exist, the draft has a slot,
    // and the server's per-file ceiling is known.
    val pastedTextMaxBytes =
        environment?.capabilities
            ?.takeIf { it.attachmentUploads }
            ?.fileAttachments?.maxUploadBytes
            ?.let { clampFileAttachmentUploadBytes(it) }
    // The provider's own "this agent is unsupported/broken" advisory — a line
    // above the composer, as RN renders it.
    val providerStatuses by
        store.workspace.providerStatuses(env).collectAsStateWithLifecycle()
    // Limits data for this driver is what makes `/usage-limits` T3's command.
    val offersUsageLimits by
        store.workspace
            .usageLimitsOffered(env, effectiveSettings.provider.driver)
            .collectAsStateWithLifecycle()
    var usageLimitsOpen by remember(threadId) { mutableStateOf(false) }
    val usageLimitsView by store.workspace.usageLimits.collectAsStateWithLifecycle()
    // `devicePreviews`: how many devices this thread holds open — the composer
    // button count and the preview screen's picker read the same list.
    val deviceState by
        remember(environmentId) { store.workspace.deviceState(env) }
            .collectAsStateWithLifecycle()
    val devicePreviews = remember(deviceState, threadId) { threadDevicePreviews(deviceState, threadId) }
    val providerAdvisory =
        remember(providerStatuses, effectiveSettings.provider) {
            providerStatuses
                .firstOrNull { it.instanceId == effectiveSettings.provider.instanceId }
                ?.takeIf {
                    it.compatibilityStatus == "unsupported" ||
                        it.compatibilityStatus == "broken"
                }
                ?.compatibilityMessage
        }
    val pullRequestRepository =
        project?.repositoryIdentity?.displayName
            ?.takeIf { environment?.capabilities?.pullRequests == true }
    val working = summary.liveStatus == ThreadStatus.Working || summary.blockedOnThreadWait
    var followUp by remember(threadId, preferences.followUpBehavior) { mutableStateOf(preferences.followUpBehavior) }
    val plan = remember(current.feed) { activePlan(current.feed) }
    // Long runs of tool calls fold behind a disclosure row, as they do in the RN
    // and desktop feeds. The expansion set is per-thread view state, so leaving and
    // returning starts folded again — which is the state a long transcript should
    // open in.
    var expandedWorkGroups by remember(threadId) { mutableStateOf(emptySet<String>()) }
    // A finished turn folds down to its answer, with its work behind the header. New
    // turns start folded, which is what makes a long transcript readable: the thing
    // worth reading is what the agent said, not the forty tool calls it took.
    var expandedTurns by remember(threadId) { mutableStateOf(emptySet<String>()) }
    // Collapsing removes rows from the list, and `LazyColumn` anchors the viewport
    // on the first visible item's key: if the fold you are closing was holding that
    // item, the list keeps the bare index and you land on whatever slides under it —
    // a screen of unrelated history. So expanding remembers where the transcript
    // stood, and collapsing puts it back. Fallback is the toggle's own key, which
    // always survives its fold.
    val foldScrollAnchors = remember(threadId) { mutableStateMapOf<String, ScrollAnchor>() }
    var pendingScrollRestore by
        remember(threadId) { mutableStateOf<Pair<ScrollAnchor, String>?>(null) }
    fun toggleFold(foldId: String, foldKey: String, currentlyExpanded: Boolean, mutate: () -> Unit) {
        if (currentlyExpanded) {
            foldScrollAnchors.remove(foldId)?.let { pendingScrollRestore = it to foldKey }
        } else {
            listState.scrollAnchor()?.let { foldScrollAnchors[foldId] = it }
        }
        mutate()
    }
    // The same jump, from a single row's own disclosure: a tool card expanded
    // and scrolled into holds the anchor item, and collapsing it lets the list
    // keep a scroll offset the shrunk row no longer spans. Record where the
    // transcript stood on expand and put it back on collapse, as folds do.
    //
    // A lone call standing in for its group (`standInGroupId`) opens and closes
    // the group with it, so a second call joining keeps both open. Its anchor is
    // kept under the group's id, so collapsing that group later still restores it.
    var expandedEntries by remember(threadId) { mutableStateOf(emptySet<String>()) }
    fun toggleEntryExpand(entryId: String, rowKey: String = entryId, standInGroupId: String? = null) {
        val next = entryId !in expandedEntries
        toggleFold(standInGroupId ?: "entry:$entryId", rowKey, !next) {
            expandedEntries = if (next) expandedEntries + entryId else expandedEntries - entryId
            standInGroupId?.let {
                expandedWorkGroups = if (next) expandedWorkGroups + it else expandedWorkGroups - it
            }
        }
    }
    // The clock the "Working for 12s" row measures from, or null when nothing is
    // running. Only the start is derived here; the row ticks itself, so a live turn
    // does not rebuild the presented list once a second.
    val activeWorkStartedAt =
        remember(current.latestTurn, current.sessionStatus, current.sessionUpdatedAtMillis) {
            activeWorkStartedAtMillis(
                latestTurn = current.latestTurn,
                sessionStatus = current.sessionStatus,
                sessionStartedAtMillis = current.sessionUpdatedAtMillis,
            )
        }
    // Automation rows lead with their task's title, as RN's `scheduledTasksLive`
    // query. Read once, and only when the transcript has one to name.
    val hasAutomationRows = current.feed.any { it is FeedEntry.UserMessage && it.scheduledTaskId != null } ||
        current.queuedRuns.any { it.scheduledTaskId != null }
    var scheduledTaskTitles by remember(environmentId) { mutableStateOf(emptyMap<String, String>()) }
    LaunchedEffect(environmentId, hasAutomationRows) {
        if (!hasAutomationRows || scheduledTaskTitles.isNotEmpty()) return@LaunchedEffect
        try {
            val payload = store.workspace.environmentStream(env, "scheduledTasks.subscribe").first()
            scheduledTaskTitles = club.touchtech.s5code.kotlin.data.scheduledTaskRows(payload).mapNotNull { task ->
                val taskId = task["id"]?.jsonPrimitive?.contentOrNull
                val title = task["title"]?.jsonPrimitive?.contentOrNull
                if (taskId != null && !title.isNullOrBlank()) taskId to title else null
            }.toMap()
        } catch (cancelled: CancellationException) { throw cancelled }
        catch (_: Exception) { /* Rows fall back to the prompt's first line. */ }
    }
    // `isCompacting` in `use-thread-composer-state.ts`: the active run has a
    // compaction still in flight (automatic ones included), or its newest user
    // message is `/compact` and no compaction has settled since. Kotlin feed rows
    // do not carry the run id on user messages, so the last message stands in for
    // "the active run asked to compact": anything older belongs to a turn that
    // already finished. A compaction row without a status (V1) counts as settled.
    val isCompacting =
        remember(current.feed, activeWorkStartedAt, current.latestTurn?.turnId) {
            if (activeWorkStartedAt == null) {
                false
            } else {
                val activeRunId = current.latestTurn?.turnId
                val compactionRunning = activeRunId != null && current.feed.any {
                    it is FeedEntry.Note && it.compaction && it.turnId == activeRunId &&
                        it.status in setOf("pending", "running", "waiting")
                }
                val lastUserIndex =
                    current.feed.indexOfLast { it is FeedEntry.UserMessage }
                compactionRunning || (lastUserIndex >= 0 &&
                    (current.feed[lastUserIndex] as FeedEntry.UserMessage)
                        .text.trim().equals("/compact", ignoreCase = true) &&
                    current.feed.drop(lastUserIndex + 1).none {
                        it is FeedEntry.Note && it.compaction &&
                            (it.status == null || it.status in setOf("completed", "failed", "interrupted", "cancelled"))
                    })
            }
        }
    // A compacting turn cannot take steering input, so follow-ups queue meanwhile.
    val canSteer = current.canSteer && !isCompacting
    // A message sent over open approvals declines them and redirects the agent,
    // as RN's `declineBeforeDispatch`; an open questionnaire owns the composer
    // instead. The redirect steers the waiting turn rather than queuing behind it.
    val redirectsApprovals = current.approvals.isNotEmpty() && current.userInput == null
    val followUpMode = if (redirectsApprovals) "steer" else followUp
    val latestApprovals by rememberUpdatedState(current.approvals)
    var redirecting by remember(threadId) { mutableStateOf(false) }
    // The approval card's answer in flight. A redirect would decline the same
    // request and race it, so it waits, as RN's `respondingApprovalId` does.
    var respondingApprovalId by remember(threadId) { mutableStateOf<String?>(null) }
    // Each turn's checkpoint totals, for the "+12 −3" on its work row. A
    // checkpoint that changed nothing has nothing to review.
    val turnDiffStats =
        remember(current.checkpoints) {
            current.checkpoints
                .filter { it.turnId != null && it.filesChanged > 0 }
                .associate { it.turnId!! to TurnDiffStat(it.filesChanged, it.additions, it.deletions) }
        }
    val rows =
        remember(
            current.feed,
            expandedWorkGroups,
            expandedTurns,
            current.latestTurn,
            activeWorkStartedAt,
            current.turnEnded,
            worktreeSetup,
            turnDiffStats,
        ) {
            val presented =
                presentFeed(
                    feed = current.feed,
                    expandedGroups = expandedWorkGroups,
                    latestTurn = current.latestTurn,
                    expandedTurns = expandedTurns,
                    // An open run whose provider turn ended is only settling
                    // (subagents finishing), so the feed does not claim it is
                    // working; stop and the composer still see the real run.
                    activeWorkStartedAtMillis = activeWorkStartedAt.takeUnless { current.turnEnded },
                    turnDiffStats = turnDiffStats,
                )
            if (worktreeSetup == null) return@remember presented
            // RN anchors the card on the first user message (the prompt whose
            // turn is being set up). With no messages it floats to the visual
            // top instead — handled as a trailing lazy item, not a row.
            val anchor =
                presented.indexOfFirst {
                    it is FeedRow.Entry && it.entry is FeedEntry.UserMessage
                }
            if (anchor >= 0) {
                presented.toMutableList().also { it.add(anchor, FeedRow.WorktreeSetup) }
            } else {
                presented
            }
        }
    // True when the setup card renders as the unanchored top item rather than
    // spliced under the first prompt.
    val setupAtTop = worktreeSetup != null && rows.none { it is FeedRow.WorktreeSetup }
    // One card body for both placements: the spliced row and the floating top
    // item differ only in where the list puts them.
    val setupCard: (@Composable () -> Unit)? =
        worktreeSetup?.let { setup ->
            {
                WorktreeSetupCard(
                    snapshot = setup,
                    turnStarted = current.latestTurn?.startedAtMillis != null,
                    turnStartedAtMillis = current.latestTurn?.startedAtMillis,
                    working = activeWorkStartedAt != null,
                    onCancel = {
                        scope.launch {
                            runCatching { store.workspace.cancelWorktreeSetup(env, id) }
                                .onFailure {
                                    store.showError(
                                        it.message ?: "The setup could not be cancelled."
                                    )
                                }
                        }
                    },
                    onWorkLocally =
                        if (store.workspace.retainedThreadCreation(env, id) != null) {
                            {
                                scope.launch {
                                    store
                                        .workLocally(environmentId, threadId)
                                        ?.let { onOpenThread(environmentId, it.value) }
                                        ?: store.showError(
                                            "The setup could not be switched to a local workspace."
                                        )
                                }
                            }
                        } else {
                            null
                        },
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    // Keys in lazy-index order: the approval gate sits at index 0 (visual bottom
    // of the reversed list), the feed rows reversed after it, "Load earlier" last.
    val lazyKeys =
        remember(
            rows,
            current.approval?.id,
            current.page?.hasMore,
            current.page?.beforeCursor,
            setupAtTop,
        ) {
            buildList<Any> {
                current.approval?.let { add("approval-${it.id}") }
                rows.asReversed().forEach { add(it.key) }
                if (current.page?.hasMore == true && current.page.beforeCursor != null) {
                    add("load-earlier")
                }
                // The reversed list's visual top: the floating setup card sits
                // above "Load earlier", matching RN's ListHeaderComponent order.
                if (setupAtTop) add("worktree-setup")
            }
        }
    // `expandedEntries` is a key even though `presentFeed` does not read it: a
    // row-level collapse restores through here too, and its toggle leaves
    // `rows` unchanged.
    LaunchedEffect(rows, expandedEntries) {
        val pending = pendingScrollRestore ?: return@LaunchedEffect
        pendingScrollRestore = null
        listState.scrollToAnchor(pending.first, lazyKeys, pending.second)
    }
    // The tall title belongs to the top of the thread and nowhere else: the header
    // expands once the oldest entry is in view, and is compact everywhere below
    // that. The decision (including its hysteresis) lives in `atHistoryTop`.
    var atTop by remember(threadId) { mutableStateOf(false) }
    LaunchedEffect(listState, threadId) {
        snapshotFlow {
                listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index to
                    listState.layoutInfo.totalItemsCount
            }
            .distinctUntilChanged()
            .collect { (lastVisible, total) ->
                atTop = atHistoryTop(lastVisible ?: -1, total - 1, atTop)
            }
    }

    // Follow the tail while the user has not scrolled away, and resume the moment
    // they come back to it. The thresholds live in `shouldFollowTail`.
    LaunchedEffect(listState, threadId) {
        snapshotFlow { listState.firstVisibleItemIndex to listState.firstVisibleItemScrollOffset }
            .distinctUntilChanged()
            .collect { (index, offset) -> following = shouldFollowTail(index, offset, following) }
    }
    LaunchedEffect(rows.size, following) {
        if (following && rows.isNotEmpty()) listState.animateScrollToItem(0)
    }
    // A Home search match opens the thread at that message (RN's threadMessageJump).
    ThreadMessageJumpEffect(
        threadKey = threadJumpKey(environmentId, threadId),
        feed = current.feed,
        rows = rows,
        expandedTurns = expandedTurns,
        hasMoreHistory = current.page?.let { it.hasMore && it.beforeCursor != null } == true,
        loadingHistory = current.page?.loadingOlder == true,
        loadEarlier = { store.workspace.loadOlderTurns(env, id) },
        unfoldTurn = { turnId -> expandedTurns = expandedTurns + turnId },
        scrollToRow = { rowIndex ->
            following = false
            // Reversed list: the approval gate and pending handoff sit below the rows.
            val leading = listOfNotNull(current.approval, pendingHandoff).size
            listState.scrollToItem(leading + rows.lastIndex - rowIndex)
        },
    )

    // The single wait indicator for this screen. A transcript with rows already drawn
    // gets the pill; an empty one gets the whole screen, matching how the RN client
    // splits the two. Rows rather than feed entries, because a brand new thread whose
    // first turn is running has a working row and nothing else — and that row is the
    // answer to "is anything happening", so it must not be replaced by a spinner.
    val wait =
        remember(environment?.state, environment?.label, rows.isEmpty(), detail != null) {
            waitNotice(
                states = listOfNotNull(environment?.state),
                environmentLabel = environment?.label,
                resourceName = "transcript",
                hasContent = rows.isNotEmpty(),
                loaded = detail != null,
            )
        }

    S5Screen(
        title = summary.title,
        subtitle =
            listOfNotNull(
                    summary.branch,
                    summary.provider.label,
                    machineCatalog
                        .firstOrNull { it.instance.instanceId == summary.provider.instanceId }
                        ?.modelLabel(summary.model) ?: summary.model,
                )
                .joinToString(" · "),
        prominence = S5TopBarProminence.Hero,
        onBack = onBack,
        topBarCollapsed = !atTop,
        // The plan strip takes over the space the tall title gives up, so the
        // header's height barely changes: the title's second line and the plan
        // line are never both on screen. Driven by the bar's own collapsed
        // fraction rather than a scroll offset, so the two cannot disagree. It is
        // also gated on the turn still being live — a strip naming the last step of
        // a finished turn reads as work in progress.
        belowTopBar = { collapsedFraction ->
            ActivePlanBar(
                plan = plan,
                visible =
                    collapsedFraction >= PLAN_BAR_COLLAPSE_THRESHOLD &&
                        planBarApplies(summary.liveStatus),
            )
        },
        actions = {
            // The same four controls the RN client's Android header carries, in
            // the same order. Thread lifecycle (pin, settle, snooze, archive,
            // delete, regenerate) lives on the home list's row menu there, and
            // model choice lives in the composer, so neither belongs here. A
            // status badge does not either: the composer's stop button and the
            // transcript already say whether the agent is working, and the badge
            // was the third place saying it.
            S5IconButton(
                icon = Icons.Rounded.Folder,
                label = "Open files",
                onClick = { onOpen("files") },
            )
            S5IconButton(
                icon = Icons.Rounded.Terminal,
                label = "Open terminal",
                onClick = { onOpen("terminal") },
            )
            S5IconButton(
                icon = Icons.Rounded.Source,
                label = "Open git controls",
                onClick = { onOpen("git") },
            )
            S5IconButton(
                icon = Icons.Rounded.History,
                label = "Session rewind",
                onClick = { onOpen("rewind") },
            )
            if (environment?.capabilities?.serverBrowser == true) {
                S5IconButton(icon = Icons.Rounded.Public,
                    label = "Browser", onClick = { onOpen("browser") })
            }
        },
        bottomBar = {
            // Scaffold overlays direct slot children; controls must sit above the composer.
            // The slot is measured against the whole screen, so the column is capped
            // below the top bar: the usage card yields that space and scrolls rather
            // than pushing its close button under the header when the keyboard opens.
            BoxWithConstraints {
            val statusBar = with(LocalDensity.current) { WindowInsets.statusBars.getTop(this).toDp() }
            Column(
                Modifier.fillMaxWidth()
                    .heightIn(max = (maxHeight - statusBar - BOTTOM_BAR_TOP_RESERVE).coerceAtLeast(0.dp))
            ) {
            // RN's child-input banner: one line over every blocked subagent, opening the first.
            childInputs.firstOrNull()?.let { child ->
                androidx.compose.material3.TextButton(
                    onClick = { onOpenThread(environmentId, child.threadId) },
                    modifier = Modifier.semantics {
                        contentDescription = "Open ${if (child.approval) "approval" else "question"} from ${child.title}"
                    },
                ) {
                    Text(childInputsTitle(childInputs))
                }
            }
            // Why the last send never left the queue, dismissable above the
            // composer — RN's ComposerErrorNotice in the same slot.
            composerError?.let { error ->
                ComposerErrorNotice(
                    message = error.message,
                    onDismiss = {
                        store.workspace.clearThreadComposerError("$environmentId:$threadId")
                    },
                    modifier =
                        Modifier.padding(
                            horizontal = S5Theme.spacing.gutter,
                            vertical = S5Theme.spacing.tiny,
                        ),
                )
            }
            val pendingInput = current.userInput
            if (pendingInput != null) {
                val capabilities = environment?.capabilities
                val attachmentSupport =
                    remember(capabilities) {
                        capabilities?.let {
                            QuestionAttachmentSupport(
                                enabled = it.questionAttachments,
                                supportsFiles = it.fileAttachments != null,
                            )
                        }
                    }
                // Staged uploads, re-keyed by question id for the card. The
                // send-turn cap is request-wide, matching RN's sibling count.
                val stagedByQuestion =
                    remember(stagedQuestionAttachments, pendingInput.id) {
                        store.questionAttachmentsFor(environmentId, threadId, pendingInput.id)
                    }
                // Pickers feed one question at a time; the id travels in state
                // because the launcher callback carries only URIs.
                var preparingQuestions by
                    remember(pendingInput.id) { mutableStateOf(emptySet<String>()) }
                var pickingQuestion by
                    remember(pendingInput.id) { mutableStateOf<String?>(null) }
                val maxFileBytes =
                    capabilities?.fileAttachments?.maxUploadBytes?.let {
                        minOf(it, ComposerAttachmentLimits.MAX_FILE_BYTES)
                    }
                val intake =
                    rememberQuestionFileIntake(maxFileBytes) { staged ->
                        val question = pickingQuestion
                        pickingQuestion = null
                        if (question == null) {
                            // No question owns the result (the request changed
                            // mid-pick): free the copies rather than leak them.
                            staged.files.forEach { java.io.File(it.localPath).delete() }
                        } else {
                            preparingQuestions = preparingQuestions - question
                            staged.error?.let(store::showError)
                            store.stageQuestionAttachments(
                                environmentId, threadId, pendingInput.id, question, staged.files,
                            )
                        }
                    }
                val pickFiles = rememberQuestionFilePicker(onPicked = intake)
                val pickMedia =
                    rememberQuestionMediaPicker(
                        remaining =
                            ComposerAttachmentLimits.MAX_ATTACHMENTS -
                                stagedByQuestion.values.sumOf { it.size },
                        allowVideos = attachmentSupport?.supportsFiles == true,
                        onPicked = intake,
                    )
                // The card replaces the composer outright, so it must carry the
                // same IME and navigation-bar padding the composer would.
                Column(
                    Modifier.imePadding().navigationBarsPadding()
                ) {
                var submitting by remember(pendingInput.id) { mutableStateOf(false) }
                if (userInputCollapsed) {
                    UserInputCollapsedBar(
                        questionCount = pendingInput.questions.size,
                        working = working,
                        onExpand = { userInputCollapsed = false },
                        onStop = {
                            scope.launch {
                                runCatching { store.workspace.cancelTurn(env, id) }
                                    .onFailure {
                                        store.showError(
                                            it.message ?: "The turn could not be stopped."
                                        )
                                    }
                            }
                        },
                    )
                } else {
                    UserInputCard(
                        request = pendingInput,
                        submitting = submitting,
                        modifier =
                            Modifier.padding(
                                horizontal = S5Theme.spacing.medium,
                                vertical = S5Theme.spacing.small,
                            ),
                        attachments = stagedByQuestion,
                        attachmentSupport = attachmentSupport,
                        preparingQuestions = preparingQuestions,
                        onPickImages = { questionId ->
                            pickingQuestion = questionId
                            preparingQuestions = preparingQuestions + questionId
                            pickMedia()
                        },
                        onPickFiles = { questionId ->
                            pickingQuestion = questionId
                            preparingQuestions = preparingQuestions + questionId
                            pickFiles()
                        },
                        onRemoveAttachment = { questionId, attachment ->
                            store.removeQuestionAttachment(
                                environmentId, threadId, pendingInput.id, questionId,
                                attachment.localId,
                            )
                        },
                        onRetryAttachment = { questionId, attachment ->
                            store.retryQuestionAttachment(
                                environmentId, threadId, pendingInput.id, questionId,
                                attachment.localId,
                            )
                        },
                        onSubmit = { submission ->
                            if (!submitting) {
                                submitting = true
                                scope.launch {
                                    try {
                                        store.workspace.respondToInput(
                                            env,
                                            id,
                                            pendingInput.id,
                                            submission.answers,
                                            submission.attachmentsByQuestionId,
                                        )
                                        // Sent uploads belong to the request
                                        // now; the draft copies are released.
                                        store.releaseQuestionAttachments(
                                            environmentId, threadId, pendingInput.id,
                                        )
                                    } catch (error: Exception) {
                                        store.showError(
                                            error.message ?: "The answer could not be sent."
                                        )
                                    } finally {
                                        submitting = false
                                    }
                                }
                            }
                        },
                        onDismiss =
                            if (pendingInput.dismissible) {
                                {
                                    if (!submitting) {
                                        submitting = true
                                        scope.launch {
                                            try {
                                                store.workspace.dismissInput(
                                                    env,
                                                    id,
                                                    pendingInput.id,
                                                )
                                                store.releaseQuestionAttachments(
                                                    environmentId, threadId, pendingInput.id,
                                                )
                                            } catch (error: Exception) {
                                                store.showError(
                                                    error.message
                                                        ?: "The question could not be dismissed."
                                                )
                                            } finally {
                                                submitting = false
                                            }
                                        }
                                    }
                                }
                            } else {
                                null
                            },
                    )
                }
                }
            } else {
            if (usageLimitsOpen && offersUsageLimits) {
                // Docked above the composer like RN's ComposerUsageLimits —
                // opaque, one size down from the Limits tab's cards.
                ComposerUsageLimitsCard(
                    view = usageLimitsView,
                    driver = effectiveSettings.provider.driver,
                    now = System.currentTimeMillis(),
                    onRedeemCredit = { account ->
                        val redeem = account.redeem ?: return@ComposerUsageLimitsCard
                        scope.launch {
                            runCatching {
                                    store.workspace.consumeResetCredit(
                                        redeem.environmentId, redeem,
                                    )
                                }
                                .onSuccess { result ->
                                    store.showError(
                                        when (result.outcome) {
                                            "reset" -> "Usage limits reset."
                                            "alreadyRedeemed" -> "That credit was already used."
                                            "noCredit" -> "No reset credit is available."
                                            "nothingToReset" -> "There is nothing to reset."
                                            else -> result.warning ?: "Reset credit failed."
                                        }
                                    )
                                }
                                .onFailure {
                                    store.showError(
                                        it.message ?: "The reset credit could not be used.",
                                    )
                                }
                        }
                    },
                    onClose = { usageLimitsOpen = false },
                    modifier =
                        Modifier.weight(1f, fill = false)
                            .fillMaxWidth()
                            .padding(
                                horizontal = S5Theme.spacing.gutter,
                                vertical = S5Theme.spacing.tiny,
                            ),
                )
            }
            key(current.latestTurn?.turnId) {
                ChatGptUsageLimitNotice(
                    lastErrorCode = current.lastErrorCode,
                    onCopy = copy,
                    modifier = Modifier.padding(horizontal = 12.dp).padding(bottom = 8.dp),
                )
            }
            V2ThreadControls(store, env, id, current, working, followUp, canSteer = canSteer,
                onFollowUp = { followUp = it }, onOpenThread = { onOpenThread(environmentId, it) },
                scheduledTaskTitles = scheduledTaskTitles,
                senderThreadTitle = { senderId ->
                    (store.workspace.threads.value + store.workspace.archived.value)
                        .firstOrNull { it.environmentId.value == environmentId && it.id.value == senderId }?.title
                })
            if (current.providerNativeSubagent) {
                Text("Provider-managed subagent · ${summary.status}", modifier = Modifier.padding(16.dp),
                    style = MaterialTheme.typography.bodySmall)
            } else ThreadComposer(
                sendLabel =
                    if (working || redirectsApprovals) {
                        if (canSteer && followUpMode == "steer") "Steer" else "Queue"
                    } else {
                        "Send"
                    },
                placeholder =
                    when {
                        current.approval?.supportsFeedback == true -> "Send feedback to decline this request"
                        current.approval != null -> "Send a message to decline and redirect the agent"
                        else -> "Ask anything"
                    },
                value = draft.text,
                commands =
                    remember(effectiveSettings.provider, workspaceRoot) {
                        store.workspace.slashCommands(effectiveSettings.provider, workspaceRoot)
                    },
                skills = skills,
                onSearchPaths = { query -> store.workspace.searchPaths(env, id, query) },
                onSearchPullRequests =
                    pullRequestRepository?.let { repository ->
                        { query: String ->
                            store.workspace.searchComposerPullRequests(
                                env, summary.projectId, repository, query,
                                threadPullRequests = current.pullRequests,
                            )
                        }
                    },
                pullRequestRepository = pullRequestRepository,
                onPickPullRequest = pickPullRequest@{ pullRequest, rangeStart, rangeEnd ->
                    // RN's COMPOSER_CONTEXT_MAX_RECORDS — a draft carries at most
                    // this many context payloads.
                    if (draft.contextRecords.size >= 200) {
                        store.showError(
                            "Too many context items. Remove some context from the draft and try again.",
                        )
                        return@pickPullRequest
                    }
                    val record = pullRequestComposerContext(pullRequest)
                    val nextText =
                        replaceComposerTextRange(
                            draft.text,
                            rangeStart,
                            rangeEnd,
                            formatComposerContextReference(record) + " ",
                        )
                    store.setThreadDraftWithContext(
                        environmentId,
                        threadId,
                        nextText,
                        draft.contextRecords + record,
                    )
                },
                onRefreshWorkspaceSnapshot = {
                    store.workspace.refreshProviderWorkspaceSnapshot(
                        env, effectiveSettings.provider, workspaceRoot,
                    )
                },
                canAttachPastedText = {
                    pastedTextMaxBytes != null &&
                        draft.attachments.size < ComposerAttachmentLimits.MAX_ATTACHMENTS
                },
                onPastedText = onPastedText@{ text ->
                    val maxBytes = pastedTextMaxBytes
                    if (maxBytes == null) {
                        store.showError("This server does not support file attachments.")
                        return@onPastedText
                    }
                    val bytes = text.toByteArray(Charsets.UTF_8).size.toLong()
                    if (bytes > maxBytes) {
                        store.showError(fileAttachmentTooLargeMessage("pasted-text.txt", maxBytes))
                        return@onPastedText
                    }
                    scope.launch {
                        val attachment =
                            persistPastedTextAttachment(
                                context.cacheDir,
                                text,
                                draft.attachments.map { it.name },
                            )
                        if (attachment == null) {
                            store.showError("Could not attach pasted text.")
                        } else {
                            store.addThreadDraftAttachment(environmentId, threadId, attachment)
                        }
                    }
                },
                hasCompactableConversation = hasCompactableConversation,
                providerAdvisory = providerAdvisory,
                offersUsageLimits = offersUsageLimits,
                onUsageLimits = { usageLimitsOpen = true },
                devicePreviewCount = devicePreviews.size,
                onOpenDevicePreview = { onOpen("devices") },
                onValueChange = { store.setThreadDraft(environmentId, threadId, it) },
                onSend = {
                    // A redirect is still declining; a second tap would decline
                    // and send twice.
                    if (redirecting) return@ThreadComposer
                    val sentDraft = draft
                    val text = draft.text
                    val images = draft.attachments
                    val contextRecords = draft.contextRecords
                    // The command typed in full, no attachments: T3 answers it
                    // locally rather than spending a turn.
                    if (
                        offersUsageLimits &&
                        isUsageLimitsCommand(text) &&
                        images.isEmpty()
                    ) {
                        store.setThreadDraft(environmentId, threadId, "")
                        usageLimitsOpen = true
                        return@ThreadComposer
                    }
                    val declined = if (redirectsApprovals) current.approvals else emptyList()
                    if (declined.isNotEmpty() && respondingApprovalId != null) return@ThreadComposer
                    // RN's `sendApprovalFeedback`: a request that takes feedback gets the
                    // message as its decline reason, delivered to the requesting agent,
                    // and nothing is queued for this thread.
                    val feedbackTarget = approvalFeedbackTarget(declined)
                    if (feedbackTarget != null) {
                        if (images.isNotEmpty() || contextRecords.isNotEmpty()) {
                            store.showError("Send approval feedback without attachments or added context.")
                            return@ThreadComposer
                        }
                        val feedback = text.trim()
                        if (feedback.isEmpty()) return@ThreadComposer
                        redirecting = true
                        scope.launch {
                            try {
                                store.workspace.respondToApproval(env, id, feedbackTarget.id, "decline", feedback)
                                // A draft edited during the reply is not what was sent; it stays.
                                if (store.threadDraft(environmentId, threadId) == sentDraft) {
                                    store.clearThreadDraftContent(environmentId, threadId)
                                }
                            } catch (cancellation: CancellationException) {
                                throw cancellation
                            } catch (error: Exception) {
                                store.showError(error.message ?: "The feedback could not be sent.")
                            } finally {
                                redirecting = false
                            }
                        }
                        return@ThreadComposer
                    }
                    val steers = (working || declined.isNotEmpty()) && canSteer && followUpMode == "steer"
                    if (declined.isNotEmpty()) redirecting = true
                    scope.launch {
                        try {
                            if (declined.isNotEmpty()) {
                                val failure =
                                    declinePendingApprovals(declined) { approvalId ->
                                        store.workspace.respondToApproval(env, id, approvalId, "decline")
                                    }
                                if (failure != null) {
                                    store.showError(failure.message ?: "The approval could not be declined.")
                                    return@launch
                                }
                                if (
                                    !redirectStillSendable(
                                        sent = sentDraft,
                                        current = store.threadDraft(environmentId, threadId),
                                        declined = declined,
                                        pending = latestApprovals,
                                    )
                                ) {
                                    return@launch
                                }
                            }
                            store.enqueueThreadMessage(
                                environmentId = environmentId,
                                threadId = threadId,
                                text = text,
                                attachments = images,
                                settings = effectiveSettings,
                                contextRecords = contextRecords,
                                // The server turns a steered /goal into its own
                                // turn and stops goal work for it; a queued one waits.
                                dispatchMode = if (steers) "steer" else "queue",
                            )
                            following = true
                        } catch (cancellation: CancellationException) {
                            throw cancellation
                        } catch (error: Exception) {
                            store.workspace.setThreadComposerError(
                                "$environmentId:$threadId",
                                error.message
                                    ?: "The message could not be saved to the outbox.",
                            )
                        } finally {
                            if (declined.isNotEmpty()) redirecting = false
                        }
                    }
                },
                onCancel = {
                    scope.launch {
                        runCatching { store.workspace.cancelTurn(env, id) }
                            .onFailure { store.showError(it.message ?: "The turn could not be stopped.") }
                    }
                },
                working = working,
                onResumeQueue =
                    if (current.queueHeld) {
                        {
                            try {
                                store.workspace.queueAction(env, id, "queue.resume")
                            } catch (cancellation: CancellationException) {
                                throw cancellation
                            } catch (error: Exception) {
                                store.showError(error.message ?: "The queue could not be resumed.")
                            }
                        }
                    } else {
                        null
                    },
                attachments = draft.attachments,
                onAddAttachment = pickImages,
                onAddImages = addImages,
                onRemoveAttachment = { attachment ->
                    store.removeThreadDraftImage(environmentId, threadId, attachment.id)
                },
                queuedMessages = current.queuedMessages + queuedMessages.count {
                        it.environmentId == env && it.threadId == id
                    },
                connectionState = environment?.state ?: club.touchtech.s5code.kotlin.model.ConnectionState.Offline,
                provider = effectiveSettings.provider,
                modelLabel =
                    machineCatalog
                        .firstOrNull {
                            it.instance.instanceId == effectiveSettings.provider.instanceId
                        }
                        ?.modelShortLabel(effectiveSettings.model)
                        ?: effectiveSettings.model,
                modelSummary =
                    remember(machineCatalog, effectiveSettings, draft.text) {
                        club.touchtech.s5code.kotlin.feature.settings.composerTraitsSummary(
                            machineCatalog, effectiveSettings, draft.text,
                        )
                    },
                onOpenSettings = { settingsOpen = true },
                // RN gates the Build/Plan control on the planModeEnabled
                // preference in addition to what the provider advertises.
                interactionModeAllowed =
                    preferences.planModeEnabled &&
                        machineCatalog
                            .firstOrNull {
                                it.instance.instanceId == effectiveSettings.provider.instanceId
                            }
                            ?.interactionModeToggle != false,
                onInteractionMode = { mode ->
                    store.setThreadDraftSettings(
                        environmentId,
                        threadId,
                        effectiveSettings.copy(runtimeMode = mode),
                    )
                },
                draftKey = threadId,
            )
            }
            }
            }
        },
        floatingActionButton = {
            // One floating control above the composer, matching the RN client's
            // FloatingWorkingControl: a centered pill that reports connection,
            // syncing, compaction, the working timer, or background work — in
            // that precedence — with a separate scroll-to-end circle alongside.
            // A pending approval or user input hides the status pill so it does
            // not compete with the gate, exactly as RN does.
            val floatingStatus =
                when {
                    wait != null ->
                        FloatingStatus.Connection(wait!!)
                    current.approval != null || current.userInput != null -> null
                    syncPhase != ThreadSyncPhase.Live ->
                        FloatingStatus.Syncing(
                            if (syncPhase == ThreadSyncPhase.Loading) {
                                "Loading messages…"
                            } else {
                                "Syncing messages…"
                            }
                        )
                    isCompacting -> FloatingStatus.Compacting
                    activeWorkStartedAt != null ->
                        FloatingStatus.Working(activeWorkStartedAt)
                    else -> {
                        // RN's order: leftover background work first, then the
                        // thread's lifecycle, then a native /goal as the
                        // idle-thread status.
                        pendingBackgroundWork(current.pendingBackgroundTasks)
                            ?.let { FloatingStatus.Waiting(it.title, it.waiting) }
                            ?: lifecycleFloatingStatus(
                                summary,
                                snoozeSupported = environment?.capabilities?.threadSnooze == true,
                                settlementSupported = environment?.capabilities?.threadSettlement == true,
                            )
                            ?: current.summary.goal?.let {
                                FloatingStatus.Goal(providerGoalPillLabel(it, working = false))
                            }
                    }
                }
            fun runStatusAction(block: suspend () -> Unit) {
                if (statusActionPending) return
                statusActionPending = true
                scope.launch {
                    try {
                        block()
                    } catch (cancelled: kotlinx.coroutines.CancellationException) {
                        throw cancelled
                    } catch (error: Exception) {
                        store.showError(error.message ?: "Could not update thread.")
                    } finally {
                        statusActionPending = false
                    }
                }
            }
            val canOperate = environment?.capabilities?.sessionAllowsOperate == true
            val floatingAction: FloatingStatusAction? =
                when (val status = floatingStatus) {
                    // A provider-managed subagent has no run of its own to stop.
                    is FloatingStatus.Waiting ->
                        if (current.providerNativeSubagent) null
                        else FloatingStatusAction(
                            label = if (statusActionPending) "Stopping…" else "Stop",
                            description = "Stop background work",
                            enabled = canOperate && !statusActionPending,
                        ) { runStatusAction { store.workspace.cancelTurn(env, id) } }
                    is FloatingStatus.Lifecycle ->
                        when (status.state) {
                            ThreadLifecycle.Snoozed ->
                                FloatingStatusAction(
                                    label = if (statusActionPending) "Waking…" else "Wake now",
                                    description = "Wake now",
                                    enabled = canOperate && !statusActionPending,
                                ) {
                                    runStatusAction {
                                        // A usage-limit recovery owns its snooze;
                                        // clearing the thread snooze would leave
                                        // the recovery to re-snooze it.
                                        val runId = current.latestTurn?.turnId
                                        val resetAt = current.usageLimitResetAt
                                        if (current.limitRecoverySnoozed && runId != null && resetAt != null) {
                                            store.workspace.updateLimitRecovery(env, id, runId, resetAt, snooze = false)
                                        } else {
                                            store.workspace.setSnoozed(env, id, false)
                                        }
                                    }
                                }
                            ThreadLifecycle.Settled ->
                                FloatingStatusAction(
                                    label = if (statusActionPending) "Un-settling…" else "Un-settle",
                                    description = "Un-settle",
                                    enabled = canOperate && !statusActionPending,
                                ) { runStatusAction { store.workspace.setSettled(env, id, false) } }
                            ThreadLifecycle.Woke ->
                                FloatingStatusAction(
                                    label = if (statusActionPending) "Dismissing…" else "Dismiss",
                                    description = "Dismiss",
                                    enabled = canOperate && !statusActionPending,
                                ) {
                                    runStatusAction {
                                        store.workspace.visitThread(
                                            env, id, summary.wokeAtMillis ?: System.currentTimeMillis(),
                                        )
                                    }
                                }
                        }
                    else -> null
                }
            val showJump = !following && rows.isNotEmpty()
            Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.large),
                ) {
                    AnimatedVisibility(floatingStatus != null && rows.isNotEmpty()) {
                        floatingStatus?.let { status ->
                            FloatingStatusPill(
                                status = status,
                                onClick =
                                    when (status) {
                                        is FloatingStatus.Connection -> {
                                            { store.retryEnvironment(env) }
                                        }
                                        is FloatingStatus.Waiting -> {
                                            { backgroundWorkOpen = true }
                                        }
                                        else -> null
                                    },
                                action = floatingAction,
                            )
                        }
                    }
                    AnimatedVisibility(showJump) {
                        S5FloatingAction(
                            icon = Icons.Rounded.ArrowDownward,
                            label = "Jump to latest",
                            onClick = { scope.launch { listState.animateScrollToItem(0) } },
                        )
                    }
                }
            }
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            if (summary.lastError != null) {
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.tiny)) {
                    S5Notice(
                        icon = Icons.Rounded.Difference,
                        text = summary.lastError,
                        containerColor = MaterialTheme.colorScheme.errorContainer,
                        contentColor = MaterialTheme.colorScheme.onErrorContainer,
                    )
                }
            }

            attachmentError?.let { error ->
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.tiny)) {
                    S5Notice(
                        icon = Icons.Rounded.BrokenImage,
                        text = error,
                        containerColor = MaterialTheme.colorScheme.errorContainer,
                        contentColor = MaterialTheme.colorScheme.onErrorContainer,
                        onDismiss = store::clearAttachmentError,
                    )
                }
            }

            if (rows.isEmpty()) {
                // Nothing cached: the wait state is the screen. It names the phase and
                // offers a retry, rather than spinning under a bare "Loading".
                S5WaitState(
                    title = wait?.title ?: "Loading transcript",
                    detail = wait?.detail ?: "Reading this thread's history.",
                    icon = health?.icon ?: Icons.Rounded.Difference,
                    spinning = wait?.spinning ?: true,
                    actionLabel = if (wait?.showRetry == true) "Retry now" else null,
                    onAction = { store.retryEnvironment(env) },
                )
            } else {
                LazyColumn(
                    state = listState,
                    reverseLayout = true,
                    modifier = Modifier.fillMaxSize(),
                    contentPadding =
                        PaddingValues(
                            start = S5Theme.spacing.gutter,
                            end = S5Theme.spacing.gutter,
                            top = S5Theme.spacing.large,
                            bottom = S5Theme.spacing.large,
                        ),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    // The approval gate sits at the visual bottom, which is index 0
                    // in a reversed list. A user-input request owns the composer
                    // slot instead of this one.
                    current.approval?.let { approval ->
                        item(key = "approval-${approval.id}") {
                            Box(Modifier.animateItem()) {
                                ApprovalCard(
                                    approval = approval,
                                    submitting = respondingApprovalId == approval.id || redirecting,
                                    onDecision = { decision ->
                                        if (respondingApprovalId == null && !redirecting) {
                                            respondingApprovalId = approval.id
                                            scope.launch {
                                                try {
                                                    store.workspace.respondToApproval(
                                                        env,
                                                        id,
                                                        approval.id,
                                                        decision,
                                                    )
                                                } catch (cancelled: CancellationException) {
                                                    throw cancelled
                                                } catch (error: Exception) {
                                                    store.showError(
                                                        error.message ?: "The approval response could not be sent."
                                                    )
                                                } finally {
                                                    respondingApprovalId = null
                                                }
                                            }
                                        }
                                    },
                                )
                            }
                        }
                    }
                    // The handoff the next send would trigger, at the end of the
                    // feed: just above the approval gate in this reversed list.
                    pendingHandoff?.let { handoff ->
                        item(key = "pending-handoff") {
                            Box(Modifier.animateItem()) {
                                PendingHandoffDivider(
                                    from = handoffEndpointLabel(handoff.from, machineCatalog, providerStatuses),
                                    to = handoffEndpointLabel(handoff.to, machineCatalog, providerStatuses),
                                )
                            }
                        }
                    }
                    items(
                        count = rows.size,
                        key = { index -> rows[rows.lastIndex - index].key },
                        contentType = { index ->
                            when (rows[rows.lastIndex - index]) {
                                is FeedRow.WorkToggle -> "toggle"
                                is FeedRow.TurnFold -> "turn-fold"
                                is FeedRow.Working -> "working"
                                is FeedRow.WorktreeSetup -> "worktree-setup"
                                is FeedRow.Entry -> "entry"
                            }
                        },
                    ) { index ->
                        Box(Modifier.animateItem()) {
                            when (val row = rows[rows.lastIndex - index]) {
                            is FeedRow.Entry ->
                                FeedEntryRow(
                                    entry = row.entry,
                                    mcpAppContent = { app -> McpAppView(store, env, ThreadId(threadId), app,
                                        awaitingUser = current.approval != null || current.userInput != null) },
                                    onCopy = copy,
                                    workspaceRoot = workspaceRoot,
                                    onOpenFile = { path ->
                                        scope.launch {
                                            club.touchtech.s5code.kotlin.data.transcriptFileLinkSuffix(
                                                store.workspace, env, workspaceRoot, path,
                                            )?.let(onOpen)
                                        }
                                    },
                                    environmentId = environmentId,
                                    resolveTranscriptAttachment = { attachmentId ->
                                        runCatching { store.workspace.attachmentUrl(env, attachmentId) }
                                            .onFailure {
                                                store.showError(it.message ?: "The image could not be loaded.")
                                            }
                                            .getOrNull()
                                    },
                                    onOpenAttachment = { attachment ->
                                        onOpen(
                                            "attachments/${android.net.Uri.encode(attachment.id)}" +
                                                "?name=${android.net.Uri.encode(attachment.name)}" +
                                                "&mimeType=${android.net.Uri.encode(attachment.mimeType)}" +
                                                "&sizeBytes=${attachment.sizeBytes}"
                                        )
                                    },
                                    expandedIds = expandedEntries,
                                    onToggleExpand = { entry ->
                                        toggleEntryExpand(entry.id, row.key, row.standIn?.groupId)
                                    },
                                    toolLabelOverride = row.standIn?.label,
                                    diffStat = row.diffStat,
                                    onOpenTurnDiff = { onOpen("review") },
                                    onSavePlan =
                                        workspaceRoot
                                            ?.takeIf { environment?.capabilities?.sessionAllowsFilesystemWrite == true }
                                            ?.let { root ->
                                                { relativePath: String, contents: String ->
                                                    // The screen's scope, not the card's: scrolling the
                                                    // card out of the list must not cancel the write.
                                                    scope.async {
                                                        store.workspace.environmentRequest(
                                                            env,
                                                            "projects.writeFile",
                                                            buildJsonObject {
                                                                put("cwd", root)
                                                                put("relativePath", relativePath)
                                                                put("contents", contents)
                                                            },
                                                        )["relativePath"]?.jsonPrimitive?.contentOrNull ?: relativePath
                                                    }.await()
                                                }
                                            },
                                    onOpenThread = { onOpenThread(environmentId, it) },
                                    senderThreadTitle = { senderId ->
                                        // Read at composition, as the thread links below: collecting
                                        // every shell here would recompose the transcript on each update.
                                        (store.workspace.threads.value + store.workspace.archived.value)
                                            .firstOrNull { it.environmentId.value == environmentId && it.id.value == senderId }?.title
                                    },
                                    scheduledTaskTitles = scheduledTaskTitles,
                                    onOpenAutomation = { onOpenRoute("settings/scheduled-tasks") },
                                    onOpenThreadLink = { href ->
                                        // Read at tap time: collecting every shell here would
                                        // recompose the transcript on each list update.
                                        parseThreadLink(href)?.let { link ->
                                            val refs = (store.workspace.threads.value + store.workspace.archived.value)
                                                .map { ThreadLinkRef(it.environmentId.value, it.id.value) }
                                            val target =
                                                resolveThreadLink(link, environmentId, refs)
                                                    ?: ThreadLinkRef(link.environmentId ?: environmentId, link.threadId)
                                            onOpenThread(target.environmentId, target.threadId)
                                        }
                                    },
                                    onAnswerSecretRequest = { request, answer ->
                                        // The item's own threadId is the
                                        // address: an inherited request is
                                        // answered on the thread that raised it.
                                        store.workspace.answerSecretRequest(
                                            env,
                                            ThreadId(request.threadId),
                                            request.id,
                                            answer,
                                        )
                                    },
                                    fetchToolDetail = { call ->
                                        store.workspace.turnItemDetail(
                                            env,
                                            call.sourceThreadId?.let(::ThreadId) ?: id,
                                            call.id,
                                            call.detailRevision,
                                        )
                                    },
                                    resolveToolOutputImage = { call, index ->
                                        store.workspace.toolOutputImageUrl(env, call.sourceThreadId?.let(::ThreadId) ?: id, call.id, index)
                                    },
                                    commandOutput =
                                        if (environment?.capabilities?.commandOutputStreaming == true) {
                                            { call -> store.workspace.commandOutput(env, call.sourceThreadId ?: id.value, call.id) }
                                        } else {
                                            null
                                        },
                                    onRetryPreparation = { runId ->
                                        runCatching {
                                                store.workspace.retryPreparedRun(env, id, runId)
                                            }
                                            .onFailure {
                                                store.showError(
                                                    it.message
                                                        ?: "The setup could not be retried."
                                                )
                                            }
                                    },
                                    modifier = Modifier.fillMaxWidth(),
                                )
                            is FeedRow.WorkToggle ->
                                WorkGroupToggleRow(
                                    row = row,
                                    onToggle = {
                                        toggleFold(row.groupId, row.key, row.groupId in expandedWorkGroups) {
                                            expandedWorkGroups =
                                                if (row.groupId in expandedWorkGroups) {
                                                    expandedWorkGroups - row.groupId
                                                } else {
                                                    expandedWorkGroups + row.groupId
                                                }
                                        }
                                    },
                                    modifier = Modifier.fillMaxWidth(),
                                    onOpenTurnDiff = { onOpen("review") },
                                )
                            is FeedRow.Working -> WorkingRow(row, Modifier.fillMaxWidth())
                            is FeedRow.TurnFold ->
                                TurnFoldRow(
                                    row = row,
                                    onToggle = {
                                        toggleFold(row.turnId, "turn-fold:${row.turnId}:header", row.turnId in expandedTurns) {
                                            expandedTurns =
                                                if (row.turnId in expandedTurns) expandedTurns - row.turnId
                                                else expandedTurns + row.turnId
                                        }
                                    },
                                    modifier = Modifier.fillMaxWidth(),
                                    onOpenTurnDiff = { onOpen("review") },
                                )
                            is FeedRow.WorktreeSetup -> setupCard?.invoke()
                            }
                        }
                    }
                    // Reversed list: the last index is the visual top, where
                    // "Load earlier turns" sits in the RN feed.
                    val page = current.page
                    if (page?.hasMore == true && page.beforeCursor != null) {
                        item(key = "load-earlier") {
                            Box(
                                Modifier.fillMaxWidth().animateItem(),
                                contentAlignment = Alignment.Center,
                            ) {
                                S5Button(
                                    text =
                                        if (page.loadingOlder) "Loading earlier turns…"
                                        else "Load earlier turns",
                                    onClick = {
                                        scope.launch {
                                            try {
                                                store.workspace.loadOlderTurns(env, id)
                                            } catch (error: CancellationException) {
                                                throw error
                                            } catch (error: Exception) {
                                                store.showError(error.message ?: "Could not load earlier turns.")
                                            }
                                        }
                                    },
                                    emphasis = S5ActionEmphasis.Secondary,
                                    style = S5ButtonStyle.Outlined,
                                    enabled = !page.loadingOlder,
                                )
                            }
                        }
                    }
                    // No prompt row to anchor on (a bootstrap still writing the
                    // thread): the card sits at the very top, above the pager.
                    if (setupAtTop) {
                        item(key = "worktree-setup") {
                            Box(Modifier.fillMaxWidth().animateItem()) {
                                setupCard?.invoke()
                            }
                        }
                    }
                }
            }
        }
    }

    if (backgroundWorkOpen) {
        BackgroundWorkSheet(
            items = pendingBackgroundWorkItems(current.pendingBackgroundTasks),
            onDismiss = { backgroundWorkOpen = false },
            onOpenThread = { childThreadId ->
                backgroundWorkOpen = false
                onOpenThread(environmentId, childThreadId)
            },
        )
    }
    if (settingsOpen) {
        TaskSettingsSheet(
            settings = effectiveSettings,
            catalog = machineCatalog,
            searchScope = if (current.canSwitchProvider) ModelSearchScope.AllProviders else ModelSearchScope.ActiveProvider,
            modelsFor = { provider -> store.modelsFor(provider, env) },
            onSettingsChange = { settings ->
                // Existing-thread settings are composer state in RN. Staging them
                // keeps the chosen model visible immediately and applies it to the
                // next turn atomically instead of waiting for a shell round trip.
                store.setThreadDraftSettings(environmentId, threadId, settings)
            },
            onDismiss = { settingsOpen = false },
            title = "Model and settings",
            favorites = environment?.serverSettings?.favorites?.map { ModelFavorite(it.provider, it.model) }
                ?: preferences.modelFavorites,
            onToggleFavorite = { store.toggleModelFavorite(env, it.instanceId, it.model) },
            onToggleHidden = { model, hidden -> store.setModelHidden(env, model, hidden) },
            onUpdateProvider = { store.updateCatalogProvider(env, it) },
            canUpdateProviders = environment?.capabilities?.sessionCanManageProviders == true,
            catalogRefreshing = env.value in catalogRefreshing,
            onRefreshCatalog = { store.refreshProviderCatalog(env) },
            planModeEnabled = preferences.planModeEnabled,
            prompt = draft.text,
            onPromptChange = { store.setThreadDraft(environmentId, threadId, it) },
        )
    }
}

/**
 * The collapsed form of the pending questionnaire — a composer-sized bar in the
 * composer's place, matching the RN card's collapsed state. Tapping expands
 * back to the full card; while a turn is running it also carries the stop
 * control the composer would otherwise own.
 */
@Composable
private fun UserInputCollapsedBar(
    questionCount: Int,
    working: Boolean,
    onExpand: () -> Unit,
    onStop: () -> Unit,
) {
    Surface(
        onClick = onExpand,
        shape = androidx.compose.foundation.shape.CircleShape,
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        modifier =
            Modifier.fillMaxWidth()
                .padding(
                    horizontal = S5Theme.spacing.medium,
                    vertical = S5Theme.spacing.small,
                ),
    ) {
        Row(
            Modifier.padding(start = S5Theme.spacing.medium, end = S5Theme.spacing.tiny),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Text(
                "User input needed",
                style = MaterialTheme.typography.labelMediumEmphasized,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                "$questionCount question${if (questionCount == 1) "" else "s"}",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.weight(1f),
            )
            if (working) {
                S5IconButton(
                    icon = Icons.Rounded.Stop,
                    label = "Stop the agent",
                    onClick = onStop,
                )
            }
            Icon(
                Icons.Rounded.KeyboardArrowUp,
                contentDescription = "Expand user input",
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/**
 * How far the header must collapse before the plan strip appears. Not zero: the
 * strip would then flicker on the first pixel of scroll while the bar settles.
 * Not one either, since the last few pixels of the collapse animation would
 * delay the strip past the moment the title has already gone.
 */
private const val PLAN_BAR_COLLAPSE_THRESHOLD = 0.6f

/** The collapsed top bar plus a gap, kept clear of the docked composer stack. */
private val BOTTOM_BAR_TOP_RESERVE = 88.dp

/**
 * What the floating pill above the composer says — the Kotlin port of
 * `FloatingWorkingStatus` in `apps/mobile/src/features/threads/
 * floating-working-status.ts`. Connection, syncing, compacting, working, and
 * waiting share one element so the label swaps in place; only the connection
 * variant is tappable and triggers a reconnect.
 */
private sealed interface FloatingStatus {
    val label: String

    data class Connection(val notice: WaitNotice) : FloatingStatus {
        override val label get() = waitPillLabel(notice)
    }

    data class Syncing(override val label: String) : FloatingStatus
    data object Compacting : FloatingStatus {
        override val label get() = "Compacting…"
    }
    data class Working(val startedAtMillis: Long) : FloatingStatus {
        // The pill renders `floatingWorkingLabel` instead — it needs the tick.
        override val label get() = "Working"
    }
    /**
     * The turn settled while work it started still runs. [waiting] is false
     * when only commands remain, such as a dev server: the agent is done, so
     * the pill drops the bolt for the terminal glyph.
     */
    data class Waiting(override val label: String, val waiting: Boolean) : FloatingStatus
    /** A native /goal on an idle thread: paused, blocked, complete, or set. */
    data class Goal(override val label: String) : FloatingStatus
    /** An idle thread that is snoozed, settled, or freshly woken, with its way back out. */
    data class Lifecycle(val state: ThreadLifecycle, override val label: String) : FloatingStatus
}

private enum class ThreadLifecycle { Snoozed, Settled, Woke }

/** The pill's trailing command: Stop, Wake now, Un-settle, or Dismiss. */
private class FloatingStatusAction(
    val label: String,
    val description: String,
    val enabled: Boolean,
    val onClick: () -> Unit,
)

/**
 * The lifecycle the idle pill reports, following `ThreadDetailScreen`'s
 * lifecycle status. Each state is gated on the capability its action writes,
 * since a command the server does not know kills the socket. A wake counts
 * only while nothing has happened since — a run that finished after it means
 * the user already watched the thread come back.
 */
private fun lifecycleFloatingStatus(
    summary: ThreadSummary,
    snoozeSupported: Boolean,
    settlementSupported: Boolean,
): FloatingStatus.Lifecycle? {
    val wokeAt = summary.wokeAtMillis
    return when {
        snoozeSupported && summary.status == ThreadStatus.Snoozed ->
            FloatingStatus.Lifecycle(
                ThreadLifecycle.Snoozed,
                summary.snoozedUntilLabel?.let { "Snoozed until $it" } ?: "Snoozed",
            )
        settlementSupported && summary.status == ThreadStatus.Settled ->
            FloatingStatus.Lifecycle(ThreadLifecycle.Settled, "Settled")
        snoozeSupported && wokeAt != null && (summary.latestCompletedAtMillis ?: Long.MIN_VALUE) < wokeAt ->
            FloatingStatus.Lifecycle(ThreadLifecycle.Woke, "Woke from snooze")
        else -> null
    }
}

/**
 * What a settled turn left running, one row per task, from the background pill.
 * A subagent with its own thread opens it; the rest are facts.
 */
@Composable
private fun BackgroundWorkSheet(
    items: List<PendingBackgroundWorkItem>,
    onDismiss: () -> Unit,
    onOpenThread: (String) -> Unit,
) {
    S5BottomSheet(onDismiss = onDismiss, title = "Background work") {
        if (items.isEmpty()) {
            Text(
                "Nothing running in the background.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.fillMaxWidth().padding(S5Theme.spacing.gutter),
            )
        } else {
            Column(
                Modifier.padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.small),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                items.forEachIndexed { index, item ->
                    val childThreadId = item.childThreadId
                    club.touchtech.s5code.kotlin.design.component.S5SettingsRow(
                        icon =
                            when (item.kind) {
                                "subagent" -> Icons.Rounded.Groups
                                "monitor" -> Icons.Rounded.Schedule
                                else -> Icons.Rounded.Terminal
                            },
                        label = item.label,
                        supporting = item.kindLabel,
                        onClick = childThreadId?.let { { onOpenThread(it) } },
                        position = club.touchtech.s5code.kotlin.design.component.rowPosition(index, items.size),
                    )
                }
            }
        }
    }
}

/**
 * The centered pill from the RN `FloatingWorkingControl`. Syncing and
 * compacting spin; a dead connection shows the red dot instead of a spinner,
 * since it is not making progress; the working kind ticks its own duration.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun FloatingStatusPill(
    status: FloatingStatus,
    /** Reconnect for a dead connection, the work list for background work. */
    onClick: (() -> Unit)?,
    action: FloatingStatusAction?,
    modifier: Modifier = Modifier,
) {
    Surface(
        onClick = onClick ?: {},
        enabled = onClick != null,
        shape = androidx.compose.foundation.shape.CircleShape,
        color = MaterialTheme.colorScheme.surfaceContainerHighest,
        contentColor = MaterialTheme.colorScheme.onSurface,
        tonalElevation = 3.dp,
        shadowElevation = 2.dp,
        modifier = modifier,
    ) {
        Row(
            Modifier.padding(horizontal = S5Theme.spacing.medium, vertical = S5Theme.spacing.small),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            val spinning =
                when (status) {
                    is FloatingStatus.Connection -> status.notice.spinning
                    is FloatingStatus.Syncing,
                    FloatingStatus.Compacting -> true
                    else -> false
                }
            when {
                spinning ->
                    S5InlineLoading(Modifier.size(16.dp))
                status is FloatingStatus.Connection ->
                    // The dead-connection dot, same as S5WaitPill's static
                    // affordance: no spinner where nothing is retrying.
                    Box(
                        Modifier.size(8.dp)
                            .background(
                                MaterialTheme.colorScheme.error,
                                androidx.compose.foundation.shape.CircleShape,
                            ),
                    )
                // A dev server can run for hours after the agent is done, so
                // only work that will wake the agent gets the bolt (RN's same
                // split between `bolt` and `terminal` glyphs).
                status is FloatingStatus.Waiting ->
                    Icon(
                        if (status.waiting) Icons.Rounded.Bolt else Icons.Rounded.Terminal,
                        contentDescription = null,
                        modifier = Modifier.size(13.dp),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                status is FloatingStatus.Goal ->
                    Icon(
                        Icons.Rounded.TrackChanges,
                        contentDescription = null,
                        modifier = Modifier.size(13.dp),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                status is FloatingStatus.Lifecycle ->
                    Icon(
                        if (status.state == ThreadLifecycle.Settled) Icons.Rounded.CheckCircle
                        else Icons.Rounded.Schedule,
                        contentDescription = null,
                        modifier = Modifier.size(13.dp),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
            }
            if (status is FloatingStatus.Working) {
                var nowMillis by
                    remember(status.startedAtMillis) {
                        mutableLongStateOf(System.currentTimeMillis())
                    }
                LaunchedEffect(status.startedAtMillis) {
                    while (true) {
                        delay(1_000)
                        nowMillis = System.currentTimeMillis()
                    }
                }
                Text(
                    floatingWorkingLabel(status.startedAtMillis, nowMillis),
                    style = MaterialTheme.typography.labelLargeEmphasized,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.widthIn(max = 260.dp),
                )
            } else {
                Text(
                    status.label,
                    style = MaterialTheme.typography.labelLargeEmphasized,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.widthIn(max = 220.dp),
                )
            }
            if (action != null) {
                Box(
                    Modifier.size(width = 1.dp, height = 16.dp)
                        .background(MaterialTheme.colorScheme.outlineVariant),
                )
                Text(
                    action.label,
                    style = MaterialTheme.typography.labelLargeEmphasized,
                    color =
                        if (action.enabled) MaterialTheme.colorScheme.primary
                        else MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    modifier =
                        Modifier.clip(androidx.compose.foundation.shape.CircleShape)
                            .clickable(
                                enabled = action.enabled,
                                onClickLabel = action.description,
                                role = androidx.compose.ui.semantics.Role.Button,
                                onClick = action.onClick,
                            )
                            .padding(horizontal = S5Theme.spacing.small, vertical = S5Theme.spacing.tiny),
                )
            }
        }
    }
}

/**
 * A handoff side's label: the model's display name, else the instance's, else its
 * id. Same-model handoffs between accounts would read "GPT-5 → GPT-5", so a side
 * whose instance needs telling apart is prefixed with its name the way the
 * desktop tooltip formats it ("Work · GPT-5").
 */
private fun handoffEndpointLabel(
    endpoint: HandoffEndpoint,
    catalog: List<ProviderCatalogEntry>,
    statuses: List<ProviderStatus>,
): String {
    val entry = catalog.firstOrNull { it.instance.instanceId == endpoint.instanceId }
    val label = endpoint.model?.let { model -> entry?.modelShortLabel(model) ?: model }
        ?: entry?.instance?.label
        ?: endpoint.instanceId
    val status = statuses.firstOrNull { it.instanceId == endpoint.instanceId }
    return if (status != null && showsInstanceBadge(status, statuses)) {
        "${providerInstanceDisplayName(status)} · $label"
    } else {
        label
    }
}

/**
 * `shouldShowInstanceBadge`: an accent colour marks an instance the user styled
 * apart; otherwise only a second instance on the same driver needs naming. The
 * ACP registry's agent id is not on [ProviderStatus], so registry instances all
 * count as one driver here and may be named when they need not be.
 */
private fun showsInstanceBadge(status: ProviderStatus, statuses: List<ProviderStatus>): Boolean {
    if (status.accentColor?.trim()?.matches(Regex("^#[0-9a-fA-F]{6}$")) == true) return true
    // Historical threads still need to distinguish accounts that are all disabled.
    return statuses.count { it.driver == status.driver && (!status.enabled || it.enabled) } > 1
}

/** `resolveProviderInstanceDisplayName`: the user's name, else a non-default id humanized. */
private fun providerInstanceDisplayName(status: ProviderStatus): String {
    val name = status.displayName?.trim().orEmpty()
    val kindLabel = formatProviderDriverName(status.driver)
    if (name.isNotEmpty() && name != kindLabel) return name
    if (status.instanceId != status.driver) {
        val humanized = status.instanceId
            .replace(Regex("([a-z])([A-Z])"), "$1 $2")
            .replace(Regex("[_-]+"), " ")
            .trim()
            .replace(Regex("\\b\\w")) { it.value.uppercase() }
        if (humanized.isNotEmpty()) return humanized
    }
    return name.ifEmpty { kindLabel }
}

/**
 * "Context handoff pending" at the end of the feed: sending now moves the
 * conversation to another provider session. Ported from `ThreadHandoffDivider`.
 */
@Composable
private fun PendingHandoffDivider(from: String, to: String) {
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
    Column(
        Modifier.fillMaxWidth().padding(vertical = S5Theme.spacing.small)
            .semantics(mergeDescendants = true) {},
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            HorizontalDivider(Modifier.weight(1f))
            Icon(
                Icons.Rounded.SwapHoriz,
                contentDescription = null,
                modifier = Modifier.size(14.dp),
                tint = muted,
            )
            Text("Context handoff pending", style = MaterialTheme.typography.labelSmall, color = muted)
            HorizontalDivider(Modifier.weight(1f))
        }
        Text(
            "$from → $to",
            style = MaterialTheme.typography.labelSmall,
            color = muted,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}
