package club.touchtech.s5code.kotlin.feature.pullrequests

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.CallMerge
import androidx.compose.material.icons.automirrored.rounded.OpenInNew
import androidx.compose.material.icons.rounded.Cancel
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.FilterList
import androidx.compose.material.icons.rounded.Hub
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Schedule
import androidx.compose.material.icons.rounded.SearchOff
import androidx.compose.material.icons.rounded.Settings
import androidx.compose.material.icons.rounded.Share
import androidx.compose.material.icons.rounded.Visibility
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
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.DEFAULT_PR_LIST_CONTROLS
import club.touchtech.s5code.kotlin.data.PullRequestListControls
import club.touchtech.s5code.kotlin.data.PullRequestListReview
import club.touchtech.s5code.kotlin.data.PullRequestListTarget
import club.touchtech.s5code.kotlin.data.PullRequestProjectScope
import club.touchtech.s5code.kotlin.data.PullRequestReference
import club.touchtech.s5code.kotlin.data.PullRequestTone
import club.touchtech.s5code.kotlin.data.ScopedPullRequestEntry
import club.touchtech.s5code.kotlin.data.applyPullRequestDraftFilterChange
import club.touchtech.s5code.kotlin.data.filterPullRequestList
import club.touchtech.s5code.kotlin.data.mergeEnvironmentPullRequestPages
import club.touchtech.s5code.kotlin.data.nextPullRequestListInput
import club.touchtech.s5code.kotlin.data.openOnProviderLabel
import club.touchtech.s5code.kotlin.data.pullRequestChecksStateTone
import club.touchtech.s5code.kotlin.data.pullRequestConflict
import club.touchtech.s5code.kotlin.data.pullRequestLabelColor
import club.touchtech.s5code.kotlin.data.pullRequestListFilterCount
import club.touchtech.s5code.kotlin.data.pullRequestListInput
import club.touchtech.s5code.kotlin.data.pullRequestListReviewStatus
import club.touchtech.s5code.kotlin.data.pullRequestStatus
import club.touchtech.s5code.kotlin.data.reference
import club.touchtech.s5code.kotlin.data.relativeLabel
import club.touchtech.s5code.kotlin.data.scopedPullRequestListKey
import club.touchtech.s5code.kotlin.data.withoutScratchProjectScope
import club.touchtech.s5code.kotlin.data.canPerformPullRequestAction
import club.touchtech.s5code.kotlin.data.pullRequestActionLabel
import club.touchtech.s5code.kotlin.data.pullRequestListRowActions
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogRequest
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.EditNote
import androidx.compose.material.icons.rounded.Replay
import androidx.compose.material.icons.rounded.TaskAlt
import kotlinx.coroutines.flow.first
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5FilterMenu
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.component.S5LoadingStrip
import club.touchtech.s5code.kotlin.design.component.S5MenuOption
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5OverflowMenu
import club.touchtech.s5code.kotlin.design.component.S5PullToRefreshBox
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SearchField
import club.touchtech.s5code.kotlin.design.component.S5SkeletonRow
import club.touchtech.s5code.kotlin.design.component.S5TextBadge
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.rememberClipboardWriter
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.model.Environment
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch

private val STATUS_ACTION_ICONS =
    mapOf(
        "ready" to Icons.Rounded.TaskAlt,
        "draft" to Icons.Rounded.EditNote,
        "close" to Icons.Rounded.Close,
        "reopen" to Icons.Rounded.Replay,
    )

private val STATES = listOf("open" to "Open", "closed" to "Closed", "merged" to "Merged", "all" to "All states")
private val INVOLVEMENT = listOf("all" to "All PRs", "reviewing" to "Your review", "authored" to "By you")
private val DRAFTS = listOf(null to "All drafts", "only" to "Drafts only", "hide" to "Hide drafts")
private val REVIEWS =
    listOf(
        null to "All reviews",
        "approved" to "Approved",
        "changes-requested" to "Changes requested",
        "review-required" to "Review required",
        "none" to "No reviews",
    )
private val CHECKS = listOf(null to "All checks", "passing" to "Passing", "failing" to "Failing")
private val SORTS = listOf("updated" to "Recently updated", "created" to "Newest created", "oldest" to "Oldest updated")
private const val SEARCH_DEBOUNCE_MS = 180L
private const val MAX_FACET_OPTIONS = 60
private const val MAX_LABEL_FILTERS = 10
private const val LOAD_MORE_THRESHOLD = 5
private const val STATS_FALLBACK_ROWS = 8

/** Environments the tab can read: live with pull request support, or remembered from a past session. */
fun pullRequestEnvironments(environments: List<Environment>, known: Set<String>): List<Environment> =
    environments.filter { it.isEnabled && (it.capabilities.pullRequests || it.id.value in known) }

/**
 * The Pull requests tab: every pull request the selected environments'
 * projects can see, with GitHub-style search qualifiers and saved filter
 * menus. Read-only in this phase; a row opens the PR page.
 */
@Composable
fun PullRequestListScreen(
    store: AppStore,
    onOpenPullRequest: (environmentId: String, reference: PullRequestReference) -> Unit,
    onSettings: () -> Unit,
    onConnections: () -> Unit,
    confirmController: S5ConfirmDialogController,
    bottomBar: @Composable () -> Unit,
) {
    val gateway = store.pullRequests
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val copy = rememberClipboardWriter()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val known by gateway.knownEnvironments.collectAsStateWithLifecycle()
    val savedEnvironment by gateway.listEnvironment.collectAsStateWithLifecycle()
    val savedControls by gateway.listControls.collectAsStateWithLifecycle()

    val available = remember(environments, known) { pullRequestEnvironments(environments, known) }
    val environmentId = savedEnvironment?.takeIf { id -> available.any { it.id.value == id } }
    val scopeKey = environmentId.orEmpty()
    val controls =
        remember(savedControls, scopeKey, projects) {
            withoutScratchProjectScope(
                savedControls[scopeKey] ?: DEFAULT_PR_LIST_CONTROLS,
                projects.map { PullRequestProjectScope(it.id.value, it.environmentId.value, it.isScratch) },
            )
        }
    fun update(next: PullRequestListControls) = gateway.setListControls(scopeKey, next)

    var query by rememberSaveable { mutableStateOf("") }
    var searchQuery by remember { mutableStateOf(query) }
    LaunchedEffect(query) {
        delay(SEARCH_DEBOUNCE_MS)
        searchQuery = query
    }

    val targets =
        remember(available, environmentId, controls, searchQuery) {
            val input = pullRequestListInput(controls, searchQuery)
            available
                .filter { environment ->
                    (environmentId == null || environment.id.value == environmentId) &&
                        (controls.projectEnvironmentId == null || environment.id.value == controls.projectEnvironmentId)
                }
                .sortedBy { it.id.value }
                .map { PullRequestListTarget(it.id.value, input) }
        }
    val sources by remember(targets) { gateway.list(targets) }.collectAsStateWithLifecycle(emptyList())
    val stats by
        remember(targets) {
                val ids = targets.map { it.environmentId }.distinct()
                if (ids.isEmpty()) flowOf(emptyMap())
                else combine(ids.map { id -> gateway.stats(id).map { id to it } }) { it.toMap() }
            }
            .collectAsStateWithLifecycle(emptyMap())
    val connectedIds =
        remember(environments) {
            environments.filter { it.state == ConnectionState.Connected }.mapTo(HashSet()) { it.id.value }
        }
    val merged =
        remember(sources, stats) {
            mergeEnvironmentPullRequestPages(
                sources.mapNotNull { source ->
                    val pages = source.pages ?: return@mapNotNull null
                    val counts = stats[source.target.environmentId].orEmpty()
                    source.target.environmentId to
                        pages.copy(
                            entries =
                                pages.entries.map { entry ->
                                    counts["${entry.projectId}:${entry.repository}:${entry.number}"]
                                        ?.let { entry.copy(additions = it.additions, deletions = it.deletions) }
                                        ?: entry
                                }
                        )
                }
            )
        }
    val rows =
        remember(merged, controls, searchQuery, connectedIds) {
            filterPullRequestList(merged.entries, controls, merged.viewers, searchQuery, merged.localSearchHosts, connectedIds)
        }

    // Facets only grow within a scope, so picking a label never empties its own menu.
    val facetAuthors = remember(scopeKey) { sortedSetOf<String>() }
    val facetLabels = remember(scopeKey) { sortedSetOf<String>() }
    merged.entries.forEach { row ->
        row.entry.author?.login?.takeIf { it.isNotEmpty() }?.let(facetAuthors::add)
        row.entry.labels.forEach { facetLabels += it.name }
    }

    var pageLoading by remember(targets) { mutableStateOf(false) }
    var pageError by remember(targets) { mutableStateOf<String?>(null) }
    var refreshing by remember(targets) { mutableStateOf(false) }
    var rowMenuFor by remember { mutableStateOf<String?>(null) }
    val hasMore = sources.any { source -> source.pages?.let { nextPullRequestListInput(it, source.target.input) } != null }

    fun loadMore(retry: Boolean) {
        if (pageLoading || refreshing || (!retry && pageError != null)) return
        pageLoading = true
        pageError = null
        scope.launch {
            try {
                gateway.loadMore(targets, retry)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (error: Exception) {
                pageError = error.message ?: "Couldn't load more pull requests."
            } finally {
                pageLoading = false
            }
        }
    }

    val writableEnvironments =
        remember(environments) {
            environments.filter { it.capabilities.sessionAllowsSourceControlWrite }.mapTo(HashSet()) { it.id.value }
        }

    // RN's row status writes: confirm, re-read the detail (the row may be stale), write, refresh the list.
    fun requestStatusAction(environmentId: String, row: ScopedPullRequestEntry, action: String) {
        val entry = row.entry
        val reference = entry.reference()
        val label = pullRequestActionLabel(action)
        confirmController.show(
            S5ConfirmDialogRequest(
                title = "$label?",
                message = "#${entry.number} · ${entry.title}",
                confirmText = label,
                destructive = action == "close",
                onConfirm = {
                    scope.launch {
                        try {
                            if (!gateway.connected(environmentId).first()) {
                                throw IllegalStateException("Reconnect to update this pull request.")
                            }
                            gateway.refreshDetail(environmentId, reference)
                            val fresh = gateway.detail(environmentId, reference).first { it.hasValue || it.error != null }
                            val detail = fresh.value ?: throw IllegalStateException(fresh.error ?: "Couldn't read this pull request.")
                            if (!canPerformPullRequestAction(detail, action)) {
                                throw IllegalStateException("This pull request changed. Open it to see what's available.")
                            }
                            gateway.runAction(environmentId, reference, action)
                            runCatching { gateway.refreshList(targets) }
                        } catch (cancelled: CancellationException) {
                            throw cancelled
                        } catch (error: Exception) {
                            confirmController.alert("Couldn't update this pull request", error.alertMessage())
                        }
                    }
                },
            )
        )
    }

    fun refresh() {
        if (refreshing || targets.isEmpty()) return
        refreshing = true
        pageError = null
        scope.launch {
            try {
                gateway.refreshList(targets)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (error: Exception) {
                store.showError(error.message ?: "Couldn't refresh pull requests.")
            } finally {
                refreshing = false
            }
        }
    }

    val listState = rememberLazyListState()
    LaunchedEffect(listState, rows.size, hasMore) {
        snapshotFlow { listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: -1 }
            .distinctUntilChanged()
            .collect { last -> if (hasMore && rows.isNotEmpty() && last >= rows.size - LOAD_MORE_THRESHOLD) loadMore(false) }
    }
    // Diff counts the listing leaves out (GitHub), for the rows on screen.
    LaunchedEffect(listState, rows) {
        snapshotFlow { listState.layoutInfo.visibleItemsInfo.map { it.key } }
            .distinctUntilChanged()
            .collect { keys ->
                val visible = rows.filter { scopedPullRequestListKey(it) in keys }.ifEmpty { rows.take(STATS_FALLBACK_ROWS) }
                visible
                    .filter { it.entry.provider == "github" && it.entry.additions + it.entry.deletions == 0 && it.environmentId != null }
                    .groupBy { it.environmentId!! }
                    .forEach { (id, group) -> gateway.loadStats(id, group.map { it.entry.reference() }) }
            }
    }

    val environmentLabels = remember(environments) { environments.associate { it.id.value to it.label } }
    val errors =
        sources.mapNotNull { source ->
            source.query.error?.let { "${environmentLabels[source.target.environmentId] ?: "Environment"}: $it" }
        } + merged.errors.map { "${it.projectTitle.ifEmpty { environmentLabels[it.environmentId] ?: "Project" }}: ${it.message}" }
    val headLoading = sources.any { it.pages == null && it.query.loading } ||
        (sources.isNotEmpty() && sources.all { it.pages == null && it.query.error == null && it.target.environmentId in connectedIds })
    val allOffline = targets.isNotEmpty() && targets.none { it.environmentId in connectedIds }

    S5Screen(
        title = "Pull requests",
        prominence = S5TopBarProminence.Hero,
        actions = {
            S5IconButton(icon = Icons.Rounded.Hub, label = "Connections", onClick = onConnections)
            S5IconButton(icon = Icons.Rounded.Settings, label = "Settings", onClick = onSettings)
        },
        bottomBar = bottomBar,
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            S5SearchField(
                value = query,
                onValueChange = { query = it },
                placeholder = "Search, or label:bug author:me",
                modifier = Modifier.padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.small),
            )
            PullRequestFilterRow(
                controls = controls,
                query = query,
                environments = available,
                environmentId = environmentId,
                projects = projects.filter { project -> !project.isScratch && available.any { it.id == project.environmentId } }
                    .filter { environmentId == null || it.environmentId.value == environmentId }
                    .map { Triple(it.environmentId.value, it.id.value, it.title) },
                authors = facetAuthors.take(MAX_FACET_OPTIONS),
                labels = facetLabels.take(MAX_FACET_OPTIONS),
                onEnvironment = gateway::setListEnvironment,
                onControls = ::update,
                onDraftChange = { next ->
                    val change = applyPullRequestDraftFilterChange(query, controls.filters, next)
                    query = change.query
                    update(controls.copy(filters = change.filters))
                },
            )
            if (refreshing || (headLoading && rows.isNotEmpty())) S5LoadingStrip()
            S5PullToRefreshBox(isRefreshing = refreshing, onRefresh = ::refresh, modifier = Modifier.fillMaxSize()) {
                when {
                    rows.isEmpty() && headLoading && !allOffline ->
                        Column(Modifier.fillMaxSize()) { repeat(6) { S5SkeletonRow() } }
                    rows.isEmpty() -> {
                        val (icon, title, detail) =
                            when {
                                available.isEmpty() ->
                                    Triple(Icons.Rounded.Hub, "Connect an environment", "Add an environment in Settings to browse its pull requests.")
                                allOffline ->
                                    Triple(Icons.Rounded.CloudOff, "Environment offline", "Reconnect to load pull requests that aren't saved on this device.")
                                errors.isNotEmpty() ->
                                    Triple(Icons.Rounded.CloudOff, "Couldn't load pull requests", errors.joinToString("\n"))
                                searchQuery.isNotBlank() ->
                                    Triple(Icons.Rounded.SearchOff, "No results", "No pull requests match \"$searchQuery\".")
                                else ->
                                    Triple(
                                        Icons.AutoMirrored.Rounded.CallMerge,
                                        "No matching pull requests",
                                        "Pull requests from the selected environments' projects appear here.",
                                    )
                            }
                        S5EmptyState(
                            icon = icon,
                            title = title,
                            detail = detail,
                            actionLabel =
                                when {
                                    available.isEmpty() || allOffline -> "Open connections"
                                    errors.isNotEmpty() -> "Try again"
                                    else -> null
                                },
                            onAction = {
                                if (available.isEmpty() || allOffline) onConnections() else refresh()
                            },
                        )
                    }
                    else ->
                        LazyColumn(
                            state = listState,
                            modifier = Modifier.fillMaxSize(),
                            contentPadding =
                                PaddingValues(
                                    start = S5Theme.spacing.gutter,
                                    end = S5Theme.spacing.gutter,
                                    bottom = S5Theme.spacing.xxLarge,
                                ),
                            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                        ) {
                            if (errors.isNotEmpty()) {
                                item(key = "errors") {
                                    S5Notice(icon = Icons.Rounded.CloudOff, text = errors.joinToString("\n"))
                                }
                            } else if (allOffline) {
                                item(key = "offline") {
                                    S5Notice(icon = Icons.Rounded.CloudOff, text = "Offline. Showing pull requests saved on this device.")
                                }
                            }
                            items(rows, key = ::scopedPullRequestListKey) { row ->
                                val key = scopedPullRequestListKey(row)
                                PullRequestListRow(
                                    row = row,
                                    statusActions =
                                        if (writableEnvironments.contains(row.environmentId)) {
                                            pullRequestListRowActions(row.entry.state, row.entry.isDraft)
                                        } else {
                                            emptyList()
                                        },
                                    environmentLabel = if (targets.size > 1) environmentLabels[row.environmentId] else null,
                                    menuExpanded = rowMenuFor == key,
                                    onMenuExpandedChange = { open -> rowMenuFor = if (open) key else null },
                                    onOpen = { row.environmentId?.let { onOpenPullRequest(it, row.entry.reference()) } },
                                    onMenuSelect = { action ->
                                        val entry = row.entry
                                        when (action) {
                                            "open" -> row.environmentId?.let { onOpenPullRequest(it, entry.reference()) }
                                            "share" ->
                                                if (!sharePullRequest(context, entry.title, entry.url)) {
                                                    store.showError("The pull request could not be shared.")
                                                }
                                            "copy" -> copy(entry.url)
                                            "provider" ->
                                                if (!openPullRequestUrl(context, entry.url)) {
                                                    store.showError("The pull request could not be opened.")
                                                }
                                            else -> row.environmentId?.let { requestStatusAction(it, row, action) }
                                        }
                                    },
                                )
                            }
                            if (pageLoading || pageError != null) {
                                item(key = "footer") {
                                    Box(Modifier.fillMaxWidth().padding(S5Theme.spacing.large), contentAlignment = Alignment.Center) {
                                        val message = pageError
                                        if (message != null) {
                                            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                                Text(
                                                    message,
                                                    style = MaterialTheme.typography.bodySmall,
                                                    color = MaterialTheme.colorScheme.error,
                                                )
                                                S5Button(text = "Try again", onClick = { loadMore(true) }, style = S5ButtonStyle.Text)
                                            }
                                        } else {
                                            S5InlineLoading()
                                        }
                                    }
                                }
                            }
                        }
                }
            }
        }
    }
}

@Composable
private fun PullRequestFilterRow(
    controls: PullRequestListControls,
    query: String,
    environments: List<Environment>,
    environmentId: String?,
    /** (environmentId, projectId, title). */
    projects: List<Triple<String, String, String>>,
    authors: List<String>,
    labels: List<String>,
    onEnvironment: (String?) -> Unit,
    onControls: (PullRequestListControls) -> Unit,
    onDraftChange: (club.touchtech.s5code.kotlin.data.PullRequestListFilters) -> Unit,
) {
    val filters = controls.filters
    val count = pullRequestListFilterCount(controls)
    val typedDraft = club.touchtech.s5code.kotlin.data.parsePullRequestQuery(query).filters.draft
    val draft = typedDraft ?: filters.draft
    Row(
        Modifier.fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.tiny),
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        S5FilterMenu(
            label = if (count > 0) "Filters $count" else "Filters",
            active = count > 0 || controls.sort != "updated",
            icon = Icons.Rounded.FilterList,
            options =
                buildList {
                    add(
                        S5MenuOption(
                            id = "drafts",
                            label = "Drafts",
                            supporting = DRAFTS.first { it.first == draft }.second,
                            children = DRAFTS.map { (value, label) -> S5MenuOption("draft:${value.orEmpty()}", label, selected = draft == value) },
                        )
                    )
                    add(
                        S5MenuOption(
                            id = "reviews",
                            label = "Reviews",
                            supporting = REVIEWS.firstOrNull { it.first == filters.review }?.second,
                            children = REVIEWS.map { (value, label) -> S5MenuOption("review:${value.orEmpty()}", label, selected = filters.review == value) },
                        )
                    )
                    add(
                        S5MenuOption(
                            id = "checks",
                            label = "Checks",
                            supporting = CHECKS.firstOrNull { it.first == filters.checks }?.second,
                            children = CHECKS.map { (value, label) -> S5MenuOption("checks:${value.orEmpty()}", label, selected = filters.checks == value) },
                        )
                    )
                    if (projects.isNotEmpty()) {
                        add(
                            S5MenuOption(
                                id = "project",
                                label = "Project",
                                supporting = projects.firstOrNull { it.second == controls.projectId && it.first == controls.projectEnvironmentId }?.third
                                    ?: "All projects",
                                children =
                                    listOf(S5MenuOption("project:", "All projects", selected = controls.projectId == null)) +
                                        projects.map { (env, id, title) ->
                                            S5MenuOption(
                                                "project:$env\u0000$id",
                                                title,
                                                supporting = if (environmentId == null && environments.size > 1) {
                                                    environments.firstOrNull { it.id.value == env }?.label
                                                } else null,
                                                selected = controls.projectId == id && controls.projectEnvironmentId == env,
                                            )
                                        },
                            )
                        )
                    }
                    add(
                        S5MenuOption(
                            id = "sort",
                            label = "Sort",
                            supporting = SORTS.first { it.first == controls.sort }.second,
                            children = SORTS.map { (value, label) -> S5MenuOption("sort:$value", label, selected = controls.sort == value) },
                        )
                    )
                    add(S5MenuOption(id = "reset", label = "Reset filters", enabled = count > 0 || controls.sort != "updated"))
                },
            onSelect = { id ->
                when {
                    id.startsWith("draft:") -> onDraftChange(filters.copy(draft = id.removePrefix("draft:").ifEmpty { null }))
                    id.startsWith("review:") -> onControls(controls.copy(filters = filters.copy(review = id.removePrefix("review:").ifEmpty { null })))
                    id.startsWith("checks:") -> onControls(controls.copy(filters = filters.copy(checks = id.removePrefix("checks:").ifEmpty { null })))
                    id.startsWith("project:") -> {
                        val value = id.removePrefix("project:")
                        onControls(
                            if (value.isEmpty()) controls.copy(projectId = null, projectEnvironmentId = null)
                            else controls.copy(projectEnvironmentId = value.substringBefore('\u0000'), projectId = value.substringAfter('\u0000'))
                        )
                    }
                    id.startsWith("sort:") -> onControls(controls.copy(sort = id.removePrefix("sort:")))
                    id == "reset" -> onControls(DEFAULT_PR_LIST_CONTROLS)
                }
            },
        )
        if (environments.size > 1) {
            S5FilterMenu(
                label = environments.firstOrNull { it.id.value == environmentId }?.label ?: "All environments",
                active = environmentId != null,
                options =
                    listOf(S5MenuOption("env:", "All environments", selected = environmentId == null)) +
                        environments.map { S5MenuOption("env:${it.id.value}", it.label, selected = it.id.value == environmentId) },
                onSelect = { id -> onEnvironment(id.removePrefix("env:").ifEmpty { null }) },
            )
        }
        S5FilterMenu(
            label = STATES.first { it.first == controls.state }.second,
            active = controls.state != "open",
            options = STATES.map { (value, label) -> S5MenuOption(value, label, selected = controls.state == value) },
            onSelect = { onControls(controls.copy(state = it)) },
        )
        S5FilterMenu(
            label = INVOLVEMENT.first { it.first == controls.involvement }.second,
            active = controls.involvement != "all",
            options = INVOLVEMENT.map { (value, label) -> S5MenuOption(value, label, selected = controls.involvement == value) },
            onSelect = { onControls(controls.copy(involvement = it)) },
        )
        val selectedLabels = filters.labels.orEmpty().flatten()
        if (labels.isNotEmpty() || selectedLabels.isNotEmpty()) {
            S5FilterMenu(
                label =
                    when (selectedLabels.size) {
                        0 -> "Labels"
                        1 -> selectedLabels.single()
                        else -> "${selectedLabels.size} labels"
                    },
                active = selectedLabels.isNotEmpty(),
                options =
                    listOf(S5MenuOption("label:", "All labels", selected = selectedLabels.isEmpty())) +
                        (selectedLabels + labels).distinct().map { name ->
                            S5MenuOption(
                                "label:$name",
                                name,
                                selected = name in selectedLabels,
                                enabled = name in selectedLabels || selectedLabels.size < MAX_LABEL_FILTERS,
                            )
                        },
                onSelect = { id ->
                    val name = id.removePrefix("label:")
                    // Each menu pick is its own group: every picked label must match.
                    val next =
                        when {
                            name.isEmpty() -> emptyList()
                            name in selectedLabels -> selectedLabels - name
                            else -> selectedLabels + name
                        }
                    onControls(controls.copy(filters = filters.copy(labels = next.map { listOf(it) }.ifEmpty { null })))
                },
            )
        }
        if (authors.isNotEmpty() || !filters.author.isNullOrEmpty()) {
            S5FilterMenu(
                label =
                    when {
                        filters.author.isNullOrEmpty() -> "Author"
                        filters.author.equals("@me", true) || filters.author.equals("me", true) -> "By me"
                        else -> filters.author
                    },
                active = !filters.author.isNullOrEmpty(),
                options =
                    listOf(
                        S5MenuOption("author:", "All authors", selected = filters.author.isNullOrEmpty()),
                        S5MenuOption("author:@me", "Me", selected = filters.author.equals("@me", true)),
                    ) +
                        authors.map { login -> S5MenuOption("author:$login", login, selected = filters.author == login) },
                onSelect = { id ->
                    onControls(controls.copy(filters = filters.copy(author = id.removePrefix("author:").ifEmpty { null })))
                },
            )
        }
    }
}

@Composable
private fun PullRequestListRow(
    row: ScopedPullRequestEntry,
    statusActions: List<String>,
    environmentLabel: String?,
    menuExpanded: Boolean,
    onMenuExpandedChange: (Boolean) -> Unit,
    onOpen: () -> Unit,
    onMenuSelect: (String) -> Unit,
) {
    val entry = row.entry
    val status = pullRequestStatus(entry.state, entry.isDraft)
    val conflict = pullRequestConflict(entry.state, entry.isDraft, entry.mergeability, entry.baseBranch)
    val now = remember(entry.updatedAt) { System.currentTimeMillis() }
    S5Card(modifier = Modifier.fillMaxWidth(), onClick = onOpen) {
        Row(
            Modifier.padding(start = S5Theme.spacing.large, top = S5Theme.spacing.medium, bottom = S5Theme.spacing.medium),
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            Box(Modifier.padding(top = 2.dp)) {
                Icon(status.icon, contentDescription = status.label, tint = status.tone.color(), modifier = Modifier.size(20.dp))
                if (conflict != null) {
                    Box(
                        Modifier.align(Alignment.BottomEnd)
                            .offset(x = 3.dp, y = 3.dp)
                            .size(9.dp)
                            .clip(CircleShape)
                            .background(S5Theme.status.failed)
                    )
                }
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
                ) {
                    PullRequestAvatar(entry.author, size = 16.dp)
                    Text(
                        "${entry.repository} #${entry.number}" + (environmentLabel?.let { " · $it" } ?: ""),
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    entry.stack?.let { stack -> S5TextBadge("${stack.position}/${stack.size}") }
                    Box(Modifier.weight(0.01f))
                    Text(
                        relativeLabel(entry.updatedAt, now),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Text(
                    entry.title,
                    style = MaterialTheme.typography.titleSmallEmphasized,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                PullRequestRowMeta(row, conflict)
            }
            S5OverflowMenu(
                icon = Icons.Rounded.MoreVert,
                label = "Pull request actions",
                expanded = menuExpanded,
                onExpandedChange = onMenuExpandedChange,
                options =
                    listOf(
                        S5MenuOption("open", "Open pull request", icon = Icons.Rounded.Visibility),
                        S5MenuOption("share", "Share…", icon = Icons.Rounded.Share),
                        S5MenuOption("copy", "Copy link", icon = Icons.Rounded.ContentCopy),
                        S5MenuOption("provider", openOnProviderLabel(entry.provider), icon = Icons.AutoMirrored.Rounded.OpenInNew),
                    ) +
                        statusActions.map { action ->
                            S5MenuOption(
                                action,
                                pullRequestActionLabel(action),
                                icon = STATUS_ACTION_ICONS[action],
                                destructive = action == "close",
                            )
                        },
                onSelect = onMenuSelect,
            )
        }
    }
}

@Composable
private fun PullRequestRowMeta(row: ScopedPullRequestEntry, conflict: String?) {
    val entry = row.entry
    val review = pullRequestListReviewStatus(entry.reviewDecision, entry.viewerReviewRequested)
    val hasStats = entry.additions + entry.deletions > 0
    if (entry.labels.isEmpty() && entry.checksState == null && review == null && !hasStats && conflict == null) return
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        entry.labels.take(2).forEach { label ->
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                Box(
                    Modifier.size(8.dp)
                        .clip(CircleShape)
                        .background(pullRequestLabelColor(label.color)?.let { Color(it) } ?: MaterialTheme.colorScheme.outline)
                )
                Text(
                    label.name,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
            }
        }
        if (entry.labels.size > 2) {
            Text("+${entry.labels.size - 2}", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        if (conflict != null) {
            Text(conflict, style = MaterialTheme.typography.labelSmall, color = S5Theme.status.failed, maxLines = 1)
        }
        entry.checksState?.let { state ->
            Icon(
                when (state) {
                    "passing" -> Icons.Rounded.CheckCircle
                    "failing" -> Icons.Rounded.Cancel
                    else -> Icons.Rounded.Schedule
                },
                contentDescription = "Checks $state",
                tint = pullRequestChecksStateTone(state).color(),
                modifier = Modifier.size(14.dp),
            )
        }
        review?.let {
            Text(
                it.label,
                style = MaterialTheme.typography.labelSmall,
                color =
                    when (it) {
                        PullRequestListReview.Changes -> PullRequestTone.Danger
                        PullRequestListReview.Approved -> PullRequestTone.Success
                        PullRequestListReview.YourReview -> PullRequestTone.Accent
                        PullRequestListReview.Required -> PullRequestTone.Muted
                    }.color(),
                maxLines = 1,
            )
        }
        if (hasStats) {
            Text("+${entry.additions}", style = S5Theme.code.code, color = S5Theme.status.added)
            Text("−${entry.deletions}", style = S5Theme.code.code, color = S5Theme.status.removed)
        }
    }
}
