package club.touchtech.s5code.kotlin.feature.pullrequests

import androidx.compose.foundation.clickable
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Chat
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.rounded.AddComment
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.Difference
import androidx.compose.material.icons.rounded.FilterList
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.RadioButtonUnchecked
import androidx.compose.material.icons.rounded.RateReview
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.app.Routes
import club.touchtech.s5code.kotlin.data.PullRequestDiffAnnotation
import club.touchtech.s5code.kotlin.data.PullRequestDiffPage
import club.touchtech.s5code.kotlin.data.PullRequestFileAnnotations
import club.touchtech.s5code.kotlin.data.PullRequestQuery
import club.touchtech.s5code.kotlin.data.PullRequestReference
import club.touchtech.s5code.kotlin.data.annotatePullRequestFile
import club.touchtech.s5code.kotlin.data.combinePullRequestDiffPages
import club.touchtech.s5code.kotlin.data.filterPullRequestTestFiles
import club.touchtech.s5code.kotlin.data.pullRequestReviewKey
import club.touchtech.s5code.kotlin.data.retainPullRequestDiffPage
import club.touchtech.s5code.kotlin.data.reviewPosition
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5FileIcon
import club.touchtech.s5code.kotlin.design.component.S5IconToggleButton
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.component.S5LoadingState
import club.touchtech.s5code.kotlin.design.component.S5MenuOption
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5OverflowMenu
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.rememberClipboardWriter
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.feature.review.ReviewFileCard
import club.touchtech.s5code.kotlin.model.ReviewFile
import club.touchtech.s5code.kotlin.transport.wire.PullRequestActivityDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDetailDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestFilesViewedResultDto
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.launch

/* The PR's changed files and their diff, ported from RN's `PullRequestFiles`,
 * `PullRequestFilesRouteScreen` and `PullRequestFileRouteScreen`. */

/** `usePullRequestDiff`: the patch in pages over HTTP, combined and parsed. */
@Stable
class PullRequestDiffState internal constructor(private val load: suspend (String?) -> Unit) {
    var pages by mutableStateOf<List<PullRequestDiffPage>>(emptyList())
        internal set
    var loading by mutableStateOf(false)
        internal set
    var error by mutableStateOf<String?>(null)
        internal set

    val nextCursor: String? get() = pages.lastOrNull()?.result?.nextCursor
    val loaded: Boolean get() = pages.isNotEmpty()

    internal suspend fun fetch(cursor: String?) {
        if (loading) return
        loading = true
        error = null
        try {
            load(cursor)
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: Exception) {
            error = failure.message ?: "Couldn't load the changes."
        } finally {
            loading = false
        }
    }
}

@Composable
fun rememberPullRequestDiff(
    store: AppStore,
    environmentId: String,
    reference: PullRequestReference,
    commit: String?,
): Pair<PullRequestDiffState, (cursor: String?) -> Unit> {
    val scope = rememberCoroutineScope()
    val state =
        remember(environmentId, reference, commit) {
            lateinit var holder: PullRequestDiffState
            holder =
                PullRequestDiffState { cursor ->
                    val result = store.pullRequests.diff(environmentId, reference, cursor, commit)
                    holder.pages = retainPullRequestDiffPage(holder.pages, cursor, result)
                }
            holder
        }
    LaunchedEffect(state) { state.fetch(null) }
    return state to { cursor -> scope.launch { state.fetch(cursor) } }
}

/** `usePullRequestFilesViewed`: host or environment viewed marks, with presses shown at once. */
@Stable
class PullRequestFilesViewed internal constructor(
    val supported: Boolean,
    val canToggle: Boolean,
    private val viewedPaths: Set<String>,
    private val overlay: Map<String, Boolean>,
    val error: String?,
    private val onToggle: (String, Boolean) -> Unit,
) {
    fun isViewed(path: String): Boolean = overlay[path] ?: (path in viewedPaths)

    fun toggle(path: String) {
        if (canToggle) onToggle(path, !isViewed(path))
    }
}

@Composable
fun rememberPullRequestFilesViewed(
    store: AppStore,
    environmentId: String,
    reference: PullRequestReference,
    pr: PullRequestDetailDto?,
): PullRequestFilesViewed {
    val scope = rememberCoroutineScope()
    val supported = pr?.capabilities?.viewedFiles != null
    val permissions = rememberPullRequestPermissions(store, environmentId, pr)
    val query by remember(environmentId, reference, supported) {
            if (supported) store.pullRequests.filesViewed(environmentId, reference) else flowOf(PullRequestQuery())
        }
        .collectAsStateWithLifecycle(PullRequestQuery<PullRequestFilesViewedResultDto>())
    val overlay by remember(environmentId, reference) { store.pullRequests.filesViewedOverlay(environmentId, reference) }
        .collectAsStateWithLifecycle()
    val viewedPaths =
        remember(query.value) { query.value?.files.orEmpty().filter { it.state == "viewed" }.mapTo(HashSet()) { it.path } }
    var error by remember(environmentId, reference) { mutableStateOf<String?>(null) }
    val viewed =
        PullRequestFilesViewed(
            supported = supported,
            canToggle = supported && permissions.canWrite && query.hasValue,
            viewedPaths = viewedPaths,
            overlay = overlay,
            error = error,
        ) { path, next ->
            error = null
            scope.launch {
                try {
                    store.pullRequests.setFileViewed(environmentId, reference, path, next)
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (failure: Exception) {
                    error = failure.message ?: "Could not update viewed files."
                }
            }
        }
    return viewed
}

private fun fileCountLabel(count: Int) = if (count == 1) "1 file" else "$count files"

/** The changed files, grouped by directory; tapping one opens its diff. */
@Composable
fun PullRequestFilesScreen(
    store: AppStore,
    environmentId: String,
    reference: PullRequestReference,
    commit: String?,
    onBack: () -> Unit,
    onOpenRoute: (String) -> Unit,
) {
    val detail = rememberPullRequestDetail(store, environmentId, reference).value
    val (diff, load) = rememberPullRequestDiff(store, environmentId, reference, commit)
    val combined = remember(diff.pages) { combinePullRequestDiffPages(diff.pages) }
    val preferences by store.preferences.collectAsStateWithLifecycle()
    val hideTests = preferences.pullRequestHideTestFiles
    val files = remember(combined, hideTests) { filterPullRequestTestFiles(combined.files, hideTests) }
    val hiddenCount = combined.files.size - files.size
    val viewed = rememberPullRequestFilesViewed(store, environmentId, reference, if (commit == null) detail else null)
    val groups = remember(files) { files.groupBy { it.path.substringBeforeLast('/', "") } }
    var menuOpen by remember { mutableStateOf(false) }

    S5Screen(
        title = "Changed files",
        subtitle = "#${reference.number} · " + (commit?.take(7) ?: reference.repository),
        onBack = onBack,
        loading = diff.loading && diff.loaded,
        actions = {
            S5OverflowMenu(
                icon = Icons.Rounded.FilterList,
                label = "Filter review files",
                options = listOf(S5MenuOption("hide-tests", "Hide test files", selected = hideTests)),
                onSelect = { store.updatePreferences { it.copy(pullRequestHideTestFiles = !it.pullRequestHideTestFiles) } },
                expanded = menuOpen,
                onExpandedChange = { menuOpen = it },
            )
        },
    ) { padding ->
        when {
            !diff.loaded && diff.error != null ->
                S5EmptyState(
                    icon = Icons.Rounded.Difference,
                    title = "Couldn't load the changes",
                    detail = diff.error.orEmpty(),
                    modifier = Modifier.padding(padding),
                    actionLabel = "Try again",
                    onAction = { load(null) },
                )
            !diff.loaded -> S5LoadingState("Loading changed files", Modifier.padding(padding))
            else ->
                LazyColumn(
                    Modifier.fillMaxSize().padding(padding),
                    contentPadding = PaddingValues(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.small),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
                ) {
                    item(key = "summary") {
                        Text(
                            buildString {
                                append("${files.size} ${if (hiddenCount > 0) "visible" else "changed"} ")
                                append(if (files.size == 1) "file" else "files")
                                if (viewed.supported) append(" · ${files.count { viewed.isViewed(it.path) }} viewed")
                                append(" · ")
                                append(
                                    if (hideTests) "$hiddenCount test ${if (hiddenCount == 1) "file" else "files"} hidden"
                                    else "Test files included"
                                )
                            },
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(bottom = S5Theme.spacing.small),
                        )
                    }
                    if (files.isEmpty()) {
                        item(key = "empty") {
                            Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                            if (hiddenCount > 0) {
                                S5Notice(
                                    icon = Icons.Rounded.FilterList,
                                    text =
                                        (if (diff.nextCursor != null) "No non-test files loaded yet" else "Only test files changed") +
                                            ". Test files are hidden from this review.",
                                )
                                S5Button(
                                    text = "Show test files",
                                    style = S5ButtonStyle.Text,
                                    onClick = { store.updatePreferences { it.copy(pullRequestHideTestFiles = false) } },
                                )
                            } else {
                                Text("No text changes available.", style = MaterialTheme.typography.bodyMedium)
                            }
                            }
                        }
                    }
                    groups.forEach { (directory, entries) ->
                        item(key = "dir:$directory") {
                            Text(
                                directory.ifEmpty { "Repository root" },
                                style = S5Theme.code.codeSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier = Modifier.padding(top = S5Theme.spacing.medium, bottom = S5Theme.spacing.tiny),
                            )
                        }
                        items(entries, key = { "file:${it.path}" }) { file ->
                            ChangedFileRow(file, viewed.isViewed(file.path)) {
                                onOpenRoute(Routes.pullRequestDiff(environmentId, reference, path = file.path, commit = commit))
                            }
                        }
                    }
                    if (diff.nextCursor != null || (diff.error != null && diff.loaded)) {
                        item(key = "more") {
                            Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                                if (diff.loading) {
                                    S5InlineLoading()
                                } else {
                                    S5Button(
                                        text = if (diff.error != null) "Retry loading files" else "Load more files",
                                        style = S5ButtonStyle.Text,
                                        onClick = { load(diff.nextCursor) },
                                    )
                                }
                            }
                        }
                    }
                    viewed.error?.let { item(key = "viewed-error") { Text(it, color = MaterialTheme.colorScheme.error) } }
                    combined.notice?.let { notice ->
                        item(key = "notice") {
                            Text(notice, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
        }
    }
}

@Composable
private fun ChangedFileRow(file: ReviewFile, viewed: Boolean, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(vertical = S5Theme.spacing.small),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
    ) {
        S5FileIcon(file.path, size = 20.dp)
        Column(Modifier.weight(1f)) {
            Text(file.path.substringAfterLast('/'), style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            file.oldPath?.let {
                Text(
                    "Renamed from ${it.substringAfterLast('/')}",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                )
            }
        }
        Text("+${file.additions}", style = S5Theme.code.inlineTechnical, color = S5Theme.status.added)
        Text("−${file.deletions}", style = S5Theme.code.inlineTechnical, color = S5Theme.status.removed)
        if (viewed) Icon(Icons.Rounded.CheckCircle, contentDescription = "Viewed", tint = S5Theme.status.settled, modifier = Modifier.size(18.dp))
        Icon(
            Icons.AutoMirrored.Rounded.KeyboardArrowRight,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(16.dp),
        )
    }
}

/** A discussion or pending comment drawn in the diff. */
@Composable
private fun DiffAnnotationCard(annotation: PullRequestDiffAnnotation, onClick: () -> Unit) {
    S5Card(tone = S5CardTone.Receded, onClick = onClick, modifier = Modifier.padding(S5Theme.spacing.small)) {
        Column(Modifier.padding(S5Theme.spacing.medium), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                Icon(
                    if (annotation.id.startsWith("draft:")) Icons.Rounded.RateReview else Icons.AutoMirrored.Rounded.Chat,
                    contentDescription = null,
                    modifier = Modifier.size(14.dp),
                )
                Text(annotation.title, style = MaterialTheme.typography.labelLarge)
            }
            Text(annotation.body, style = MaterialTheme.typography.bodySmall, maxLines = 4, overflow = TextOverflow.Ellipsis)
            Text(annotation.footer, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/**
 * The diff of every changed file — or one commit's — starting at [path].
 * Tapping a line drafts a review comment there; discussions and drafts sit
 * under their lines.
 */
@Composable
fun PullRequestDiffScreen(
    store: AppStore,
    environmentId: String,
    reference: PullRequestReference,
    path: String?,
    commit: String?,
    replacingDraftId: String?,
    confirmController: S5ConfirmDialogController,
    onBack: () -> Unit,
    onOpenRoute: (String) -> Unit,
    onOpenThread: (environmentId: String, threadId: String) -> Unit,
    onOpenNewTask: () -> Unit,
) {
    val copy = rememberClipboardWriter()
    val detailQuery = rememberPullRequestDetail(store, environmentId, reference)
    LaunchedEffect(environmentId, reference) { runCatching { store.pullRequests.refreshDetail(environmentId, reference) } }
    val detail = detailQuery.value
    val permissions = rememberPullRequestPermissions(store, environmentId, detail)
    val activity by remember(environmentId, reference) { store.pullRequests.activity(environmentId, reference) }
        .collectAsStateWithLifecycle(PullRequestQuery<PullRequestActivityDto>())
    val drafts by store.reviewDrafts.drafts.collectAsStateWithLifecycle()
    val reviewDraft = drafts[pullRequestReviewKey(environmentId, reference)]
    val (diff, load) = rememberPullRequestDiff(store, environmentId, reference, commit)
    val combined = remember(diff.pages) { combinePullRequestDiffPages(diff.pages) }
    val preferences by store.preferences.collectAsStateWithLifecycle()
    val hideTests = preferences.pullRequestHideTestFiles
    // The file opened from the list stays visible even when it is a hidden test file.
    val files = remember(combined, hideTests, path) { filterPullRequestTestFiles(combined.files, hideTests, path) }
    val hiddenCount = combined.files.size - files.size
    val viewed = rememberPullRequestFilesViewed(store, environmentId, reference, if (commit == null) detail else null)
    var commitViewed by remember(commit) { mutableStateOf<Set<String>>(emptySet()) }

    // The revision the diff was loaded at; comments only anchor to it.
    var diffHead by remember(environmentId, reference) { mutableStateOf<String?>(null) }
    if (diffHead == null && detail?.headSha != null) diffHead = detail.headSha
    val staleDiff = commit == null && diffHead != null && detail?.headSha != null && diffHead != detail.headSha
    val canCommentOnDiff =
        permissions.canInlineComment && commit == null && !staleDiff && detail != null &&
            !detailQuery.loading && diff.loaded && diff.error == null

    val threads = if (staleDiff || commit != null) emptyList() else activity.value?.reviewThreads.orEmpty()
    val pending = if (commit != null) emptyList() else reviewDraft?.comments.orEmpty()
    val annotations =
        remember(files, threads, pending, diffHead) {
            files.associate { it.path to annotatePullRequestFile(it, threads, pending, diffHead) }
        }
    var collapsed by remember(commit) { mutableStateOf<Set<String>>(emptySet()) }
    var selected by remember { mutableStateOf<Pair<String, Int>?>(null) }
    val listState = rememberLazyListState()
    val headerItems = 1
    val visiblePath by remember(files) {
        derivedStateOf { files.getOrNull(listState.firstVisibleItemIndex - headerItems)?.path ?: files.firstOrNull()?.path }
    }
    val agent =
        rememberPullRequestAgent(store, environmentId, reference, detail, confirmController, onOpenThread, onOpenNewTask, visiblePath)
    var jumped by rememberSaveable(path, commit) { mutableStateOf(path == null) }
    val target = path?.let { wanted -> files.indexOfFirst { it.path == wanted } }?.takeIf { it >= 0 }
    LaunchedEffect(target, jumped) {
        if (!jumped && target != null) {
            listState.scrollToItem(target + headerItems)
            jumped = true
        }
    }
    // Keep paging until the requested file arrives, then as the end comes into view.
    val nearEnd by remember(files) {
        derivedStateOf { (listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0) >= files.size + headerItems - 3 }
    }
    val seeking = path != null && target == null && combined.files.none { it.path == path } && diff.nextCursor != null
    LaunchedEffect(seeking, nearEnd, diff.nextCursor, diff.loading, diff.error) {
        if ((seeking || nearEnd) && diff.nextCursor != null && !diff.loading && diff.error == null) load(diff.nextCursor)
    }

    fun commentOn(file: ReviewFile, lineIndex: Int) {
        val head = diffHead ?: return
        if (!canCommentOnDiff) return
        val line = file.hunks.flatMap { it.lines }.getOrNull(lineIndex) ?: return
        val position = reviewPosition(line) ?: return
        onOpenRoute(
            Routes.pullRequestInlineComment(
                environmentId,
                reference,
                path = file.path,
                oldPath = file.oldPath,
                position = position,
                headSha = head,
                code = line.text,
                draftId = replacingDraftId,
            )
        )
    }

    fun openAnnotation(annotation: PullRequestDiffAnnotation) {
        when {
            annotation.id.startsWith("discussion:") ->
                onOpenRoute(Routes.pullRequestDiscussion(environmentId, reference, annotation.id.removePrefix("discussion:")))
            annotation.id.startsWith("draft:") -> {
                val draft = pending.firstOrNull { it.id == annotation.id.removePrefix("draft:") } ?: return
                onOpenRoute(
                    Routes.pullRequestInlineComment(
                        environmentId,
                        reference,
                        path = draft.path,
                        oldPath = draft.oldPath,
                        position = draft.position,
                        headSha = draft.headSha,
                        code = draft.code,
                        draftId = draft.id,
                    )
                )
            }
        }
    }

    val selectedLine =
        selected?.let { (file, index) -> files.firstOrNull { it.path == file }?.hunks?.flatMap { it.lines }?.getOrNull(index) }
    var menuOpen by remember { mutableStateOf(false) }
    val title = if (commit != null) "Commit changes" else "Changes"

    S5Screen(
        title = title,
        subtitle =
            "${commit?.take(7) ?: "#${reference.number}"} · ${fileCountLabel(files.size)}" +
                if (hideTests && hiddenCount > 0) " · $hiddenCount ${if (hiddenCount == 1) "test" else "tests"} hidden" else "",
        onBack = onBack,
        loading = diff.loading && diff.loaded,
        bottomBar = {
            if (detail != null) {
                PullRequestActionBar(
                    agent = agent,
                    permissions = permissions,
                    pendingComments = reviewDraft?.comments?.size ?: 0,
                    onOpenReview = { onOpenRoute(Routes.pullRequestReview(environmentId, reference, "review")) },
                    onOpenFiles = { onOpenRoute(Routes.pullRequestFiles(environmentId, reference, commit)) },
                )
            }
        },
        actions = {
            S5OverflowMenu(
                icon = Icons.Rounded.MoreVert,
                label = "Diff options",
                options =
                    buildList {
                        add(S5MenuOption("hide-tests", "Hide test files", Icons.Rounded.FilterList, selected = hideTests))
                        if (permissions.verdicts.isNotEmpty()) add(S5MenuOption("submit-review", "Submit review", Icons.Rounded.RateReview))
                        if (permissions.canComment) add(S5MenuOption("comment-on-pr", "Add comment", Icons.Rounded.AddComment))
                        if (selectedLine != null) {
                            if (canCommentOnDiff) {
                                add(
                                    S5MenuOption(
                                        "comment",
                                        if (replacingDraftId != null) "Move draft to this line" else "Add review comment",
                                        Icons.AutoMirrored.Rounded.Chat,
                                    )
                                )
                            }
                            add(S5MenuOption("copy", "Copy line", Icons.Rounded.ContentCopy))
                        }
                    },
                onSelect = { id ->
                    when (id) {
                        "hide-tests" ->
                            store.updatePreferences { it.copy(pullRequestHideTestFiles = !it.pullRequestHideTestFiles) }
                        "submit-review" -> onOpenRoute(Routes.pullRequestReview(environmentId, reference, "review"))
                        "comment-on-pr" -> onOpenRoute(Routes.pullRequestReview(environmentId, reference, "comment"))
                        "comment" ->
                            selected?.let { (file, index) -> files.firstOrNull { it.path == file }?.let { commentOn(it, index) } }
                        "copy" -> selectedLine?.let { copy(it.text) }
                    }
                },
                expanded = menuOpen,
                onExpandedChange = { menuOpen = it },
            )
        },
    ) { padding ->
        when {
            !diff.loaded && diff.error != null ->
                S5EmptyState(
                    icon = Icons.Rounded.Difference,
                    title = "Couldn't load the changes",
                    detail = diff.error.orEmpty(),
                    modifier = Modifier.padding(padding),
                    actionLabel = "Try again",
                    onAction = { load(null) },
                )
            !diff.loaded || seeking -> S5LoadingState("Loading changes", Modifier.padding(padding))
            files.isEmpty() && hiddenCount > 0 ->
                S5EmptyState(
                    icon = Icons.Rounded.FilterList,
                    title = "Only test files changed",
                    detail = "Test files are hidden from this review.",
                    modifier = Modifier.padding(padding),
                    actionLabel = "Show test files",
                    onAction = { store.updatePreferences { it.copy(pullRequestHideTestFiles = false) } },
                )
            files.isEmpty() ->
                S5EmptyState(
                    icon = Icons.Rounded.Difference,
                    title = "Diff unavailable",
                    detail = "Open this pull request on its provider to view the changes.",
                    modifier = Modifier.padding(padding),
                )
            else ->
                LazyColumn(
                    state = listState,
                    modifier = Modifier.fillMaxSize().padding(padding),
                    contentPadding =
                        PaddingValues(start = S5Theme.spacing.gutter, end = S5Theme.spacing.gutter, bottom = 120.dp),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    item(key = "notices") {
                        Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                            if (replacingDraftId != null) {
                                Text(
                                    "Select a current line, then choose “Move draft to this line” from its menu.",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                            viewed.error?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
                            if (staleDiff) {
                                Row(verticalAlignment = Alignment.CenterVertically) {
                                    Text(
                                        "The branch changed. Reload before adding line comments.",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        modifier = Modifier.weight(1f),
                                    )
                                    S5Button(
                                        text = "Reload changes",
                                        style = S5ButtonStyle.Text,
                                        icon = Icons.Rounded.Refresh,
                                        onClick = {
                                            load(null)
                                            diffHead = detail?.headSha
                                            selected = null
                                            collapsed = emptySet()
                                        },
                                    )
                                }
                            }
                        }
                    }
                    items(files, key = { it.path }) { file ->
                        val fileAnnotations = annotations[file.path] ?: PullRequestFileAnnotations()
                        val isViewed = if (commit != null) file.path in commitViewed else viewed.isViewed(file.path)
                        ReviewFileCard(
                            file = file,
                            expanded = file.path !in collapsed,
                            selectedRange = selected?.takeIf { it.first == file.path }?.let { it.second..it.second },
                            rangeAnchorIndex = null,
                            onLineTap = { _, index ->
                                selected = file.path to index
                                commentOn(file, index)
                            },
                            onLineLongPress = { _, index ->
                                selected = file.path to index
                                menuOpen = true
                            },
                            onToggle = {
                                collapsed = if (file.path in collapsed) collapsed - file.path else collapsed + file.path
                            },
                            headerTrailing =
                                if (commit != null || viewed.supported) {
                                    {
                                        S5IconToggleButton(
                                            icon = if (isViewed) Icons.Rounded.CheckCircle else Icons.Rounded.RadioButtonUnchecked,
                                            label = "Viewed",
                                            checked = isViewed,
                                            enabled = commit != null || viewed.canToggle,
                                            onCheckedChange = {
                                                // Marking viewed folds the file away, as on the host.
                                                collapsed = if (isViewed) collapsed - file.path else collapsed + file.path
                                                if (commit != null) {
                                                    commitViewed = if (isViewed) commitViewed - file.path else commitViewed + file.path
                                                } else {
                                                    viewed.toggle(file.path)
                                                }
                                            },
                                        )
                                    }
                                } else {
                                    null
                                },
                            belowHeader =
                                if (fileAnnotations.file.isEmpty()) null
                                else {
                                    { fileAnnotations.file.forEach { DiffAnnotationCard(it) { openAnnotation(it) } } }
                                },
                            afterLine =
                                if (fileAnnotations.lines.isEmpty()) null
                                else {
                                    { index -> fileAnnotations.lines[index]?.forEach { DiffAnnotationCard(it) { openAnnotation(it) } } }
                                },
                            unavailableText = "This file's changes are unavailable here.",
                        )
                    }
                    item(key = "footer") {
                        Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.fillMaxWidth()) {
                            if (diff.loading && diff.nextCursor != null) {
                                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                                    S5InlineLoading()
                                    Text("Loading more files…", style = MaterialTheme.typography.bodySmall)
                                }
                            }
                            if (diff.error != null && diff.nextCursor != null) {
                                S5Button(text = "Retry loading remaining files", style = S5ButtonStyle.Text, onClick = { load(diff.nextCursor) })
                            }
                            combined.notice?.let {
                                Text(it, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    }
                }
        }
    }
}
