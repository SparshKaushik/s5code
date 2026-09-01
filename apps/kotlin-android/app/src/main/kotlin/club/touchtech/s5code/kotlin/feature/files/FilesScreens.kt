package club.touchtech.s5code.kotlin.feature.files

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.automirrored.rounded.ArrowForward
import androidx.compose.material.icons.automirrored.rounded.InsertDriveFile
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.Folder
import androidx.compose.material.icons.rounded.FolderOpen
import androidx.compose.material.icons.rounded.Image
import androidx.compose.material.icons.rounded.Public
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.automirrored.rounded.WrapText
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import coil3.compose.AsyncImage
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.app.Routes
import club.touchtech.s5code.kotlin.data.Remote
import club.touchtech.s5code.kotlin.data.WorkspaceImageCache
import club.touchtech.s5code.kotlin.data.WorkspaceImageMetadata
import club.touchtech.s5code.kotlin.data.cacheSizeLabel
import club.touchtech.s5code.kotlin.data.rememberRetryableRemote
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5ErrorState
import club.touchtech.s5code.kotlin.design.component.S5FileIcon
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5IconToggleButton
import club.touchtech.s5code.kotlin.design.component.S5LoadingState
import club.touchtech.s5code.kotlin.design.component.S5Markdown
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5PullToRefreshBox
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SearchField
import club.touchtech.s5code.kotlin.design.component.S5ShapeBadge
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.rememberClipboardWriter
import club.touchtech.s5code.kotlin.design.component.rememberHighlightedLines
import club.touchtech.s5code.kotlin.design.text.codeLanguageOfPath
import club.touchtech.s5code.kotlin.design.theme.S5MaterialShapes
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.FileNode
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.platform.WorkspaceWebAction
import club.touchtech.s5code.kotlin.platform.WorkspaceWebCommand
import club.touchtech.s5code.kotlin.platform.WorkspaceWebNavigation
import club.touchtech.s5code.kotlin.platform.WorkspaceWebView

/**
 * Workspace file tree. Directories expand in place; files route to the viewer
 * that matches their type.
 */
@Composable
fun FilesTreeScreen(
    store: AppStore,
    environmentId: String,
    threadId: String,
    onBack: () -> Unit,
    onOpenFile: (String) -> Unit,
) {
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val id = remember(threadId) { ThreadId(threadId) }
    // One read per (environment, thread), retryable: a dropped socket mid-read is
    // common and re-navigating to recover from it would be silly.
    val (state, retry) =
        rememberRetryableRemote(environmentId, threadId) { store.workspace.files(env, id) }
    val root = state.value
    var expanded by remember(environmentId, threadId) { mutableStateOf(setOf("")) }
    var query by remember(environmentId, threadId) { mutableStateOf("") }

    S5Screen(
        title = "Files",
        subtitle = root.valueOrNull?.name ?: "",
        prominence = S5TopBarProminence.Compact,
        onBack = onBack,
        loading = root is Remote.Loading,
        actions = {
            S5IconButton(icon = Icons.Rounded.Refresh, label = "Refresh", onClick = retry)
        },
    ) { padding ->
        when (root) {
            is Remote.Loading -> Box(Modifier.fillMaxSize().padding(padding))
            is Remote.Failed ->
                Box(Modifier.padding(padding).padding(S5Theme.spacing.gutter)) {
                    S5ErrorState(title = "Couldn't read files", detail = root.message, onRetry = retry)
                }
            is Remote.Loaded -> {
                val rows =
                    remember(root.value, expanded, query) {
                        flattenFileTree(root.value, expanded, query)
                    }
                LaunchedEffect(rows, query) {
                    if (query.isNotBlank()) {
                        rows.asSequence().map(TreeRow::node).filterNot(FileNode::isDirectory)
                            .take(SEARCH_PREWARM_LIMIT)
                            .forEach { store.workspace.prewarmFile(env, id, it.path) }
                    }
                }
                Column(Modifier.fillMaxSize().padding(padding)) {
                    S5SearchField(
                        value = query,
                        onValueChange = { query = it },
                        placeholder = "Search files",
                        modifier =
                            Modifier.fillMaxWidth()
                                .padding(
                                    horizontal = S5Theme.spacing.gutter,
                                    vertical = S5Theme.spacing.small,
                                ),
                    )
                    if (root.value.truncated) {
                        S5Notice(
                            icon = Icons.Rounded.Folder,
                            text = "This workspace is very large. Search is limited to the indexed entries returned by the server.",
                            containerColor = MaterialTheme.colorScheme.surfaceContainer,
                            contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier =
                                Modifier.fillMaxWidth()
                                    .padding(horizontal = S5Theme.spacing.gutter),
                        )
                    }
                    if (rows.isEmpty()) {
                        S5EmptyState(
                            icon = Icons.Rounded.Folder,
                            title = if (query.isBlank()) "Empty workspace" else "No files found",
                            detail =
                                if (query.isBlank()) "Nothing to browse in this project yet."
                                else "Try a different path or file name.",
                            modifier = Modifier.fillMaxSize(),
                        )
                    } else {
                        S5PullToRefreshBox(
                            isRefreshing = false,
                            onRefresh = retry,
                            modifier = Modifier.fillMaxWidth().weight(1f),
                        ) {
                            LazyColumn(
                                Modifier.fillMaxSize(),
                                contentPadding = PaddingValues(bottom = 32.dp),
                            ) {
                                items(rows, key = { it.node.path }) { row ->
                                    FileRow(
                                        row = row,
                                        onClick = {
                                            if (row.node.isDirectory) {
                                                expanded =
                                                    if (row.node.path in expanded) {
                                                        expanded - row.node.path
                                                    } else {
                                                        expanded + row.node.path
                                                    }
                                            } else {
                                                store.workspace.prewarmFile(
                                                    env,
                                                    id,
                                                    row.node.path,
                                                )
                                                onOpenFile(
                                                    "${Routes.fileRouteSuffix(row.node.path)}?path=${android.net.Uri.encode(row.node.path)}"
                                                )
                                            }
                                        },
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

internal data class TreeRow(val node: FileNode, val depth: Int, val expanded: Boolean)

internal fun flattenFileTree(
    root: FileNode,
    expanded: Set<String>,
    rawQuery: String = "",
): List<TreeRow> {
    val tokens =
        rawQuery.trim().lowercase().split(Regex("[\\s/\\\\._-]+"))
            .filter(String::isNotEmpty)
    val searching = tokens.isNotEmpty()

    fun matches(node: FileNode): Boolean {
        val segments = node.path.lowercase().split('/').filter(String::isNotEmpty)
        val words = segments.flatMap(::splitFileSearchWords)
        return tokens.all { token ->
            segments.any { value -> fileSearchMatch(value, token, fuzzy = false) } ||
                words.any { value -> fileSearchMatch(value, token, fuzzy = true) }
        }
    }

    fun flatten(node: FileNode, depth: Int): Pair<Boolean, List<TreeRow>> {
        val ownMatch = searching && matches(node)
        val childRows = mutableListOf<TreeRow>()
        var descendantMatch = false
        if (node.isDirectory && (node.path in expanded || searching)) {
            sortedFileNodes(node.children).forEach { child ->
                val (childMatches, visibleChildren) = flatten(child, depth + 1)
                if (childMatches) descendantMatch = true
                childRows += visibleChildren
            }
        }
        val visible = !searching || ownMatch || descendantMatch
        if (!visible) return false to emptyList()
        return (ownMatch || descendantMatch) to
            buildList {
                add(TreeRow(node, depth, node.path in expanded || searching))
                addAll(childRows)
            }
    }

    return sortedFileNodes(root.children).flatMap { child -> flatten(child, 0).second }
}

private const val SEARCH_PREWARM_LIMIT = 8

private fun sortedFileNodes(nodes: List<FileNode>) =
    nodes.sortedWith(
        compareByDescending<FileNode> { it.isDirectory }
            .thenBy(String.CASE_INSENSITIVE_ORDER) { it.name }
    )

private fun splitFileSearchWords(value: String): List<String> =
    value.replace(Regex("([A-Z]+)([A-Z][a-z])"), "$1 $2")
        .replace(Regex("([a-z0-9])([A-Z])"), "$1 $2")
        .split(Regex("[^A-Za-z0-9]+"))
        .filter(String::isNotEmpty)
        .map(String::lowercase)

private fun fileSearchMatch(value: String, token: String, fuzzy: Boolean): Boolean {
    if (value == token || value.startsWith(token) || value.contains(token)) return true
    if (!fuzzy) return false
    var index = 0
    var previous = -1
    var gapCost = 0
    value.forEachIndexed { current, character ->
        if (index < token.length && character == token[index]) {
            if (previous >= 0) gapCost += current - previous - 1
            previous = current
            index += 1
        }
    }
    // Keep fuzzy matching useful for abbreviations without allowing a tiny token
    // to match almost every long word in a repository.
    return index == token.length && gapCost <= maxOf(3, token.length * 2)
}

@Composable
private fun FileRow(row: TreeRow, onClick: () -> Unit) {
    // Rows stay dense but keep the 48dp minimum target via defaultMinSize.
    Row(
        Modifier.fillMaxWidth()
            .clickable(onClick = onClick)
            .defaultMinSize(minHeight = 48.dp)
            .padding(
                start = S5Theme.spacing.gutter + (row.depth * 16).dp,
                end = S5Theme.spacing.gutter,
            ),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        if (row.node.isDirectory) {
            Icon(
                if (row.expanded) Icons.Rounded.FolderOpen else Icons.Rounded.Folder,
                contentDescription = null,
                modifier = Modifier.size(18.dp),
                tint = MaterialTheme.colorScheme.primary,
            )
        } else {
            S5FileIcon(
                path = row.node.path,
                modifier = Modifier.size(18.dp),
                size = 18.dp,
            )
        }
        Text(
            row.node.name,
            style = S5Theme.code.code,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        if (row.node.sizeLabel != null) {
            Text(
                row.node.sizeLabel,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** Syntax-highlighted, selectable, copyable source viewer. */
@Composable
fun FilePreviewScreen(
    store: AppStore,
    environmentId: String,
    threadId: String,
    path: String,
    onBack: () -> Unit,
) {
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val id = remember(threadId) { ThreadId(threadId) }
    val (state, retry) =
        rememberRetryableRemote(environmentId, threadId, path) {
            store.workspace.sourceFile(env, id, path)
        }
    val copy = rememberClipboardWriter()
    var wrap by remember { mutableStateOf(false) }
    val scroll = rememberScrollState()
    val file = state.value

    S5Screen(
        title = path.substringAfterLast('/'),
        subtitle =
            file.valueOrNull?.let { "${it.lines.size} lines · ${it.language}" } ?: path,
        prominence = S5TopBarProminence.Compact,
        onBack = onBack,
        actions = {
            S5IconToggleButton(
                icon = Icons.AutoMirrored.Rounded.WrapText,
                label = "Wrap long lines",
                checked = wrap,
                onCheckedChange = { wrap = it },
            )
            file.valueOrNull?.let { loaded ->
                S5IconButton(
                    icon = Icons.Rounded.ContentCopy,
                    label = "Copy file",
                    onClick = { copy(loaded.lines.joinToString("\n")) },
                )
            }
        },
    ) { padding ->
        when (file) {
            is Remote.Loading -> S5LoadingState("Reading the file…", Modifier.padding(padding))
            is Remote.Failed ->
                Box(Modifier.padding(padding).padding(S5Theme.spacing.gutter)) {
                    S5ErrorState(title = "Couldn't read this file", detail = file.message, onRetry = retry)
                }
            is Remote.Loaded -> {
                val loaded = file.value
                val language = remember(loaded.path) { codeLanguageOfPath(loaded.path) }
                val highlightedLines = rememberHighlightedLines(loaded.lines, language)
                Column(Modifier.fillMaxSize().padding(padding)) {
                    if (loaded.truncated) {
                        S5Notice(
                            icon = Icons.AutoMirrored.Rounded.InsertDriveFile,
                            text =
                                "Showing the first ${cacheSizeLabel(loaded.lines.joinToString("\n").toByteArray().size.toLong())} of ${cacheSizeLabel(loaded.byteLength)}.",
                            containerColor = MaterialTheme.colorScheme.tertiaryContainer,
                            contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
                            modifier =
                                Modifier.fillMaxWidth()
                                    .padding(horizontal = S5Theme.spacing.gutter),
                        )
                    }
                    SelectionContainer {
                        LazyColumn(
                            Modifier.fillMaxWidth().weight(1f),
                            contentPadding = PaddingValues(vertical = S5Theme.spacing.small),
                        ) {
                            items(loaded.lines.size, key = { it }) { index ->
                                val annotated = highlightedLines[index]
                                Row(
                                    Modifier.fillMaxWidth()
                                        .then(if (wrap) Modifier else Modifier.horizontalScroll(scroll))
                                        .padding(horizontal = S5Theme.spacing.small),
                                ) {
                                    Text(
                                        "${index + 1}".padStart(4),
                                        style = S5Theme.code.codeSmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                    Text(
                                        annotated,
                                        style = S5Theme.code.code,
                                        softWrap = wrap,
                                        modifier = Modifier.padding(start = S5Theme.spacing.small),
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/** Rendered Markdown preview of a real workspace file. */
@Composable
fun MarkdownPreviewScreen(
    store: AppStore,
    environmentId: String,
    threadId: String,
    path: String,
    onBack: () -> Unit,
    onOpenFile: (String) -> Unit,
) {
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val id = remember(threadId) { ThreadId(threadId) }
    val (state, retry) =
        rememberRetryableRemote(environmentId, threadId, path) {
            store.workspace.sourceFile(env, id, path)
        }
    val copy = rememberClipboardWriter()
    val detail by
        remember(environmentId, threadId) { store.workspace.thread(env, id) }
            .collectAsStateWithLifecycle()
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val workspaceRoot =
        remember(detail, projects, environmentId) {
            detail?.workspaceRoot
                ?: projects.firstOrNull { project ->
                    project.environmentId.value == environmentId &&
                        project.id == detail?.summary?.projectId
                }?.workspaceRoot
        }

    S5Screen(
        title = path.substringAfterLast('/'),
        subtitle = "Markdown preview",
        prominence = S5TopBarProminence.Compact,
        onBack = onBack,
    ) { padding ->
        when (val file = state.value) {
            is Remote.Loading -> S5LoadingState("Reading the file…", Modifier.padding(padding))
            is Remote.Failed ->
                Box(Modifier.padding(padding).padding(S5Theme.spacing.gutter)) {
                    S5ErrorState(title = "Couldn't read this file", detail = file.message, onRetry = retry)
                }
            is Remote.Loaded ->
                Box(
                    Modifier.fillMaxSize()
                        .padding(padding)
                        .verticalScroll(rememberScrollState())
                        .padding(S5Theme.spacing.gutter)
                ) {
                    S5Markdown(
                        source = file.value.lines.joinToString("\n"),
                        onCopyCode = copy,
                        workspaceRoot = workspaceRoot,
                        onOpenFile = onOpenFile,
                    )
                }
        }
    }
}

/**
 * Image preview.
 *
 * The bytes do not come over the RPC socket: `assets.createUrl` signs a
 * short-lived URL and the image is fetched over plain HTTP by the same OkHttp
 * stack the transport uses. That is what makes a 4 MB screenshot viewable at all
 * — base64 inside a JSON frame would be a third larger and would block the
 * socket while it streamed.
 *
 * Pinch-to-zoom is the reason this is not just an `AsyncImage`: a design mockup
 * or a diagram is unreadable at phone width, and the gesture is what the screen
 * is for. Double tap resets, because a two-finger gesture back to exactly 1x is
 * fiddly.
 */
@Composable
fun ImagePreviewScreen(
    store: AppStore,
    environmentId: String,
    threadId: String,
    path: String,
    onBack: () -> Unit,
    onOpenSource: () -> Unit,
) {
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val id = remember(threadId) { ThreadId(threadId) }
    val (state, retry) =
        rememberRetryableRemote(environmentId, threadId, path) {
            store.workspace.assetUrl(env, id, path)
        }
    val context = LocalContext.current
    var metadata by
        remember(environmentId, threadId, path) {
            mutableStateOf<WorkspaceImageMetadata?>(null)
        }
    var scale by remember(path) { mutableFloatStateOf(1f) }
    var offset by remember(path) { mutableStateOf(Offset.Zero) }

    S5Screen(
        title = path.substringAfterLast('/'),
        subtitle = path.substringBeforeLast('/', missingDelimiterValue = ""),
        prominence = S5TopBarProminence.Compact,
        onBack = onBack,
        actions = {
            S5IconButton(
                icon = Icons.AutoMirrored.Rounded.InsertDriveFile,
                label = "View as text",
                onClick = onOpenSource,
            )
        },
    ) { padding ->
        when (val url = state.value) {
            is Remote.Loading -> S5LoadingState("Preparing the image…", Modifier.padding(padding))
            is Remote.Failed ->
                Box(Modifier.padding(padding).padding(S5Theme.spacing.gutter)) {
                    S5ErrorState(
                        title = "Couldn't load this image",
                        detail = url.message,
                        onRetry = retry,
                    )
                }
            is Remote.Loaded -> {
                LaunchedEffect(url.value) {
                    metadata = runCatching { WorkspaceImageCache.preload(context, url.value) }.getOrNull()
                }
                Column(Modifier.fillMaxSize().padding(padding)) {
                    Box(
                        modifier =
                            Modifier.fillMaxWidth()
                                .weight(1f)
                                .background(MaterialTheme.colorScheme.surfaceContainerLowest)
                                .pointerInput(path) {
                                    detectTransformGestures { _, pan, zoom, _ ->
                                        scale = (scale * zoom).coerceIn(1f, 8f)
                                        // Panning is only meaningful while zoomed in; at 1x
                                        // the image is already fully visible and a drag
                                        // would just slide it off screen.
                                        offset = if (scale > 1f) offset + pan else Offset.Zero
                                    }
                                }
                                .pointerInput(path) {
                                    detectTapGestures(
                                        onDoubleTap = {
                                            scale = if (scale > 1f) 1f else 2f
                                            offset = Offset.Zero
                                        }
                                    )
                                },
                        contentAlignment = Alignment.Center,
                    ) {
                        AsyncImage(
                            model = url.value,
                            contentDescription = path.substringAfterLast('/'),
                            contentScale = ContentScale.Fit,
                            modifier =
                                Modifier.fillMaxSize()
                                    .graphicsLayer(
                                        scaleX = scale,
                                        scaleY = scale,
                                        translationX = offset.x,
                                        translationY = offset.y,
                                    ),
                        )
                    }
                    metadata?.let { details ->
                        Text(
                            text =
                                buildString {
                                    append("${details.width} × ${details.height}")
                                    details.byteLength?.let { append(" · ${cacheSizeLabel(it)}") }
                                },
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier =
                                Modifier.align(Alignment.CenterHorizontally)
                                    .padding(S5Theme.spacing.small),
                        )
                    }
                }
            }
        }
    }
}

/**
 * Web preview for HTML and PDF files.
 *
 * A `WebView` is the only way to render these, and it is locked down to the one
 * job it has: JavaScript is on because a built HTML report is usually useless
 * without it, but file and content access are off, so a page cannot read the app's
 * own storage, and navigation away from the signed asset origin is refused. The
 * URL itself is short-lived and signed, so a page cannot be reloaded later from a
 * copied link either.
 */
@Composable
fun WebPreviewScreen(
    store: AppStore,
    environmentId: String,
    threadId: String,
    path: String,
    onBack: () -> Unit,
    onOpenSource: () -> Unit,
) {
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val id = remember(threadId) { ThreadId(threadId) }
    val (state, retry) =
        rememberRetryableRemote(environmentId, threadId, path) {
            store.workspace.assetUrl(env, id, path)
        }
    var loadError by remember(path) { mutableStateOf<String?>(null) }
    var pageLoading by remember(path) { mutableStateOf(false) }
    var navigation by remember(path) { mutableStateOf(WorkspaceWebNavigation()) }
    var nextCommandId by remember(path) { mutableLongStateOf(0L) }
    var command by remember(path) { mutableStateOf<WorkspaceWebCommand?>(null) }

    fun dispatch(action: WorkspaceWebAction) {
        nextCommandId += 1
        command = WorkspaceWebCommand(nextCommandId, action)
        loadError = null
    }

    S5Screen(
        title = path.substringAfterLast('/'),
        subtitle = "Preview",
        prominence = S5TopBarProminence.Compact,
        onBack = onBack,
        actions = {
            S5IconButton(
                icon = Icons.AutoMirrored.Rounded.ArrowBack,
                label = "Previous page",
                enabled = navigation.canGoBack,
                onClick = { dispatch(WorkspaceWebAction.Back) },
            )
            S5IconButton(
                icon = Icons.AutoMirrored.Rounded.ArrowForward,
                label = "Next page",
                enabled = navigation.canGoForward,
                onClick = { dispatch(WorkspaceWebAction.Forward) },
            )
            S5IconButton(
                icon = Icons.Rounded.Refresh,
                label = "Reload preview",
                onClick = { dispatch(WorkspaceWebAction.Reload) },
            )
            S5IconButton(
                icon = Icons.AutoMirrored.Rounded.InsertDriveFile,
                label = "View as text",
                onClick = onOpenSource,
            )
        },
        loading = state.value is Remote.Loading || pageLoading,
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            loadError?.let { message ->
                Box(Modifier.padding(S5Theme.spacing.gutter)) {
                    S5Notice(
                        icon = Icons.Rounded.Public,
                        text = message,
                        containerColor = MaterialTheme.colorScheme.errorContainer,
                        contentColor = MaterialTheme.colorScheme.onErrorContainer,
                    )
                }
            }
            when (val url = state.value) {
                is Remote.Loading -> S5LoadingState("Preparing the preview…")
                is Remote.Failed ->
                    Box(Modifier.padding(S5Theme.spacing.gutter)) {
                        S5ErrorState(
                            title = "Couldn't open this file",
                            detail = url.message,
                            onRetry = retry,
                        )
                    }
                is Remote.Loaded -> {
                    Text(
                        text = navigation.url ?: url.value,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier =
                            Modifier.fillMaxWidth()
                                .padding(horizontal = S5Theme.spacing.gutter),
                    )
                    WorkspaceWebView(
                        url = url.value,
                        onError = { loadError = it },
                        onLoadingChanged = { pageLoading = it },
                        onNavigationChanged = { navigation = it },
                        command = command,
                        onCommandHandled = { handled ->
                            if (command?.id == handled) command = null
                        },
                        modifier = Modifier.fillMaxWidth().weight(1f),
                    )
                }
            }
        }
    }
}
