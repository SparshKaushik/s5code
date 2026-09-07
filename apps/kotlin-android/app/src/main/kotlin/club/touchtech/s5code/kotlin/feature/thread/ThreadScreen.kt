package club.touchtech.s5code.kotlin.feature.thread

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.ArrowDownward
import androidx.compose.material.icons.rounded.BrokenImage
import androidx.compose.material.icons.rounded.Difference
import androidx.compose.material.icons.rounded.Folder
import androidx.compose.material.icons.rounded.History
import androidx.compose.material.icons.rounded.Source
import androidx.compose.material.icons.rounded.Terminal
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.app.ThreadDraft
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5FloatingAction
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.S5WaitPill
import club.touchtech.s5code.kotlin.design.component.S5WaitState
import club.touchtech.s5code.kotlin.design.component.rememberClipboardWriter
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.feature.connections.connectionPresentation
import club.touchtech.s5code.kotlin.feature.connections.waitNotice
import club.touchtech.s5code.kotlin.feature.connections.waitPillLabel
import club.touchtech.s5code.kotlin.feature.settings.TaskSettingsSheet
import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadStatus
import club.touchtech.s5code.kotlin.model.ThreadSyncPhase
import club.touchtech.s5code.kotlin.platform.rememberComposerImageIntake
import club.touchtech.s5code.kotlin.platform.rememberComposerImagePicker
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
) {
    val id = remember(threadId) { ThreadId(threadId) }
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    // Subscribing is what starts this thread's stream, so it is keyed by both ids:
    // thread ids are only unique within one environment.
    val detail by
        remember(environmentId, threadId) { store.workspace.thread(env, id) }
            .collectAsStateWithLifecycle()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val syncPhase by
        remember(environmentId, threadId) { store.workspace.threadSyncPhase(env, id) }
            .collectAsStateWithLifecycle()
    val threadDrafts by store.threadDrafts.collectAsStateWithLifecycle()
    val queuedMessages by store.outbox.collectAsStateWithLifecycle()
    val providerCatalog by store.workspace.providerCatalog.collectAsStateWithLifecycle()
    val attachmentError by store.attachmentError.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val copy = rememberClipboardWriter()

    var settingsOpen by remember(threadId) { mutableStateOf(false) }
    var following by remember(threadId) { mutableStateOf(true) }
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
                )
        S5Screen(title = "Thread", onBack = onBack) { padding ->
            if (opening != null) {
                S5WaitState(
                    title = opening.title,
                    detail = opening.detail,
                    icon = health?.icon ?: Icons.Rounded.Difference,
                    spinning = opening.spinning,
                    actionLabel = if (opening.spinning) null else "Retry now",
                    onAction = { store.retryEnvironment(env) },
                    modifier = Modifier.padding(padding),
                )
            } else {
                S5EmptyState(
                    icon = Icons.Rounded.Difference,
                    title = "Thread not available",
                    detail = "It may have been deleted, or this environment is no longer paired.",
                    actionLabel = "Back to home",
                    onAction = onBack,
                    modifier = Modifier.padding(padding),
                )
            }
        }
        return
    }

    val summary = current.summary
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val workspaceRoot =
        remember(projects, summary.projectId, environmentId, current) {
            current.workspaceRoot
                ?: projects.firstOrNull {
                    it.environmentId.value == environmentId && it.id == summary.projectId
                }?.workspaceRoot
        }
    val effectiveSettings = draft.settings ?: current.settings
    val working = summary.status == ThreadStatus.Working
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
        remember(current.feed, expandedWorkGroups, expandedTurns, current.latestTurn, activeWorkStartedAt) {
            presentFeed(
                feed = current.feed,
                expandedGroups = expandedWorkGroups,
                latestTurn = current.latestTurn,
                expandedTurns = expandedTurns,
                activeWorkStartedAtMillis = activeWorkStartedAt,
            )
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
        remember(environment?.state, environment?.label, rows.isEmpty()) {
            waitNotice(
                states = listOfNotNull(environment?.state),
                environmentLabel = environment?.label,
                resourceName = "transcript",
                hasContent = rows.isNotEmpty(),
            )
        }

    S5Screen(
        title = summary.title,
        subtitle =
            listOfNotNull(summary.branch, summary.provider.label, summary.model).joinToString(" · "),
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
            ThreadComposer(
                value = draft.text,
                commands = remember(summary.provider) { store.workspace.slashCommands(summary.provider) },
                onSearchPaths = { query -> store.workspace.searchPaths(env, id, query) },
                onValueChange = { store.setThreadDraft(environmentId, threadId, it) },
                onSend = {
                    val text = draft.text
                    val images = draft.attachments
                    scope.launch {
                        try {
                            store.enqueueThreadMessage(
                                environmentId = environmentId,
                                threadId = threadId,
                                text = text,
                                attachments = images,
                                settings = effectiveSettings,
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
                queuedMessages =
                    queuedMessages.count {
                        it.environmentId == env && it.threadId == id
                    },
                connectionState = environment?.state ?: club.touchtech.s5code.kotlin.model.ConnectionState.Offline,
                connectionError = environment?.lastSeenLabel?.takeIf { it.isNotBlank() },
                environmentLabel = environment?.label ?: "Environment",
                syncPhase = syncPhase,
                onReconnect = { store.retryEnvironment(env) },
                provider = effectiveSettings.provider,
                model = effectiveSettings.model,
                onOpenSettings = { settingsOpen = true },
                draftKey = threadId,
            )
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
                    actionLabel = if (wait?.spinning == false) "Retry now" else null,
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
                    // Gates sit at the visual bottom, which is index 0 in a
                    // reversed list.
                    current.userInput?.let { request ->
                        item(key = "input-${request.id}") {
                            Box(Modifier.animateItem()) {
                                var submitting by remember(request.id) { mutableStateOf(false) }
                                UserInputCard(
                                    request = request,
                                    submitting = submitting,
                                    onSubmit = { answers ->
                                        if (!submitting) {
                                            submitting = true
                                            scope.launch {
                                                try {
                                                    store.workspace.respondToInput(
                                                        env,
                                                        id,
                                                        request.id,
                                                        answers,
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
                                )
                            }
                        }
                    }
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
                                    modifier = Modifier.fillMaxWidth(),
                                )
                            is FeedRow.WorkToggle ->
                                WorkGroupToggleRow(
                                    row = row,
                                    onToggle = {
                                        expandedWorkGroups =
                                            if (row.groupId in expandedWorkGroups) {
                                                expandedWorkGroups - row.groupId
                                            } else {
                                                expandedWorkGroups + row.groupId
                                            }
                                    },
                                    modifier = Modifier.fillMaxWidth(),
                                )
                            is FeedRow.Working -> WorkingRow(row, Modifier.fillMaxWidth())
                            is FeedRow.TurnFold ->
                                TurnFoldRow(
                                    row = row,
                                    onToggle = {
                                        expandedTurns =
                                            if (row.turnId in expandedTurns) expandedTurns - row.turnId
                                            else expandedTurns + row.turnId
                                    },
                                    modifier = Modifier.fillMaxWidth(),
                                )
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
            catalog = providerCatalog,
            modelsFor = store::modelsFor,
            onSettingsChange = { settings ->
                // Existing-thread settings are composer state in RN. Staging them
                // keeps the chosen model visible immediately and applies it to the
                // next turn atomically instead of waiting for a shell round trip.
                store.setThreadDraftSettings(environmentId, threadId, settings)
            },
            onDismiss = { settingsOpen = false },
            title = "Model and settings",
        )
    }
}

/**
 * How far the header must collapse before the plan strip appears. Not zero: the
 * strip would then flicker on the first pixel of scroll while the bar settles.
 * Not one either, since the last few pixels of the collapse animation would
 * delay the strip past the moment the title has already gone.
 */
private const val PLAN_BAR_COLLAPSE_THRESHOLD = 0.6f
