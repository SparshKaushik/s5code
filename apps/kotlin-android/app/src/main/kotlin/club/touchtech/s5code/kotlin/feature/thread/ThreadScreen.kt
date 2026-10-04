package club.touchtech.s5code.kotlin.feature.thread

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.ArrowDownward
import androidx.compose.material.icons.rounded.BrokenImage
import androidx.compose.material.icons.rounded.Difference
import androidx.compose.material.icons.rounded.Folder
import androidx.compose.material.icons.rounded.History
import androidx.compose.material.icons.rounded.KeyboardArrowUp
import androidx.compose.material.icons.rounded.Source
import androidx.compose.material.icons.rounded.Stop
import androidx.compose.material.icons.rounded.Terminal
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.app.ThreadDraft
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
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.S5WaitPill
import club.touchtech.s5code.kotlin.design.component.S5WaitState
import club.touchtech.s5code.kotlin.design.component.ScrollAnchor
import club.touchtech.s5code.kotlin.design.component.scrollAnchor
import club.touchtech.s5code.kotlin.design.component.scrollToAnchor
import club.touchtech.s5code.kotlin.design.component.rememberClipboardWriter
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.feature.connections.connectionPresentation
import club.touchtech.s5code.kotlin.feature.connections.showRetry
import club.touchtech.s5code.kotlin.feature.connections.waitNotice
import club.touchtech.s5code.kotlin.feature.connections.waitPillLabel
import club.touchtech.s5code.kotlin.feature.settings.TaskSettingsSheet
import club.touchtech.s5code.kotlin.feature.usage.ComposerUsageLimitsCard
import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.FeedEntry
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadStatus
import club.touchtech.s5code.kotlin.model.ThreadSyncPhase
import club.touchtech.s5code.kotlin.platform.rememberComposerImageIntake
import club.touchtech.s5code.kotlin.platform.rememberComposerImagePicker
import club.touchtech.s5code.kotlin.platform.rememberQuestionFileIntake
import club.touchtech.s5code.kotlin.platform.rememberQuestionFilePicker
import club.touchtech.s5code.kotlin.platform.rememberQuestionMediaPicker
import kotlinx.coroutines.flow.distinctUntilChanged
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
) {
    val id = remember(threadId) { ThreadId(threadId) }
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val context = androidx.compose.ui.platform.LocalContext.current
    // Subscribing is what starts this thread's stream, so it is keyed by both ids:
    // thread ids are only unique within one environment.
    val detail by
        remember(environmentId, threadId) { store.workspace.thread(env, id) }
            .collectAsStateWithLifecycle()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
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
    val machineCatalog =
        remember(env, providerCatalogs, providerCatalog) {
            providerCatalogs[env]?.takeIf { it.isNotEmpty() } ?: providerCatalog
        }
    val attachmentError by store.attachmentError.collectAsStateWithLifecycle()
    val stagedQuestionAttachments by store.questionAttachments.collectAsStateWithLifecycle()
    val preferences by store.preferences.collectAsStateWithLifecycle()
    val catalogRefreshing by store.catalogRefreshing.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val copy = rememberClipboardWriter()

    var settingsOpen by remember(threadId) { mutableStateOf(false) }
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
    val working = summary.status == ThreadStatus.Working
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
    var expandedEntries by remember(threadId) { mutableStateOf(emptySet<String>()) }
    fun toggleEntryExpand(entryId: String) {
        toggleFold("entry:$entryId", entryId, entryId in expandedEntries) {
            expandedEntries =
                if (entryId in expandedEntries) expandedEntries - entryId
                else expandedEntries + entryId
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
    val rows =
        remember(
            current.feed,
            expandedWorkGroups,
            expandedTurns,
            current.latestTurn,
            activeWorkStartedAt,
            worktreeSetup,
        ) {
            val presented =
                presentFeed(
                    feed = current.feed,
                    expandedGroups = expandedWorkGroups,
                    latestTurn = current.latestTurn,
                    expandedTurns = expandedTurns,
                    activeWorkStartedAtMillis = activeWorkStartedAt,
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
                        planBarApplies(summary.status),
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
        },
        bottomBar = {
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
                        Modifier.fillMaxWidth()
                            .padding(
                                horizontal = S5Theme.spacing.gutter,
                                vertical = S5Theme.spacing.tiny,
                            ),
                )
            }
            V2ThreadControls(store, env, id, current, working, followUp,
                onFollowUp = { followUp = it }, onOpenThread = { onOpenThread(environmentId, it) })
            if (current.providerNativeSubagent) {
                Text("Provider-managed subagent · ${summary.status}", modifier = Modifier.padding(16.dp),
                    style = MaterialTheme.typography.bodySmall)
            } else ThreadComposer(
                sendLabel = if (working) { if (current.canSteer && followUp == "steer") "Steer" else "Queue" } else "Send",
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
                            )
                        }
                    },
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
                    scope.launch {
                        try {
                            store.enqueueThreadMessage(
                                environmentId = environmentId,
                                threadId = threadId,
                                text = text,
                                attachments = images,
                                settings = effectiveSettings,
                                contextRecords = contextRecords,
                                dispatchMode = if (working && current.canSteer && followUp == "steer") "steer" else "queue",
                            )
                            following = true
                        } catch (error: Exception) {
                            store.showError(error.message ?: "The message could not be saved to the outbox.")
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
                connectionError = environment?.lastError?.takeIf { it.isNotBlank() },
                environmentLabel = environment?.label ?: "Environment",
                syncPhase = syncPhase,
                onReconnect = { store.retryEnvironment(env) },
                provider = effectiveSettings.provider,
                modelLabel =
                    machineCatalog
                        .firstOrNull {
                            it.instance.instanceId == effectiveSettings.provider.instanceId
                        }
                        ?.modelLabel(effectiveSettings.model)
                        ?: effectiveSettings.model,
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
        },
        floatingActionButton = {
            // The pill and the jump button share this slot: both belong just above the
            // composer, and only one of them is ever worth showing at a time. The pill
            // wins, since a connection that is not live makes "jump to latest"
            // meaningless.
            Column(
                horizontalAlignment = Alignment.End,
                verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                AnimatedVisibility(wait != null && rows.isNotEmpty()) {
                    wait?.let { notice ->
                        S5WaitPill(
                            label = waitPillLabel(notice),
                            spinning = notice.spinning,
                            onClick = { store.retryEnvironment(env) },
                        )
                    }
                }
                AnimatedVisibility(!following && rows.isNotEmpty()) {
                    S5FloatingAction(
                        icon = Icons.Rounded.ArrowDownward,
                        label = "Jump to latest",
                        onClick = { scope.launch { listState.animateScrollToItem(0) } },
                    )
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
                                var submitting by remember(approval.id) { mutableStateOf(false) }
                                ApprovalCard(
                                    approval = approval,
                                    submitting = submitting,
                                    onDecision = { decision ->
                                        if (!submitting) {
                                            submitting = true
                                            scope.launch {
                                                try {
                                                    store.workspace.respondToApproval(
                                                        env,
                                                        id,
                                                        approval.id,
                                                        decision,
                                                    )
                                                } catch (error: Exception) {
                                                    store.showError(
                                                        error.message ?: "The approval response could not be sent."
                                                    )
                                                } finally {
                                                    submitting = false
                                                }
                                            }
                                        }
                                    },
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
                                    onCopy = copy,
                                    workspaceRoot = workspaceRoot,
                                    onOpenFile = { path ->
                                        onOpen("${club.touchtech.s5code.kotlin.app.Routes.fileRouteSuffix(path)}?path=${android.net.Uri.encode(path)}")
                                    },
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
                                    onToggleExpand = { entry -> toggleEntryExpand(entry.id) },
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
                                            store.workspace.loadOlderTurns(env, id)
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

    if (settingsOpen) {
        TaskSettingsSheet(
            settings = effectiveSettings,
            catalog = machineCatalog,
            modelsFor = { provider -> store.modelsFor(provider, env) },
            onSettingsChange = { settings ->
                // Existing-thread settings are composer state in RN. Staging them
                // keeps the chosen model visible immediately and applies it to the
                // next turn atomically instead of waiting for a shell round trip.
                store.setThreadDraftSettings(environmentId, threadId, settings)
            },
            onDismiss = { settingsOpen = false },
            title = "Model and settings",
            favorites = preferences.modelFavorites,
            onToggleFavorite = { store.toggleModelFavorite(it.instanceId, it.model) },
            catalogRefreshing = env.value in catalogRefreshing,
            onRefreshCatalog = { store.refreshProviderCatalog(env) },
            planModeEnabled = preferences.planModeEnabled,
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
