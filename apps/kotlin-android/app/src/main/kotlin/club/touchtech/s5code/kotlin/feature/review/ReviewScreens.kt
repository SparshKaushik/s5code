package club.touchtech.s5code.kotlin.feature.review

import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
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
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.AddCircleOutline
import androidx.compose.material.icons.rounded.AddPhotoAlternate
import androidx.compose.material.icons.rounded.BrokenImage
import androidx.compose.material.icons.rounded.ContentPaste
import androidx.compose.material.icons.automirrored.rounded.Comment
import androidx.compose.material.icons.rounded.Difference
import androidx.compose.material.icons.rounded.FolderOpen
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.ExpandLess
import androidx.compose.material.icons.rounded.ExpandMore
import androidx.compose.material.icons.rounded.RemoveCircleOutline
import androidx.compose.material.icons.automirrored.rounded.Send
import androidx.compose.material.icons.rounded.VisibilityOff
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.Remote
import club.touchtech.s5code.kotlin.data.acceptComposerImages
import club.touchtech.s5code.kotlin.data.rememberRetryableRemote
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5AttachmentStrip
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5ErrorState
import club.touchtech.s5code.kotlin.design.component.S5LoadingState
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5IconToggleButton
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5MenuOption
import club.touchtech.s5code.kotlin.design.component.S5OverflowMenu
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5StatusPill
import club.touchtech.s5code.kotlin.design.component.S5TextField
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.rememberHighlightedLines
import club.touchtech.s5code.kotlin.design.text.codeLanguageOfPath
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import club.touchtech.s5code.kotlin.model.DiffLineKind
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.ReviewFile
import club.touchtech.s5code.kotlin.model.ReviewFileStatus
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.platform.composerImageReceiver
import club.touchtech.s5code.kotlin.platform.rememberClipboardHasImage
import club.touchtech.s5code.kotlin.platform.rememberClipboardImageReader
import club.touchtech.s5code.kotlin.platform.rememberComposerImageIntake
import club.touchtech.s5code.kotlin.platform.rememberComposerImagePicker
import kotlinx.coroutines.launch

/**
 * Working-tree review. Files collapse independently, diffs hydrate on expand,
 * and binary/large files get an explicit notice instead of a broken diff.
 */
@Composable
fun ReviewScreen(
    store: AppStore,
    environmentId: String,
    threadId: String,
    onBack: () -> Unit,
    onComment: (ReviewCommentTarget) -> Unit,
) {
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val id = remember(threadId) { ThreadId(threadId) }
    val (state, retry) =
        rememberRetryableRemote(environmentId, threadId) { store.workspace.review(env, id) }
    var collapsed by remember(threadId) { mutableStateOf(emptySet<String>()) }
    var hideUnchanged by remember { mutableStateOf(false) }
    var selectedTarget by remember(threadId) { mutableStateOf<ReviewCommentTarget?>(null) }
    var rangeAnchor by remember(threadId) { mutableStateOf<ReviewCommentTarget?>(null) }
    var fileMenuExpanded by remember { mutableStateOf(false) }
    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val remote = state.value
    val files = remote.valueOrNull.orEmpty()

    val additions = files.sumOf { it.additions }
    val deletions = files.sumOf { it.deletions }
    val visible =
        remember(files, hideUnchanged) {
            if (hideUnchanged) files.filter { it.additions + it.deletions > 0 } else files
        }

    S5Screen(
        title = "Review",
        subtitle =
            if (remote is Remote.Loaded) "${files.size} files · +$additions −$deletions" else "",
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
        actions = {
            if (visible.size > 1) {
                S5OverflowMenu(
                    icon = Icons.Rounded.MoreVert,
                    label = "Jump to file",
                    options =
                        visible.map { file ->
                            S5MenuOption(
                                id = file.path,
                                label = file.path.substringAfterLast('/'),
                                supporting = file.path,
                                icon = Icons.Rounded.FolderOpen,
                            )
                        },
                    onSelect = { path ->
                        selectedTarget = null
                        rangeAnchor = null
                        collapsed = collapsed - path
                        scope.launch { listState.animateScrollToItem(visible.indexOfFirst { it.path == path }) }
                    },
                    expanded = fileMenuExpanded,
                    onExpandedChange = { fileMenuExpanded = it },
                )
            }
            S5IconToggleButton(
                icon = Icons.Rounded.VisibilityOff,
                label = "Hide files with no changes",
                checked = hideUnchanged,
                onCheckedChange = { hideUnchanged = it },
            )
        },
        floatingActionButton = {
            selectedTarget?.let { target ->
                S5Button(
                    text = "Comment on ${formatReviewSelectedRangeLabel(target)}",
                    onClick = { onComment(target) },
                    icon = Icons.AutoMirrored.Rounded.Comment,
                    emphasis = S5ActionEmphasis.Primary,
                    style = S5ButtonStyle.Elevated,
                )
            }
        },
    ) { padding ->
        when (remote) {
            is Remote.Loading -> S5LoadingState("Building the diff…", Modifier.padding(padding))
            is Remote.Failed ->
                Box(Modifier.padding(padding).padding(S5Theme.spacing.gutter)) {
                    S5ErrorState(title = "Couldn't build the diff", detail = remote.message, onRetry = retry)
                }
            is Remote.Loaded ->
                if (visible.isEmpty()) {
                    S5EmptyState(
                        icon = Icons.Rounded.Difference,
                        title = "No changes",
                        detail = "The working tree matches the base branch.",
                        modifier = Modifier.padding(padding),
                    )
                } else {
                    LazyColumn(
                        state = listState,
                        modifier = Modifier.fillMaxSize().padding(padding),
                        contentPadding =
                            PaddingValues(
                                start = S5Theme.spacing.gutter,
                                end = S5Theme.spacing.gutter,
                                bottom = 96.dp,
                            ),
                        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                    ) {
                        items(visible.size, key = { visible[it].path }) { index ->
                            val file = visible[index]
                            // Tracked as collapsed rather than expanded so a diff that
                            // arrives after the first frame still opens by default.
                            ReviewFileCard(
                                file = file,
                                expanded = file.path !in collapsed,
                                selectedTarget = selectedTarget?.takeIf { it.filePath == file.path },
                                rangeAnchorIndex =
                                    rangeAnchor?.takeIf { it.filePath == file.path }?.normalizedStart,
                                onLineTap = { lines, lineIndex ->
                                    val anchor = rangeAnchor
                                    if (anchor != null && anchor.filePath == file.path) {
                                        selectedTarget =
                                            buildReviewCommentTarget(
                                                sectionId = "working-tree",
                                                sectionTitle = "Working tree",
                                                filePath = file.path,
                                                lines = lines,
                                                anchorIndex = anchor.normalizedStart,
                                                lineIndex = lineIndex,
                                            )
                                        rangeAnchor = null
                                    } else {
                                        val target =
                                            buildReviewCommentTarget(
                                                sectionId = "working-tree",
                                                sectionTitle = "Working tree",
                                                filePath = file.path,
                                                lines = lines,
                                                anchorIndex = lineIndex,
                                                lineIndex = lineIndex,
                                            )
                                        selectedTarget = target
                                        onComment(target)
                                    }
                                },
                                onLineLongPress = { lines, lineIndex ->
                                    val target =
                                        buildReviewCommentTarget(
                                            sectionId = "working-tree",
                                            sectionTitle = "Working tree",
                                            filePath = file.path,
                                            lines = lines,
                                            anchorIndex = lineIndex,
                                            lineIndex = lineIndex,
                                        )
                                    rangeAnchor = target
                                    selectedTarget = target
                                },
                                onToggle = {
                                    collapsed =
                                        if (file.path in collapsed) collapsed - file.path
                                        else collapsed + file.path
                                },
                            )
                        }
                    }
                }
        }
    }
}

@Composable
private fun ReviewFileCard(
    file: ReviewFile,
    expanded: Boolean,
    selectedTarget: ReviewCommentTarget?,
    rangeAnchorIndex: Int?,
    onLineTap: (List<club.touchtech.s5code.kotlin.model.DiffLine>, Int) -> Unit,
    onLineLongPress: (List<club.touchtech.s5code.kotlin.model.DiffLine>, Int) -> Unit,
    onToggle: () -> Unit,
) {
    val language = remember(file.path) { codeLanguageOfPath(file.path) }
    S5Card(tone = S5CardTone.Standard, modifier = Modifier.fillMaxWidth()) {
        Row(
            Modifier.fillMaxWidth()
                .combinedClickable(onClick = onToggle)
                .padding(S5Theme.spacing.medium),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Icon(
                if (expanded) Icons.Rounded.ExpandLess else Icons.Rounded.ExpandMore,
                contentDescription = null,
                modifier = Modifier.size(18.dp),
            )
            Column(Modifier.weight(1f)) {
                Text(
                    file.path.substringAfterLast('/'),
                    style = MaterialTheme.typography.titleSmallEmphasized,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    file.path,
                    style = S5Theme.code.inlineTechnical,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            S5StatusPill(
                label =
                    when (file.status) {
                        ReviewFileStatus.Modified -> "M"
                        ReviewFileStatus.Added -> "A"
                        ReviewFileStatus.Deleted -> "D"
                        ReviewFileStatus.Renamed -> "R"
                        ReviewFileStatus.Binary -> "bin"
                    },
                containerColor = MaterialTheme.colorScheme.surfaceContainerHighest,
                contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text("+${file.additions}", style = S5Theme.code.inlineTechnical, color = S5Theme.status.added)
            Text("−${file.deletions}", style = S5Theme.code.inlineTechnical, color = S5Theme.status.removed)
        }

        if (expanded) {
            if (file.status == ReviewFileStatus.Binary || file.hunks.isEmpty()) {
                Box(Modifier.padding(S5Theme.spacing.medium)) {
                    S5Notice(
                        icon = Icons.Rounded.Difference,
                        text =
                            if (file.status == ReviewFileStatus.Binary) {
                                "Binary file — no textual diff available."
                            } else {
                                "Diff not loaded. Reopen to hydrate the full patch."
                            },
                    )
                }
            } else {
                val lines = remember(file.hunks) { file.hunks.flatMap { it.lines } }
                val highlightedLines = rememberHighlightedLines(lines.map { it.text }, language)
                val wordRanges = remember(lines) { reviewWordDiffRanges(lines) }
                Column(Modifier.padding(bottom = S5Theme.spacing.small)) {
                    var lineIndex = 0
                    file.hunks.forEach { hunk ->
                            Text(
                                hunk.header,
                                style = S5Theme.code.codeSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier =
                                    Modifier.fillMaxWidth()
                                        .background(MaterialTheme.colorScheme.surfaceContainerHigh)
                                        .padding(
                                            horizontal = S5Theme.spacing.medium,
                                            vertical = S5Theme.spacing.tiny,
                                        ),
                            )
                            val scroll = rememberScrollState()
                            hunk.lines.forEach { line ->
                                val currentLineIndex = lineIndex++
                                val selected =
                                    selectedTarget != null &&
                                        currentLineIndex in selectedTarget.normalizedStart..selectedTarget.normalizedEnd
                                val anchor = rangeAnchorIndex == currentLineIndex
                                val annotated = highlightedLines[currentLineIndex]
                                val wordHighlightColor =
                                    MaterialTheme.colorScheme.onSurface.copy(alpha = 0.16f)
                                val rendered =
                                    remember(
                                        annotated,
                                        wordRanges[currentLineIndex],
                                        wordHighlightColor,
                                    ) {
                                        annotated.withWordDiff(
                                            ranges = wordRanges[currentLineIndex].orEmpty(),
                                            background = wordHighlightColor,
                                        )
                                    }
                                Row(
                                    Modifier.fillMaxWidth()
                                        .background(
                                            when {
                                                selected -> MaterialTheme.colorScheme.secondaryContainer
                                                line.kind == DiffLineKind.Added ->
                                                    S5Theme.status.added.copy(alpha = 0.12f)
                                                line.kind == DiffLineKind.Removed ->
                                                    S5Theme.status.removed.copy(alpha = 0.12f)
                                                else -> MaterialTheme.colorScheme.surfaceContainerLow
                                            }
                                        )
                                        .clip(MaterialTheme.shapes.extraSmall)
                                        .combinedClickable(
                                            onClick = { onLineTap(lines, currentLineIndex) },
                                            onLongClick = { onLineLongPress(lines, currentLineIndex) },
                                        )
                                        .horizontalScroll(scroll)
                                        .padding(horizontal = S5Theme.spacing.small),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Text(
                                        (line.oldNo?.toString() ?: "").padStart(4),
                                        style = S5Theme.code.codeSmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                    Text(
                                        (line.newNo?.toString() ?: "").padStart(4),
                                        style = S5Theme.code.codeSmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        modifier = Modifier.padding(start = S5Theme.spacing.tiny),
                                    )
                                    Icon(
                                        when (line.kind) {
                                            DiffLineKind.Added -> Icons.Rounded.AddCircleOutline
                                            DiffLineKind.Removed -> Icons.Rounded.RemoveCircleOutline
                                            DiffLineKind.Context -> Icons.Rounded.Difference
                                        },
                                        contentDescription =
                                            when (line.kind) {
                                                DiffLineKind.Added -> "Added line"
                                                DiffLineKind.Removed -> "Removed line"
                                                DiffLineKind.Context -> null
                                            },
                                        modifier =
                                            Modifier.padding(horizontal = S5Theme.spacing.tiny).size(12.dp),
                                        tint =
                                            when (line.kind) {
                                                DiffLineKind.Added -> S5Theme.status.added
                                                DiffLineKind.Removed -> S5Theme.status.removed
                                                DiffLineKind.Context ->
                                                    MaterialTheme.colorScheme.surfaceContainerLow
                                            },
                                    )
                                    if (anchor) {
                                        Text(
                                            "range start",
                                            style = MaterialTheme.typography.labelSmall,
                                            color = MaterialTheme.colorScheme.primary,
                                            fontWeight = FontWeight.Bold,
                                            modifier = Modifier.padding(end = S5Theme.spacing.tiny),
                                        )
                                    }
                                    Text(rendered, style = S5Theme.code.code, softWrap = false)
                                }
                            }
                        }
                    }
                }
            }
        }
    }

@Composable
fun ReviewCommentInlineCard(comment: ReviewInlineComment) {
    S5Card(tone = S5CardTone.Receded, modifier = Modifier.fillMaxWidth()) {
        Column {
            Column(
                Modifier.fillMaxWidth()
                    .background(MaterialTheme.colorScheme.surfaceContainerHigh)
                    .padding(S5Theme.spacing.medium),
            ) {
                Text(
                    comment.filePath.substringAfterLast('/').substringAfterLast('\\'),
                    style = S5Theme.code.inlineTechnical,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    "${comment.sectionTitle} · ${comment.rangeLabel}",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (comment.diff.isNotBlank()) {
                val scroll = rememberScrollState()
                Column(Modifier.horizontalScroll(scroll).padding(S5Theme.spacing.medium)) {
                    comment.diff.trim().lineSequence().take(12).forEach { line ->
                        Text(
                            line,
                            style = S5Theme.code.codeSmall,
                            color = MaterialTheme.colorScheme.onSurface,
                            softWrap = false,
                        )
                    }
                }
            }
            if (comment.text.isNotBlank()) {
                Text(
                    comment.text,
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.padding(S5Theme.spacing.medium),
                )
            }
        }
    }
}

private fun AnnotatedString.withWordDiff(
    ranges: List<IntRange>,
    background: Color,
): AnnotatedString {
    if (ranges.isEmpty()) return this
    return buildAnnotatedString {
        append(this@withWordDiff)
        ranges.forEach { range ->
            if (range.first < length) {
                addStyle(
                    SpanStyle(
                        background = background,
                        fontWeight = FontWeight.Bold,
                    ),
                    start = range.first,
                    end = (range.last + 1).coerceAtMost(length),
                )
            }
        }
    }
}

/** Review comment composer for a selected line range. */
@Composable
fun ReviewCommentScreen(
    store: AppStore,
    environmentId: String,
    threadId: String,
    filePath: String,
    startIndex: Int,
    endIndex: Int,
    onBack: () -> Unit,
) {
    // The field owns its text so keyboard content commits reach it. Only the
    // blank/non-blank flip is read here, so typing does not recompose the screen.
    val commentState = rememberTextFieldState()
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val id = remember(threadId) { ThreadId(threadId) }
    val (reviewState, retryReview) =
        rememberRetryableRemote(environmentId, threadId, filePath) { store.workspace.review(env, id) }
    val target =
        remember(reviewState.value, filePath, startIndex, endIndex) {
            val file = reviewState.value.valueOrNull?.firstOrNull { it.path == filePath }
            val lines = file?.hunks?.flatMap { it.lines }.orEmpty()
            if (file == null || lines.isEmpty()) null
            else buildReviewCommentTarget(
                sectionId = "working-tree",
                sectionTitle = "Working tree",
                filePath = filePath,
                lines = lines,
                anchorIndex = startIndex,
                lineIndex = endIndex,
            )
        }
    val hasComment by remember(commentState, target) {
        derivedStateOf { commentState.text.isNotBlank() && target != null }
    }
    // The comment sheet is not backed by a durable draft, so its attachments
    // live with the screen. Validation is the same as the thread composer's.
    var attachments by remember { mutableStateOf(emptyList<ComposerAttachment>()) }
    var attachmentError by remember { mutableStateOf<String?>(null) }
    val addImages =
        rememberComposerImageIntake { candidates ->
            val result = acceptComposerImages(attachments, candidates)
            attachments = attachments + result.attachments
            attachmentError = result.error
        }
    val readClipboardImages = rememberClipboardImageReader()
    val clipboardHasImage = rememberClipboardHasImage()
    val pickImages =
        rememberComposerImagePicker(
            remaining = ComposerAttachmentLimits.MAX_ATTACHMENTS - attachments.size,
            onImages = addImages,
        )
    S5Screen(
        title = "Comment",
        subtitle =
            target?.let { "${it.filePath.substringAfterLast('/')} · ${formatReviewSelectedRangeLabel(it)}" }
                ?: "Selected diff lines",
        prominence = S5TopBarProminence.Centered,
        onBack = onBack,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                when (val review = reviewState.value) {
                    is Remote.Loading -> S5LoadingState("Loading selected lines…")
                    is Remote.Failed ->
                        S5ErrorState(
                            title = "Couldn't load selected lines",
                            detail = review.message,
                            onRetry = retryReview,
                        )
                    is Remote.Loaded ->
                        if (target == null) {
                            S5EmptyState(
                                icon = Icons.AutoMirrored.Rounded.Comment,
                                title = "Selection unavailable",
                                detail = "Return to Review and select a diff line or range.",
                            )
                        } else {
                            S5Card(tone = S5CardTone.Receded, modifier = Modifier.fillMaxWidth()) {
                                Column(Modifier.padding(S5Theme.spacing.medium)) {
                                    target.selectedLines.take(5).forEach { line ->
                                        Text(
                                            text =
                                                when (line.kind) {
                                                    DiffLineKind.Added -> "+${line.text}"
                                                    DiffLineKind.Removed -> "-${line.text}"
                                                    DiffLineKind.Context -> " ${line.text}"
                                                },
                                            style = S5Theme.code.code,
                                            softWrap = false,
                                        )
                                    }
                                    if (target.selectedLines.size > 5) {
                                        Text(
                                            "… ${target.selectedLines.size - 5} more lines",
                                            style = MaterialTheme.typography.labelSmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                    }
                                }
                            }
                        }
                }
            }
            S5TextField(
                state = commentState,
                label = "Comment",
                placeholder = "Ask for a change on these lines…",
                minHeight = 88.dp,
                modifier = Modifier.padding(horizontal = S5Theme.spacing.gutter),
                fieldModifier = Modifier.composerImageReceiver(addImages),
            )
            attachmentError?.let { error ->
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5Notice(
                        icon = Icons.Rounded.BrokenImage,
                        text = error,
                        containerColor = MaterialTheme.colorScheme.errorContainer,
                        contentColor = MaterialTheme.colorScheme.onErrorContainer,
                        onDismiss = { attachmentError = null },
                    )
                }
            }
            S5AttachmentStrip(
                attachments = attachments,
                onRemove = { attachment -> attachments = attachments - attachment },
                modifier = Modifier.padding(horizontal = S5Theme.spacing.gutter),
            )
            Row(
                Modifier.fillMaxWidth().padding(horizontal = S5Theme.spacing.gutter),
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                S5IconButton(
                    icon = Icons.Rounded.AddPhotoAlternate,
                    label = "Attach image",
                    onClick = pickImages,
                )
                S5IconButton(
                    icon = Icons.Rounded.ContentPaste,
                    label = "Paste image from clipboard",
                    onClick = { addImages(readClipboardImages()) },
                    enabled = clipboardHasImage,
                )
            }
            Row(
                Modifier.fillMaxWidth().padding(horizontal = S5Theme.spacing.gutter),
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                S5Button(
                    text = "Add to thread",
                    onClick = {
                        val selected = target ?: return@S5Button
                        store.appendThreadDraft(
                            environmentId = environmentId,
                            threadId = threadId,
                            text = formatReviewCommentContext(selected, commentState.text.toString()),
                            attachments = attachments,
                        )
                        onBack()
                    },
                    emphasis = S5ActionEmphasis.Primary,
                    icon = Icons.AutoMirrored.Rounded.Send,
                    enabled = hasComment,
                )
                S5Button(
                    text = "Cancel",
                    onClick = onBack,
                    emphasis = S5ActionEmphasis.Primary,
                    style = S5ButtonStyle.Text,
                )
            }
        }
    }
}
