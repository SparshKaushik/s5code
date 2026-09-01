package club.touchtech.s5code.kotlin.feature.home

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Article
import androidx.compose.material.icons.automirrored.rounded.Sort
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Archive
import androidx.compose.material.icons.rounded.Bedtime
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Key
import androidx.compose.material.icons.rounded.DoneAll
import androidx.compose.material.icons.rounded.EditNote
import androidx.compose.material.icons.rounded.ExpandLess
import androidx.compose.material.icons.rounded.ExpandMore
import androidx.compose.material.icons.rounded.Hub
import androidx.compose.material.icons.rounded.Inbox
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.PushPin
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material.icons.rounded.Settings
import androidx.compose.material.icons.rounded.Warning
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5ConnectedButtonGroup
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogRequest
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5WaitState
import club.touchtech.s5code.kotlin.design.component.S5HeroFab
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5LoadingStrip
import club.touchtech.s5code.kotlin.design.component.S5MenuOption
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5OverflowMenu
import club.touchtech.s5code.kotlin.design.component.S5ProjectIcon
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SearchField
import club.touchtech.s5code.kotlin.design.component.S5SectionHeader
import club.touchtech.s5code.kotlin.design.component.S5SkeletonRow
import club.touchtech.s5code.kotlin.design.component.S5SwipeAction
import club.touchtech.s5code.kotlin.design.component.S5SwipeableRow
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.S5WaitPill
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.feature.connections.WaitPhase
import club.touchtech.s5code.kotlin.feature.connections.connectionPresentation
import club.touchtech.s5code.kotlin.feature.connections.waitNotice
import club.touchtech.s5code.kotlin.feature.connections.waitPillLabel
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.HomeListItem
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.ShelfKind
import club.touchtech.s5code.kotlin.model.ThreadFilter
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadSort
import club.touchtech.s5code.kotlin.model.ThreadStatus
import club.touchtech.s5code.kotlin.model.ThreadSummary
import club.touchtech.s5code.kotlin.model.ThreadSearchMatch
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * Merged workspace home. One list across every environment, with search,
 * filters, project grouping, collapsible snoozed/settled shelves, and per-thread
 * quick actions. The hero action is the new-task FAB.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun HomeScreen(
    store: AppStore,
    onOpenThread: (String, String) -> Unit,
    onNewTask: () -> Unit,
    /**
     * Reopens the unsent new-task draft. Separate from [onNewTask] because it skips
     * the project step: the draft already has a project, and asking again is what
     * made the draft feel lost in the first place.
     */
    onResumeDraft: () -> Unit,
    onSettings: () -> Unit,
    onConnections: () -> Unit,
    onArchive: () -> Unit,
    confirmController: S5ConfirmDialogController,
    hardwareShortcut: club.touchtech.s5code.kotlin.app.S5HardwareShortcutEvent? = null,
    onEscape: () -> Unit = {},
) {
    val scope = rememberCoroutineScope()
    val focusManager = LocalFocusManager.current
    val keyboardController = LocalSoftwareKeyboardController.current
    val searchFocusRequester = remember { FocusRequester() }
    val threads by store.workspace.threads.collectAsStateWithLifecycle()
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val home by store.home.collectAsStateWithLifecycle()
    val preferences by store.preferences.collectAsStateWithLifecycle()
    val threadDrafts by store.threadDrafts.collectAsStateWithLifecycle()
    val newTaskDraft by store.draft.collectAsStateWithLifecycle()
    val pendingRequests by store.workspace.pendingRequests.collectAsStateWithLifecycle()
    val paired by store.paired.collectAsStateWithLifecycle()
    val sessionRestored by store.sessionRestored.collectAsStateWithLifecycle()

    var searchVisible by rememberSaveable { mutableStateOf(false) }
    var sortMenuOpen by remember { mutableStateOf(false) }
    var rowMenuFor by remember { mutableStateOf<String?>(null) }
    var submittingRequestKey by remember { mutableStateOf<String?>(null) }
    var searchMatches by remember { mutableStateOf(emptyList<ThreadSearchMatch>()) }
    var searchLoading by remember { mutableStateOf(false) }

    LaunchedEffect(hardwareShortcut?.id) {
        when (hardwareShortcut?.shortcut) {
            club.touchtech.s5code.kotlin.app.S5HardwareShortcut.NewTask -> Unit
            club.touchtech.s5code.kotlin.app.S5HardwareShortcut.FocusSearch -> {
                searchVisible = true
                withFrameNanos { }
                searchFocusRequester.requestFocus()
                keyboardController?.show()
            }
            club.touchtech.s5code.kotlin.app.S5HardwareShortcut.Escape -> {
                if (searchVisible) {
                    searchVisible = false
                    store.updateHome { it.copy(query = "") }
                    focusManager.clearFocus(force = true)
                    keyboardController?.hide()
                } else {
                    onEscape()
                }
            }
            null -> Unit
        }
    }

    BackHandler(enabled = searchVisible) {
        searchVisible = false
        store.updateHome { it.copy(query = "") }
        focusManager.clearFocus(force = true)
        keyboardController?.hide()
    }

    val searchEnvironmentIds =
        remember(environments, home.environmentId) {
            environments
                .filter { environment ->
                    environment.state == club.touchtech.s5code.kotlin.model.ConnectionState.Connected &&
                        (home.environmentId == null || environment.id == home.environmentId)
                }
                .map { it.id }
                .toSet()
        }

    LaunchedEffect(home.query, searchEnvironmentIds) {
        val query = home.query.trim()
        if (query.length < 2 || searchEnvironmentIds.isEmpty()) {
            searchMatches = emptyList()
            searchLoading = false
            return@LaunchedEffect
        }
        searchLoading = true
        delay(SEARCH_DEBOUNCE_MILLIS)
        try {
            searchMatches = store.workspace.searchThreads(searchEnvironmentIds, query)
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (error: Exception) {
            searchMatches = emptyList()
            store.showError(error.message ?: "Message search could not be completed.")
        } finally {
            searchLoading = false
        }
    }

    fun reportError(message: String) = store.showError(message)

    fun launchThreadAction(thread: ThreadSummary, action: String) {
        scope.launch {
            try {
                performThreadAction(
                    store = store,
                    environmentId = thread.environmentId,
                    id = thread.id,
                    action = action,
                    pinned = thread.pinned,
                    status = thread.status,
                )
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (error: Exception) {
                reportError(error.message ?: "The thread action could not be completed.")
            }
        }
    }

    val items =
        remember(
            threads,
            projects,
            environments,
            home,
            preferences.projectGrouping,
            preferences.threadSort,
            threadDrafts,
            newTaskDraft,
            pendingRequests,
            searchMatches,
        ) {
            homeListItems(
                threads = threads,
                projects = projects,
                environments = environments,
                state = home,
                grouping = preferences.projectGrouping,
                sort = preferences.threadSort,
                drafts = threadDrafts,
                newTask = newTaskDraft,
                pendingRequests = pendingRequests,
                serverSearchMatches = searchMatches,
            )
        }
    val counts = remember(threads) { homeFilterCounts(threads) }
    // Swipe actions are capability-gated per environment, and a merged home draws
    // rows from several at once.
    val environmentsById = remember(environments) { environments.associateBy { it.id.value } }
    val listState = rememberLazyListState()
    val offline = environments.count { connectionPresentation(it.state).offline }
    // What the list is waiting on, if anything. A pill once there are rows, the whole
    // screen when there are none.
    val wait =
        remember(environments, items.isEmpty(), paired, sessionRestored) {
            waitNotice(
                states = environments.map { it.state },
                environmentLabel = environments.singleOrNull()?.label,
                resourceName = "threads",
                hasContent = items.isNotEmpty(),
                awaitingEnvironments =
                    environments.isEmpty() && (!sessionRestored || paired),
            )
        }

    S5Screen(
        title = "S5 Code",
        subtitle =
            when {
                environments.isEmpty() -> "No environments"
                offline > 0 -> "${environments.size} environments · $offline unreachable"
                else -> "${environments.size} environments online"
            },
        prominence = S5TopBarProminence.Hero,
        actions = {
            S5IconButton(
                icon = if (searchVisible) Icons.Rounded.Close else Icons.Rounded.Search,
                label = if (searchVisible) "Close search" else "Search threads",
                onClick = {
                    searchVisible = !searchVisible
                    if (!searchVisible) store.updateHome { it.copy(query = "") }
                },
            )
            S5OverflowMenu(
                icon = Icons.AutoMirrored.Rounded.Sort,
                label = "Sort and group",
                expanded = sortMenuOpen,
                onExpandedChange = { sortMenuOpen = it },
                options =
                    ThreadSort.entries.map { sort ->
                        S5MenuOption(
                            id = "sort:${sort.name}",
                            label = sort.label,
                            selected = preferences.threadSort == sort,
                        )
                    },
                onSelect = { id ->
                    val sort = ThreadSort.valueOf(id.removePrefix("sort:"))
                    store.updatePreferences { it.copy(threadSort = sort) }
                },
            )
            S5IconButton(icon = Icons.Rounded.Hub, label = "Connections", onClick = onConnections)
            S5IconButton(icon = Icons.Rounded.Settings, label = "Settings", onClick = onSettings)
        },
        floatingActionButton = {
            // The pill sits above the FAB rather than over the list: it describes the
            // whole screen, and a banner pushed in above the rows would move the row
            // under the user's thumb every time a connection wobbled.
            Column(
                horizontalAlignment = Alignment.End,
                verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                AnimatedVisibility(wait != null && items.isNotEmpty()) {
                    wait?.let { notice ->
                        S5WaitPill(
                            label = waitPillLabel(notice),
                            spinning = notice.spinning,
                            onClick = onConnections,
                        )
                    }
                }
                S5HeroFab(icon = Icons.Rounded.Add, label = "New task", onClick = onNewTask)
            }
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            AnimatedVisibility(searchVisible) {
                S5SearchField(
                    value = home.query,
                    onValueChange = { text -> store.updateHome { it.copy(query = text) } },
                    placeholder = "Search threads",
                    focusRequester = searchFocusRequester,
                    modifier =
                        Modifier.padding(
                            horizontal = S5Theme.spacing.gutter,
                            vertical = S5Theme.spacing.small,
                        ),
                )
            }
            if (searchLoading) S5LoadingStrip()
            Row(
                Modifier.fillMaxWidth()
                    .horizontalScroll(rememberScrollState())
                    .padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.small),
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                S5ConnectedButtonGroup(
                    options = ThreadFilter.entries,
                    selected = home.filter,
                    onSelect = { filter -> store.updateHome { it.copy(filter = filter) } },
                    label = { filter ->
                        val count = counts[filter] ?: 0
                        if (filter == ThreadFilter.All) filter.label else "${filter.label} $count"
                    },
                )
            }

            if (items.isEmpty() && wait?.phase == WaitPhase.Loading) {
                // Connected and waiting on the first snapshot: rows are coming, so the
                // screen shows their shape. A centred spinner here reads as "something
                // is wrong", and the empty state would be an outright lie.
                Column(Modifier.fillMaxSize()) {
                    repeat(HOME_SKELETON_ROWS) { S5SkeletonRow() }
                }
            } else if (items.isEmpty() && wait != null && home.query.isBlank()) {
                // Connecting, offline, or needing sign-in: say so rather than claiming
                // there are no threads. "No threads yet" on a connection that has not
                // landed is the lie the RN client's notice exists to avoid.
                S5WaitState(
                    title = wait.title,
                    detail = wait.detail,
                    icon = if (wait.phase == WaitPhase.SignInNeeded) Icons.Rounded.Key else Icons.Rounded.CloudOff,
                    spinning = wait.spinning,
                    actionLabel = if (wait.spinning) null else "Open connections",
                    onAction = onConnections,
                )
            } else if (items.isEmpty()) {
                S5EmptyState(
                    icon = Icons.Rounded.Inbox,
                    title = if (home.query.isBlank()) "No threads yet" else "No matches",
                    detail =
                        if (home.query.isBlank()) {
                            "Start a task and it shows up here across every connected environment."
                        } else {
                            "Nothing matches \"${home.query}\". Try a shorter search."
                        },
                    actionLabel = if (home.query.isBlank()) "New task" else "Clear search",
                    onAction = {
                        if (home.query.isBlank()) onNewTask()
                        else store.updateHome { it.copy(query = "") }
                    },
                )
            } else {
                LazyColumn(
                    state = listState,
                    modifier = Modifier.fillMaxSize(),
                    contentPadding =
                        PaddingValues(
                            start = S5Theme.spacing.gutter,
                            end = S5Theme.spacing.gutter,
                            bottom = 96.dp,
                        ),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    items(items, key = { it.key }) { item ->
                        Box(Modifier.animateItem()) {
                        when (item) {
                            is HomeListItem.Section -> S5SectionHeader(item.label)

                            is HomeListItem.Draft ->
                                DraftRow(
                                    item = item,
                                    onClick = onResumeDraft,
                                    onDiscard = {
                                        store.updateDraft {
                                            it.copy(prompt = "", attachments = emptyList())
                                        }
                                    },
                                )

                            is HomeListItem.ShelfHeader ->
                                ShelfHeaderRow(
                                    item = item,
                                    onToggle = {
                                        store.updateHome { state ->
                                            when (item.kind) {
                                                ShelfKind.Snoozed ->
                                                    state.copy(snoozedExpanded = !state.snoozedExpanded)
                                                ShelfKind.Settled ->
                                                    state.copy(settledExpanded = !state.settledExpanded)
                                            }
                                        }
                                    },
                                )

                            is HomeListItem.Queued ->
                                QueuedRow(item, store.workspace::projectIconUrl)

                            is HomeListItem.PendingApprovalCard -> {
                                val requestKey = item.key
                                HomeApprovalCard(
                                    item = item,
                                    resolveProjectIconUrl = store.workspace::projectIconUrl,
                                    submitting = submittingRequestKey == requestKey,
                                    onOpenThread = {
                                        onOpenThread(
                                            item.thread.environmentId.value,
                                            item.thread.id.value,
                                        )
                                    },
                                    onDecision = { decision ->
                                        if (submittingRequestKey == null) {
                                            submittingRequestKey = requestKey
                                            scope.launch {
                                                try {
                                                    store.workspace.respondToApproval(
                                                        item.thread.environmentId,
                                                        item.thread.id,
                                                        item.approval.id,
                                                        decision,
                                                    )
                                                } catch (cancelled: CancellationException) {
                                                    throw cancelled
                                                } catch (error: Exception) {
                                                    reportError(
                                                        error.message ?: "The approval response could not be sent."
                                                    )
                                                } finally {
                                                    if (submittingRequestKey == requestKey) {
                                                        submittingRequestKey = null
                                                    }
                                                }
                                            }
                                        }
                                    },
                                )
                            }

                            is HomeListItem.PendingInputCard -> {
                                val requestKey = item.key
                                HomeUserInputCard(
                                    item = item,
                                    resolveProjectIconUrl = store.workspace::projectIconUrl,
                                    submitting = submittingRequestKey == requestKey,
                                    onOpenThread = {
                                        onOpenThread(
                                            item.thread.environmentId.value,
                                            item.thread.id.value,
                                        )
                                    },
                                    onSubmit = { answers ->
                                        if (submittingRequestKey == null) {
                                            submittingRequestKey = requestKey
                                            scope.launch {
                                                try {
                                                    store.workspace.respondToInput(
                                                        item.thread.environmentId,
                                                        item.thread.id,
                                                        item.request.id,
                                                        answers,
                                                    )
                                                } catch (cancelled: CancellationException) {
                                                    throw cancelled
                                                } catch (error: Exception) {
                                                    reportError(
                                                        error.message ?: "The answer could not be sent."
                                                    )
                                                } finally {
                                                    if (submittingRequestKey == requestKey) {
                                                        submittingRequestKey = null
                                                    }
                                                }
                                            }
                                        }
                                    },
                                )
                            }

                            is HomeListItem.Thread -> {
                                val threadKey = item.thread.id.value
                                val environment =
                                    environmentsById[item.thread.environmentId.value]
                                val swipes =
                                    threadSwipeActions(
                                        thread = item.thread,
                                        settlementSupported =
                                            environment
                                                ?.capabilities
                                                ?.threadSettlement != false,
                                        snoozeSupported =
                                            environment?.capabilities?.threadSnooze == true,
                                    )
                                S5SwipeableRow(
                                    endAction =
                                        swipes.end?.let { action ->
                                            swipeActionFor(action) {
                                                launchThreadAction(item.thread, action.command)
                                            }
                                        },
                                    startAction =
                                        swipes.start?.let { action ->
                                            swipeActionFor(action) {
                                                launchThreadAction(item.thread, action.command)
                                            }
                                        },
                                ) {
                                    ThreadRow(
                                        thread = item.thread,
                                        project = item.project,
                                        environmentLabel = item.environmentLabel,
                                        draftPreview = item.draftPreview,
                                        searchMatch = item.searchMatch,
                                        searchQuery = home.query,
                                        resolveProjectIconUrl = store.workspace::projectIconUrl,
                                        onClick = {
                                            onOpenThread(
                                                item.thread.environmentId.value,
                                                threadKey,
                                            )
                                        },
                                        trailing = {
                                            val titleRegenerationSupported =
                                                environment
                                                    ?.capabilities
                                                    ?.threadTitleRegeneration == true
                                            S5OverflowMenu(
                                                icon = Icons.Rounded.MoreVert,
                                                label = "Thread actions",
                                                expanded = rowMenuFor == threadKey,
                                                onExpandedChange = { open ->
                                                    rowMenuFor = if (open) threadKey else null
                                                },
                                                options =
                                                    threadMenuOptions(
                                                        thread = item.thread,
                                                        titleRegenerationSupported =
                                                            titleRegenerationSupported,
                                                    ),
                                                onSelect = { action ->
                                                    val runAction = {
                                                        launchThreadAction(item.thread, action)
                                                    }
                                                    if (action == "delete") {
                                                        confirmController.show(
                                                            S5ConfirmDialogRequest(
                                                                title = "Delete thread?",
                                                                message =
                                                                    "\"${item.thread.title}\" and its local thread history will be permanently deleted.",
                                                                confirmText = "Delete",
                                                                destructive = true,
                                                                onConfirm = runAction,
                                                            )
                                                        )
                                                    } else {
                                                        runAction()
                                                    }
                                                },
                                            )
                                        },
                                    )
                                }
                            }
                        }
                        }
                    }
                    item(key = "archive-link") {
                        Box(Modifier.fillMaxWidth().padding(top = S5Theme.spacing.large)) {
                            S5Button(
                                text = "Archived threads",
                                onClick = onArchive,
                                icon = Icons.Rounded.Archive,
                                emphasis = S5ActionEmphasis.Prominent,
                                style = S5ButtonStyle.Text,
                            )
                        }
                    }
                }
            }
        }
    }
}

/**
 * How a swipe action is dressed. Settle reads as completion and snooze as
 * deferral, so they borrow the settled and approval status colors rather than
 * inventing a second palette for the same two ideas.
 */
@Composable
private fun swipeActionFor(action: ThreadSwipeAction, onAction: () -> Unit): S5SwipeAction =
    when (action) {
        ThreadSwipeAction.Settle,
        ThreadSwipeAction.Unsettle ->
            S5SwipeAction(
                label = action.label,
                icon = Icons.Rounded.DoneAll,
                containerColor = S5Theme.status.settledContainer,
                contentColor = S5Theme.status.onSettledContainer,
                onAction = onAction,
            )
        ThreadSwipeAction.Snooze,
        ThreadSwipeAction.Unsnooze ->
            S5SwipeAction(
                label = action.label,
                icon = Icons.Rounded.Bedtime,
                containerColor = S5Theme.status.approvalContainer,
                contentColor = S5Theme.status.onApprovalContainer,
                onAction = onAction,
            )
        ThreadSwipeAction.Archive ->
            S5SwipeAction(
                label = action.label,
                icon = Icons.Rounded.Archive,
                containerColor = MaterialTheme.colorScheme.surfaceContainerHighest,
                contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
                onAction = onAction,
            )
    }

/**
 * Row menu for a thread in the list, matching the RN client's: lifecycle plus
 * title regeneration. This is the only place thread lifecycle lives, so the
 * thread screen's header can stay about the work rather than the object.
 */
internal fun threadMenuOptions(
    thread: ThreadSummary,
    titleRegenerationSupported: Boolean,
): List<S5MenuOption> {
    return buildList {
        add(
            S5MenuOption(
                id = "pin",
                label = if (thread.pinned) "Unpin" else "Pin",
                icon = Icons.Rounded.PushPin,
            )
        )
        add(
            S5MenuOption(
                id = "snooze",
                label = if (thread.status == ThreadStatus.Snoozed) "Unsnooze" else "Snooze",
                icon = Icons.Rounded.Bedtime,
            )
        )
        add(
            S5MenuOption(
                id = "settle",
                label = if (thread.status == ThreadStatus.Settled) "Reopen" else "Settle",
                icon = Icons.Rounded.DoneAll,
            )
        )
        if (titleRegenerationSupported) {
            add(
                S5MenuOption(
                    id = "regenerate-title",
                    label = if (thread.titleRegenerating) "Regenerating…" else "Regenerate title",
                    icon = Icons.Rounded.Refresh,
                    enabled = !thread.titleRegenerating,
                )
            )
        }
        add(S5MenuOption(id = "archive", label = "Archive", icon = Icons.Rounded.Archive))
        add(
            S5MenuOption(
                id = "delete",
                label = "Delete",
                icon = Icons.Rounded.Close,
                destructive = true,
            )
        )
    }
}

private suspend fun performThreadAction(
    store: AppStore,
    environmentId: EnvironmentId,
    id: ThreadId,
    action: String,
    pinned: Boolean,
    status: ThreadStatus,
) {
    when (action) {
        "pin" -> store.workspace.setPinned(environmentId, id, !pinned)
        "snooze" -> store.workspace.setSnoozed(environmentId, id, status != ThreadStatus.Snoozed)
        "settle" -> store.workspace.setSettled(environmentId, id, status != ThreadStatus.Settled)
        "regenerate-title" -> store.workspace.regenerateTitle(environmentId, id)
        "archive" -> store.workspace.setArchived(environmentId, id, true)
        "delete" -> store.workspace.deleteThread(environmentId, id)
    }
}

@Composable
private fun ShelfHeaderRow(item: HomeListItem.ShelfHeader, onToggle: () -> Unit) {
    S5Card(tone = S5CardTone.Receded, onClick = onToggle, modifier = Modifier.fillMaxWidth()) {
        Row(
            Modifier.fillMaxWidth().padding(S5Theme.spacing.medium),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Icon(
                if (item.expanded) Icons.Rounded.ExpandLess else Icons.Rounded.ExpandMore,
                contentDescription = null,
                modifier = Modifier.size(20.dp),
            )
            Text("${item.label} (${item.count})", style = MaterialTheme.typography.labelLargeEmphasized)
        }
    }
}

/**
 * The unsent new-task draft, at the top of the list.
 *
 * Amber and a pencil, matching the in-thread draft line and the web sidebar's draft
 * row, so "unsent" reads the same wherever it appears. Discard is on the row rather
 * than behind a menu: this is the one row whose whole purpose is to be temporary,
 * and the alternative is a prompt the user has to open just to clear.
 */
@Composable
private fun DraftRow(item: HomeListItem.Draft, onClick: () -> Unit, onDiscard: () -> Unit) {
    S5Card(tone = S5CardTone.Receded, onClick = onClick, modifier = Modifier.fillMaxWidth()) {
        Row(
            Modifier.fillMaxWidth().padding(S5Theme.spacing.large),
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                Icons.Rounded.EditNote,
                contentDescription = null,
                tint = S5Theme.status.approval,
                modifier = Modifier.size(20.dp),
            )
            Column(Modifier.weight(1f)) {
                Text(
                    "Unsent task",
                    style = MaterialTheme.typography.labelMedium,
                    color = S5Theme.status.approval,
                )
                Text(
                    item.preview,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                // The project is what tells two drafts apart, and a draft with no
                // project yet is still resumable, so the line is dropped rather than
                // filled with a placeholder.
                listOfNotNull(item.projectTitle, item.environmentLabel)
                    .takeIf { it.isNotEmpty() }
                    ?.let { meta ->
                        Text(
                            meta.joinToString(" · "),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
            }
            S5IconButton(
                icon = Icons.Rounded.Close,
                label = "Discard draft",
                onClick = onDiscard,
            )
        }
    }
}

@Composable
private fun QueuedRow(item: HomeListItem.Queued, resolveProjectIconUrl: suspend (Project) -> String?) {
    S5Card(tone = S5CardTone.Receded, modifier = Modifier.fillMaxWidth()) {
        Row(
            Modifier.fillMaxWidth().padding(S5Theme.spacing.large),
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            S5ProjectIcon(
                project = item.project,
                resolveUrl = resolveProjectIconUrl,
                size = 28.dp,
            )
            Column(Modifier.weight(1f)) {
                Text(
                    item.thread.title,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    item.thread.excerpt ?: "Queued until the environment reconnects",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Icon(
                Icons.AutoMirrored.Rounded.Article,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(18.dp),
            )
        }
    }
}

/**
 * Placeholder rows drawn while the first snapshot lands. Six, which is about a
 * phone screen: fewer reads as a broken list, and more only adds rows nobody sees
 * before the real ones replace them.
 */
private const val HOME_SKELETON_ROWS = 6
private const val SEARCH_DEBOUNCE_MILLIS = 250L
