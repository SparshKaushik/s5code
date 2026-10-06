package club.touchtech.s5code.kotlin.feature.thread

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Chat
import androidx.compose.material.icons.automirrored.rounded.InsertDriveFile
import androidx.compose.material.icons.rounded.AutoAwesome
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.ExpandLess
import androidx.compose.material.icons.rounded.ExpandMore
import androidx.compose.material.icons.rounded.Image
import androidx.compose.material.icons.rounded.Psychology
import androidx.compose.material.icons.rounded.RadioButtonUnchecked
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.WarningAmber
import androidx.compose.material.icons.rounded.Build
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5CodeBlock
import club.touchtech.s5code.kotlin.design.component.S5ImageLightbox
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.component.S5Markdown
import coil3.compose.AsyncImage
import club.touchtech.s5code.kotlin.data.SecretRequestAnswer
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.feature.review.ReviewCommentInlineCard
import club.touchtech.s5code.kotlin.feature.review.ReviewCommentMessageSegment
import club.touchtech.s5code.kotlin.feature.review.parseReviewCommentMessageSegments
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.FeedEntry
import club.touchtech.s5code.kotlin.model.PlanStepState
import club.touchtech.s5code.kotlin.model.SentAttachment
import club.touchtech.s5code.kotlin.model.ToolState
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonObject

/**
 * One transcript entry. Every branch is a separate composable so a streaming
 * agent message never invalidates the tool rows above it.
 */
@Composable
fun FeedEntryRow(
    entry: FeedEntry,
    onCopy: (String) -> Unit,
    modifier: Modifier = Modifier,
    workspaceRoot: String? = null,
    onOpenFile: (String) -> Unit = {},
    resolveTranscriptAttachment: suspend (String) -> String? = { null },
    /** Opens a sent attachment in the viewer route — file chips only; images keep the lightbox. */
    onOpenAttachment: (SentAttachment) -> Unit = {},
    /**
     * Hoisted disclosure state for the collapsible rows (tool calls, thoughts,
     * subagents, answered prompts). Null means the row owns its own state; the
     * thread feed hoists so a collapse can re-anchor the viewport — a row that
     * shrinks entirely above it shifts what the reader is looking at otherwise.
     */
    expandedIds: Set<String>? = null,
    onToggleExpand: (FeedEntry) -> Unit = {},
    onOpenThread: (String) -> Unit = {},
    /**
     * Sends `secrets.answerRequest` for a pending [FeedEntry.SecretRequest];
     * throws on failure so the card can map it with `secretRequestFailureMessage`.
     */
    onAnswerSecretRequest: suspend (FeedEntry.SecretRequest, SecretRequestAnswer) -> Unit = { _, _ -> },
    /**
     * `orchestration.getTurnItem` for a row whose wire payload withheld its
     * output ([FeedEntry.ToolCall.fetchesDetail]); runs only while expanded.
     */
    fetchToolDetail: suspend (FeedEntry.ToolCall) -> JsonObject? = { null },
    /**
     * Mints the signed `tool-output-image` URL for one index of a tool call's
     * output ([FeedEntry.ToolCall.outputImageCount]); null when unsupported.
     */
    resolveToolOutputImage: suspend (FeedEntry.ToolCall, Int) -> String? = { _, _ -> null },
    /**
     * `prepared-run.retry` for an error row carrying
     * [FeedEntry.ErrorEntry.retryablePreparationRunId]; the caller surfaces
     * failures, so implementations catch their own.
     */
    onRetryPreparation: suspend (runId: String) -> Unit = {},
) {
    when (entry) {
        is FeedEntry.TurnDivider ->
            Row(
                modifier.fillMaxWidth().padding(vertical = S5Theme.spacing.medium),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                HorizontalDivider(Modifier.weight(1f))
                Text(
                    entry.label,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                HorizontalDivider(Modifier.weight(1f))
            }

        is FeedEntry.UserMessage ->
            UserBubble(entry, modifier, resolveTranscriptAttachment, onOpenAttachment)

        is FeedEntry.AgentMessage ->
            AgentMessage(
                entry,
                onCopy,
                modifier,
                workspaceRoot,
                onOpenFile,
                resolveTranscriptAttachment,
                onOpenAttachment,
            )

        is FeedEntry.Reasoning -> ReasoningRow(entry, expandedIds, onToggleExpand, modifier)

        is FeedEntry.ToolCall ->
            ToolRow(
                entry,
                onCopy,
                expandedIds,
                onToggleExpand,
                modifier,
                fetchToolDetail,
                resolveToolOutputImage,
            )

        is FeedEntry.PlanUpdate -> PlanCard(entry, modifier)

        is FeedEntry.Subagent -> SubagentRow(entry, expandedIds, onToggleExpand, modifier, onOpenThread)

        is FeedEntry.QuestionAnswer -> QuestionAnswerRow(entry, onOpenAttachment, expandedIds, onToggleExpand, modifier)

        is FeedEntry.Warning -> WarningRow(entry, modifier)

        is FeedEntry.SecretRequest ->
            SecretRequestRow(entry, modifier, onAnswer = { onAnswerSecretRequest(entry, it) })

        is FeedEntry.Note -> NoteRow(entry, modifier)

        is FeedEntry.ErrorEntry -> ErrorRow(entry, modifier, onRetryPreparation)
    }
}

/**
 * "Working for 12s" at the live edge of the transcript, for as long as a turn is in
 * flight. Ported from `WorkingTimelineRow` in `apps/mobile/src/features/threads/ThreadFeed.tsx`.
 *
 * The row exists for the case where the agent has produced nothing at all: a provider
 * thinking for ninety seconds looks exactly like a dead thread otherwise. So it is
 * shown whenever work is running, not only when there is nothing else to show.
 *
 * Three static dots rather than a `LoadingIndicator`: a turn can run for ten minutes,
 * and a morphing indicator that long is the perpetual repaint the house rules ban.
 * The ticking label is the liveness signal, and it costs one recomposition a second.
 */
@Composable
fun WorkingRow(row: FeedRow.Working, modifier: Modifier = Modifier) {
    var nowMillis by remember(row.startedAtMillis) { mutableStateOf(System.currentTimeMillis()) }
    LaunchedEffect(row.startedAtMillis) {
        while (true) {
            // Align to the second boundary so the number changes when the user's own
            // clock says it should, not 400ms after.
            val elapsed = System.currentTimeMillis() - row.startedAtMillis
            delay((1_000 - elapsed.mod(1_000L)).coerceAtLeast(1L))
            nowMillis = System.currentTimeMillis()
        }
    }

    Row(
        modifier.fillMaxWidth().padding(vertical = S5Theme.spacing.tiny, horizontal = S5Theme.spacing.tiny),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(2.dp), verticalAlignment = Alignment.CenterVertically) {
            listOf(1f, 0.8f, 0.6f).forEach { alpha ->
                Box(
                    Modifier.size(4.dp)
                        .clip(CircleShape)
                        .background(MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = alpha))
                )
            }
        }
        Text(
            workingLabel(row.startedAtMillis, nowMillis),
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/**
 * Discloses a whole finished turn, whose work is folded down to its answer.
 *
 * A hairline above the label rather than a chip: this is a section boundary between
 * turns, and every turn in a long transcript gets one, so it has to be quiet enough
 * to scroll past. Same treatment as the RN row's bottom border.
 *
 * An expanded turn renders this twice — once above its work and once below it — so a
 * turn that fills several screens can be closed from wherever the reader ended up.
 * The footer names the action instead of repeating the duration.
 */
@Composable
fun TurnFoldRow(row: FeedRow.TurnFold, onToggle: () -> Unit, modifier: Modifier = Modifier) {
    val footer = row.placement == FeedRow.TurnFold.Placement.Footer
    Column(modifier.fillMaxWidth()) {
        // The divider sits between turns. On the footer it would draw a second line
        // immediately above the next turn's own, so it is the header's alone.
        if (!footer) {
            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f))
        }
        Row(
            Modifier.fillMaxWidth()
                .clip(MaterialTheme.shapes.small)
                .clickable(onClick = onToggle)
                .semantics { stateDescription = if (row.expanded) "Expanded" else "Collapsed" }
                .padding(vertical = S5Theme.spacing.small, horizontal = S5Theme.spacing.tiny),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Text(
                row.text,
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Icon(
                if (row.expanded) Icons.Rounded.ExpandLess else Icons.Rounded.ExpandMore,
                contentDescription = if (row.expanded) "Hide this turn's work" else "Show this turn's work",
                modifier = Modifier.size(16.dp),
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/**
 * Discloses the work rows folded above it.
 *
 * Deliberately a plain row rather than a card: it is a control over the rows around
 * it, not another entry in the transcript, and a card here reads as a third kind
 * of tool row.
 */
@Composable
fun WorkGroupToggleRow(row: FeedRow.WorkToggle, onToggle: () -> Unit, modifier: Modifier = Modifier) {
    Row(
        modifier
            .fillMaxWidth()
            .clip(MaterialTheme.shapes.small)
            .clickable(onClick = onToggle)
            .semantics { this.stateDescription = if (row.expanded) "Expanded" else "Collapsed" }
            .padding(vertical = S5Theme.spacing.tiny, horizontal = S5Theme.spacing.tiny),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        Icon(
            if (row.expanded) Icons.Rounded.ExpandLess else Icons.Rounded.ExpandMore,
            contentDescription = null,
            modifier = Modifier.size(16.dp),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            row.summary,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun UserBubble(
    entry: FeedEntry.UserMessage,
    modifier: Modifier,
    resolveTranscriptAttachment: suspend (String) -> String?,
    onOpenAttachment: (SentAttachment) -> Unit,
) {
    var previewAttachment by remember(entry.id) { mutableStateOf<ComposerAttachment?>(null) }
    Row(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
        Surface(
            shape = MaterialTheme.shapes.largeIncreased,
            color = MaterialTheme.colorScheme.primaryContainer,
            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
            modifier = Modifier.widthIn(max = 520.dp),
        ) {
            Column(Modifier.padding(S5Theme.spacing.large)) {
                // `t3-context://` links resolve to their record labels (or an
                // "(unavailable)" marker) before review-comment segmentation:
                // a label can never look like a <review-comment> block.
                val contextResolvedText =
                    remember(entry.text, entry.contextRecords) {
                        replaceComposerContextReferences(entry.text, entry.contextRecords)
                    }
                val segments =
                    remember(contextResolvedText) {
                        parseReviewCommentMessageSegments(contextResolvedText)
                    }
                val hasReviewComments = segments.any { it is ReviewCommentMessageSegment.Comment }
                if (hasReviewComments) {
                    Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                        segments.forEach { segment ->
                            when (segment) {
                                is ReviewCommentMessageSegment.Comment ->
                                    ReviewCommentInlineCard(segment.comment)
                                is ReviewCommentMessageSegment.Text ->
                                    segment.text.trim().takeIf(String::isNotEmpty)?.let { text ->
                                        Text(text, style = MaterialTheme.typography.bodyMedium)
                                    }
                            }
                        }
                    }
                } else {
                    Text(contextResolvedText, style = MaterialTheme.typography.bodyMedium)
                }
                if (entry.attachments.isNotEmpty()) {
                    Row(
                        Modifier.padding(top = S5Theme.spacing.small),
                        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                    ) {
                        entry.attachments.forEach { attachment ->
                            SentAttachmentThumbnail(
                                attachment = attachment,
                                resolveUrl = { resolveTranscriptAttachment(attachment.id) },
                                onPreview = { url ->
                                    previewAttachment = attachment.copy(uri = url)
                                },
                                onOpen = { onOpenAttachment(attachment.toSentAttachment()) },
                            )
                        }
                    }
                }
                Text(
                    entry.timeLabel,
                    style = MaterialTheme.typography.labelSmall,
                    modifier = Modifier.padding(top = S5Theme.spacing.tiny),
                )
            }
        }
    }
    S5ImageLightbox(
        model = previewAttachment?.uri?.takeIf(String::isNotBlank),
        contentDescription = previewAttachment?.name,
        imageKey = previewAttachment?.id,
        onDismiss = { previewAttachment = null },
    )
}

@Composable
private fun AgentMessage(
    entry: FeedEntry.AgentMessage,
    onCopy: (String) -> Unit,
    modifier: Modifier,
    workspaceRoot: String?,
    onOpenFile: (String) -> Unit,
    resolveTranscriptAttachment: suspend (String) -> String?,
    onOpenAttachment: (SentAttachment) -> Unit,
) {
    var previewAttachment by remember(entry.id) { mutableStateOf<ComposerAttachment?>(null) }
    // No avatar and no provider label, matching the RN feed: the agent's identity
    // belongs to the thread, not to every paragraph it writes. Assistant text runs
    // full-width, and the meta row sits underneath a settled message only —
    // a copy button on text still arriving copies half an answer.
    Column(modifier.fillMaxWidth()) {
        S5Markdown(
            source = entry.markdown,
            onCopyCode = onCopy,
            workspaceRoot = workspaceRoot,
            onOpenFile = onOpenFile,
        )
        if (entry.attachments.isNotEmpty()) {
            Row(
                Modifier.padding(top = S5Theme.spacing.small),
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                entry.attachments.forEach { attachment ->
                    SentAttachmentThumbnail(
                        attachment = attachment,
                        resolveUrl = { resolveTranscriptAttachment(attachment.id) },
                        onPreview = { url ->
                            previewAttachment = attachment.copy(uri = url)
                        },
                        onOpen = { onOpenAttachment(attachment.toSentAttachment()) },
                    )
                }
            }
        }
        if (entry.streaming) {
            S5InlineLoading(Modifier.padding(top = S5Theme.spacing.tiny).size(18.dp))
        } else {
            Row(
                Modifier.padding(top = S5Theme.spacing.tiny),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
            ) {
                S5IconButton(
                    icon = Icons.Rounded.ContentCopy,
                    label = "Copy message",
                    onClick = { onCopy(entry.markdown) },
                )
                Text(
                    entry.timeLabel,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
    S5ImageLightbox(
        model = previewAttachment?.uri,
        contentDescription = previewAttachment?.name,
        imageKey = previewAttachment?.id,
        onDismiss = { previewAttachment = null },
    )
}

/**
 * A sent attachment's chip. Images keep the in-place thumbnail + lightbox;
 * every other `type` renders as a file chip that opens the attachment viewer —
 * the RN feed's split between a picture you glance at and a file you open.
 */
@Composable
private fun SentAttachmentThumbnail(
    attachment: ComposerAttachment,
    resolveUrl: suspend () -> String?,
    onPreview: (String) -> Unit,
    onOpen: () -> Unit,
) {
    if (attachment.type != "image") {
        Surface(
            onClick = onOpen,
            shape = MaterialTheme.shapes.small,
            color = MaterialTheme.colorScheme.surfaceContainerHighest,
        ) {
            Row(
                Modifier.padding(horizontal = S5Theme.spacing.small, vertical = S5Theme.spacing.tiny),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
            ) {
                Icon(
                    Icons.AutoMirrored.Rounded.InsertDriveFile,
                    contentDescription = null,
                    modifier = Modifier.size(14.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    attachment.name,
                    style = MaterialTheme.typography.labelSmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.widthIn(max = 160.dp),
                )
            }
        }
        return
    }
    var resolvedUrl by remember(attachment.id, attachment.uri) {
        mutableStateOf(attachment.uri.takeIf(String::isNotBlank))
    }
    LaunchedEffect(attachment.id, attachment.uri) {
        if (resolvedUrl == null) resolvedUrl = resolveUrl()
    }
    if (resolvedUrl != null) {
        Surface(
            onClick = { onPreview(checkNotNull(resolvedUrl)) },
            shape = MaterialTheme.shapes.small,
        ) {
            AsyncImage(
                model = resolvedUrl,
                contentDescription = attachment.name,
                contentScale = ContentScale.Crop,
                modifier = Modifier.size(44.dp),
            )
        }
    } else {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
        ) {
            Icon(Icons.Rounded.Image, contentDescription = null, modifier = Modifier.size(14.dp))
            Text(
                attachment.name,
                style = MaterialTheme.typography.labelSmall,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/** The viewer route works off the `ChatAttachment` record; the composer type carries it. */
private fun ComposerAttachment.toSentAttachment(): SentAttachment =
    SentAttachment(
        id = id,
        name = name,
        mimeType = mimeType,
        sizeBytes = sizeBytes,
        type = type,
    )

/**
 * Reasoning is collapsed by default: it is context, not the answer. A provider
 * thought trace reads "Thought (×N)" — `reasoningLabel`, matching
 * `ThreadReasoningRow` — while a `task.progress` tick stays "Thinking".
 */
@Composable
private fun ReasoningRow(
    entry: FeedEntry.Reasoning,
    expandedIds: Set<String>?,
    onToggleExpand: (FeedEntry) -> Unit,
    modifier: Modifier,
) {
    // Keyed on the entry: a reused row must not inherit the previous entry's
    // disclosure state when the list recycles it.
    var localExpanded by remember(entry.id) { mutableStateOf(false) }
    val expanded = if (expandedIds != null) entry.id in expandedIds else localExpanded
    val toggle: () -> Unit =
        if (expandedIds != null) {
            { onToggleExpand(entry) }
        } else {
            { localExpanded = !localExpanded }
        }
    S5Card(tone = S5CardTone.Receded, onClick = toggle, modifier = modifier.fillMaxWidth()) {
        Column(Modifier.padding(S5Theme.spacing.medium)) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                Icon(
                    Icons.Rounded.Psychology,
                    contentDescription = null,
                    modifier = Modifier.size(16.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    reasoningLabel(entry),
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Box(Modifier.weight(1f))
                Icon(
                    if (expanded) Icons.Rounded.ExpandLess else Icons.Rounded.ExpandMore,
                    contentDescription = null,
                    modifier = Modifier.size(18.dp),
                )
            }
            if (!expanded) {
                Text(
                    entry.text,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            AnimatedVisibility(
                visible = expanded,
                enter = fadeIn() + expandVertically(),
                exit = fadeOut() + shrinkVertically(),
            ) {
                // Merged traces render each message, as RN maps
                // `reasoningMessages` into the card.
                Column(
                    Modifier.padding(top = S5Theme.spacing.tiny),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    listOf(entry.text).plus(entry.extraParts).forEach { part ->
                        S5Markdown(
                            source = part,
                            textStyle = MaterialTheme.typography.bodySmall,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun ToolRow(
    entry: FeedEntry.ToolCall,
    onCopy: (String) -> Unit,
    expandedIds: Set<String>?,
    onToggleExpand: (FeedEntry) -> Unit,
    modifier: Modifier,
    fetchToolDetail: suspend (FeedEntry.ToolCall) -> JsonObject?,
    resolveToolOutputImage: suspend (FeedEntry.ToolCall, Int) -> String?,
) {
    // A chevron on a row with nothing behind it is a promise the row cannot keep.
    val canExpand = toolCallCanExpand(entry)
    var localExpanded by remember(entry.id) { mutableStateOf(false) }
    val expanded = if (expandedIds != null) entry.id in expandedIds else localExpanded
    val tint =
        when (entry.state) {
            ToolState.Running -> S5Theme.status.working
            ToolState.Succeeded -> S5Theme.status.settled
            ToolState.Failed -> S5Theme.status.failed
        }
    // A withheld payload arrives on expand (`useTurnItemDetail`): the row's
    // `detailRevision` keys the read, so a still-running item refetches only
    // when the row reopens rather than on every stream tick. Null means the
    // item was unchanged for that revision or gone entirely.
    var fetched by remember(entry.id, entry.detailRevision) { mutableStateOf(false) }
    var fetchedItem by remember(entry.id, entry.detailRevision) { mutableStateOf<JsonObject?>(null) }
    var detailError by remember(entry.id, entry.detailRevision) { mutableStateOf<String?>(null) }
    LaunchedEffect(expanded, entry.id, entry.detailRevision) {
        if (!expanded || !entry.fetchesDetail || fetched || detailError != null) return@LaunchedEffect
        try {
            fetchedItem = fetchToolDetail(entry)
            fetched = true
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (error: Exception) {
            detailError = error.message ?: "unknown error"
        }
    }
    var previewImage by remember(entry.id) { mutableStateOf<String?>(null) }
    S5Card(
        tone = S5CardTone.Standard,
        onClick =
            if (canExpand) {
                if (expandedIds != null) {
                    { onToggleExpand(entry) }
                } else {
                    { localExpanded = !localExpanded }
                }
            } else {
                null
            },
        modifier = modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(S5Theme.spacing.medium)) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                if (entry.state == ToolState.Running) {
                    S5InlineLoading(Modifier.size(16.dp))
                } else {
                    Icon(
                        if (entry.state == ToolState.Failed) Icons.Rounded.ErrorOutline
                        else Icons.Rounded.Build,
                        contentDescription = null,
                        modifier = Modifier.size(16.dp),
                        tint = tint,
                    )
                }
                // One line, same text RN's collapsed work row shows: the T3 tool's
                // friendly name, the command, the detail, or the file list.
                Text(
                    toolCallRowLabel(entry, expanded),
                    style = S5Theme.code.codeEmphasized,
                    maxLines = if (expanded) Int.MAX_VALUE else 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                Icon(
                    if (expanded) Icons.Rounded.ExpandLess else Icons.Rounded.ExpandMore,
                    contentDescription = if (expanded) "Collapse detail" else "Expand detail",
                    modifier = Modifier.size(18.dp),
                    tint =
                        if (canExpand) MaterialTheme.colorScheme.onSurfaceVariant
                        else Color.Transparent,
                )
            }
            if (expanded) {
                val fetchedText = fetchedItem?.let(::fetchedTurnItemText)
                val body =
                    remember(entry.id, entry, fetchedText) { toolCallExpandedBody(entry, fetchedText) }
                if (body != null) {
                    Box(Modifier.padding(top = S5Theme.spacing.small)) {
                        S5CodeBlock(
                            lines = body.lines(),
                            onCopy = { onCopy(body) },
                        )
                    }
                }
                // The output the timeline withheld: a fetched block under the
                // call, and the fetch's own status while it is in flight or
                // failed — RN's "Loading output…" / error line.
                if (entry.fetchesDetail && (detailError != null || !fetched || fetchedItem == null)) {
                    Row(
                        Modifier.padding(top = S5Theme.spacing.small),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                    ) {
                        if (detailError == null && !fetched) {
                            S5InlineLoading(Modifier.size(16.dp))
                        }
                        Text(
                            when {
                                detailError != null -> "Couldn't load output: $detailError"
                                fetched -> "Output is no longer available."
                                else -> "Loading output…"
                            },
                            style = MaterialTheme.typography.bodySmall,
                            color =
                                if (detailError != null) MaterialTheme.colorScheme.error
                                else MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                // Inline images never ride the timeline; each is a signed
                // `tool-output-image` asset by index, rendered below the text.
                val imageCount =
                    fetchedItem?.let(::turnItemDetailImageCount) ?: entry.outputImageCount
                if (imageCount > 0) {
                    Row(
                        Modifier.padding(top = S5Theme.spacing.small),
                        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                    ) {
                        for (index in 0 until imageCount) {
                            ToolOutputImageThumbnail(
                                index = index,
                                resolveUrl = { resolveToolOutputImage(entry, index) },
                                onPreview = { previewImage = it },
                            )
                        }
                    }
                }
            }
        }
    }
    S5ImageLightbox(
        model = previewImage,
        contentDescription = "Tool output image",
        imageKey = previewImage,
        onDismiss = { previewImage = null },
    )
}

/**
 * One inline image a tool returned, rendered at the same 44dp as a sent
 * attachment thumbnail. Signing happens once per index; a failed sign keeps the
 * quiet placeholder rather than a broken image.
 */
@Composable
private fun ToolOutputImageThumbnail(
    index: Int,
    resolveUrl: suspend () -> String?,
    onPreview: (String) -> Unit,
) {
    val url by
        produceState<String?>(initialValue = null, index) {
            value =
                try {
                    resolveUrl()
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (_: Exception) {
                    null
                }
        }
    if (url != null) {
        Surface(
            onClick = { url?.let(onPreview) },
            shape = MaterialTheme.shapes.small,
        ) {
            AsyncImage(
                model = url,
                contentDescription = "Tool output image",
                contentScale = ContentScale.Crop,
                modifier = Modifier.size(44.dp),
            )
        }
    } else {
        Surface(
            shape = MaterialTheme.shapes.small,
            color = MaterialTheme.colorScheme.surfaceContainerHighest,
        ) {
            Box(Modifier.size(44.dp), contentAlignment = Alignment.Center) {
                Icon(
                    Icons.Rounded.Image,
                    contentDescription = null,
                    modifier = Modifier.size(16.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun PlanCard(entry: FeedEntry.PlanUpdate, modifier: Modifier) {
    val done = entry.steps.count { it.state == PlanStepState.Done }
    S5Card(tone = S5CardTone.Standard, modifier = modifier.fillMaxWidth()) {
        Column(
            Modifier.padding(S5Theme.spacing.large),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Text(
                "Plan · $done of ${entry.steps.size}",
                style = MaterialTheme.typography.labelLargeEmphasized,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            entry.steps.forEach { step ->
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    when (step.state) {
                        PlanStepState.Done ->
                            Icon(
                                Icons.Rounded.CheckCircle,
                                contentDescription = "Done",
                                modifier = Modifier.size(16.dp),
                                tint = S5Theme.status.settled,
                            )
                        PlanStepState.Active ->
                            Icon(
                                Icons.Rounded.AutoAwesome,
                                contentDescription = "In progress",
                                modifier = Modifier.size(16.dp),
                                tint = S5Theme.status.working,
                            )
                        PlanStepState.Pending ->
                            Icon(
                                Icons.Rounded.RadioButtonUnchecked,
                                contentDescription = "Pending",
                                modifier = Modifier.size(16.dp),
                                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                    }
                    Text(
                        step.text,
                        style =
                            if (step.state == PlanStepState.Active) {
                                MaterialTheme.typography.bodyMediumEmphasized
                            } else {
                                MaterialTheme.typography.bodyMedium
                            },
                        color =
                            if (step.state == PlanStepState.Pending) {
                                MaterialTheme.colorScheme.onSurfaceVariant
                            } else {
                                MaterialTheme.colorScheme.onSurface
                            },
                    )
                }
            }
        }
    }
}

/**
 * A subagent's row. Collapsible for the same reason a tool row is: the task line is
 * often a paragraph, and a transcript of full paragraphs is unreadable, while a
 * truncated one with no way to see the rest is useless.
 */
@Composable
private fun SubagentRow(
    entry: FeedEntry.Subagent,
    expandedIds: Set<String>?,
    onToggleExpand: (FeedEntry) -> Unit,
    modifier: Modifier,
    onOpenThread: (String) -> Unit,
) {
    // Only worth a disclosure when there is something the one-line form hides.
    val canExpand = entry.task.isNotBlank() || !entry.result.isNullOrBlank() || entry.childThreadId != null
    var localExpanded by remember(entry.id) { mutableStateOf(false) }
    val expanded = if (expandedIds != null) entry.id in expandedIds else localExpanded
    Column(
        modifier
            .fillMaxWidth()
            .clip(MaterialTheme.shapes.small)
            .then(
                if (canExpand) {
                    Modifier.clickable {
                        if (expandedIds != null) onToggleExpand(entry) else localExpanded = !localExpanded
                    }
                } else {
                    Modifier
                },
            )
            .padding(vertical = S5Theme.spacing.tiny)
    ) {
        Row(
            Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            if (entry.active) {
                S5InlineLoading(Modifier.size(16.dp))
            } else {
                Icon(
                    if (entry.status == "failed") Icons.Rounded.ErrorOutline else if (entry.status in setOf("interrupted", "cancelled")) Icons.Rounded.WarningAmber else Icons.Rounded.CheckCircle,
                    contentDescription = null,
                    modifier = Modifier.size(16.dp),
                    tint = if (entry.status == "failed") MaterialTheme.colorScheme.error else S5Theme.status.settled,
                )
            }
            Text(entry.name, style = MaterialTheme.typography.labelLargeEmphasized)
            Text(
                entry.task,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            if (canExpand) {
                Icon(
                    if (expanded) Icons.Rounded.ExpandLess else Icons.Rounded.ExpandMore,
                    contentDescription = if (expanded) "Collapse task" else "Expand task",
                    modifier = Modifier.size(16.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        AnimatedVisibility(
            visible = expanded,
            enter = fadeIn() + expandVertically(),
            exit = fadeOut() + shrinkVertically(),
        ) {
            Column(Modifier.padding(start = 24.dp, top = S5Theme.spacing.tiny)) {
                entry.status?.let { Text(it.replace('_', ' '), style = MaterialTheme.typography.labelSmall) }
                if (entry.task.isNotBlank()) Text(entry.task, style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant)
                entry.result?.takeIf { it.isNotBlank() }?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
                entry.childThreadId?.let { child -> TextButton(onClick = { onOpenThread(child) }) { Text("Open subagent thread") } }
            }
        }
    }
}

/**
 * A folded user-input history row: the question block, plus its answer when one
 * was submitted. Quiet like a tool row — one line with a disclosure — because it
 * is a record of a prompt already answered, not a new gate.
 */
@Composable
private fun QuestionAnswerRow(
    entry: FeedEntry.QuestionAnswer,
    onOpenAttachment: (SentAttachment) -> Unit,
    expandedIds: Set<String>?,
    onToggleExpand: (FeedEntry) -> Unit,
    modifier: Modifier,
) {
    val canExpand = entry.lines.isNotEmpty()
    var localExpanded by remember(entry.id) { mutableStateOf(false) }
    val expanded = if (expandedIds != null) entry.id in expandedIds else localExpanded
    Column(
        modifier
            .fillMaxWidth()
            .clip(MaterialTheme.shapes.small)
            .then(
                if (canExpand) {
                    Modifier.clickable {
                        if (expandedIds != null) onToggleExpand(entry) else localExpanded = !localExpanded
                    }
                } else {
                    Modifier
                },
            )
            .padding(vertical = S5Theme.spacing.tiny, horizontal = S5Theme.spacing.tiny),
    ) {
        Row(
            Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Icon(
                Icons.AutoMirrored.Rounded.Chat,
                contentDescription = null,
                modifier = Modifier.size(14.dp),
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                entry.summary,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (entry.preview.isNotBlank()) {
                Text(
                    entry.preview,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurface,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
            } else {
                Box(Modifier.weight(1f))
            }
            if (canExpand) {
                Icon(
                    if (expanded) Icons.Rounded.ExpandLess else Icons.Rounded.ExpandMore,
                    contentDescription = if (expanded) "Hide answers" else "Show answers",
                    modifier = Modifier.size(16.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        AnimatedVisibility(
            visible = expanded,
            enter = fadeIn() + expandVertically(),
            exit = fadeOut() + shrinkVertically(),
        ) {
            Column(
                verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                modifier = Modifier.padding(start = 24.dp, top = S5Theme.spacing.tiny),
            ) {
                entry.lines.forEach { line ->
                    Column {
                        if (line.question.isNotBlank()) {
                            Text(
                                line.question,
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        if (line.answer.isNotBlank()) {
                            Text(
                                line.answer,
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurface,
                                modifier = Modifier.padding(start = S5Theme.spacing.small),
                            )
                        }
                        // Answered-with files link into the attachment viewer,
                        // the same destination a sent-message file chip opens.
                        line.attachments.forEach { attachment ->
                            Text(
                                attachment.name,
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.primary,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier =
                                    Modifier.padding(start = S5Theme.spacing.small)
                                        .clip(MaterialTheme.shapes.small)
                                        .clickable { onOpenAttachment(attachment) },
                            )
                        }
                    }
                }
            }
        }
    }
}

/** A `runtime.warning` row: advisory tint, no error surface. */
@Composable
private fun WarningRow(entry: FeedEntry.Warning, modifier: Modifier) {
    Row(
        modifier
            .fillMaxWidth()
            .padding(vertical = S5Theme.spacing.tiny, horizontal = S5Theme.spacing.tiny),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        Icon(
            Icons.Rounded.WarningAmber,
            contentDescription = null,
            modifier = Modifier.size(16.dp),
            tint = MaterialTheme.colorScheme.tertiary,
        )
        Text(
            entry.message,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/** An info-tone row: the quiet line RN's work log gives a check icon. */
@Composable
private fun NoteRow(entry: FeedEntry.Note, modifier: Modifier) {
    Row(
        modifier
            .fillMaxWidth()
            .padding(vertical = S5Theme.spacing.tiny, horizontal = S5Theme.spacing.tiny),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        Icon(
            Icons.Rounded.Check,
            contentDescription = null,
            modifier = Modifier.size(16.dp),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            entry.message,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun ErrorRow(
    entry: FeedEntry.ErrorEntry,
    modifier: Modifier,
    onRetryPreparation: suspend (String) -> Unit,
) {
    val scope = rememberCoroutineScope()
    var retrying by remember(entry.id) { mutableStateOf(false) }
    Surface(
        modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.errorContainer,
        contentColor = MaterialTheme.colorScheme.onErrorContainer,
    ) {
        Column(Modifier.padding(S5Theme.spacing.medium)) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                Icon(Icons.Rounded.ErrorOutline, contentDescription = null, modifier = Modifier.size(18.dp))
                Text(entry.message, style = MaterialTheme.typography.bodySmall)
            }
            // RN's WorkspacePreparationRetryButton: the run only appears retryable
            // while its workspacePreparation still ends in this failure.
            val runId = entry.retryablePreparationRunId
            if (runId != null) {
                S5Button(
                    text = "Retry",
                    onClick = {
                        if (!retrying) {
                            retrying = true
                            scope.launch {
                                try {
                                    onRetryPreparation(runId)
                                } finally {
                                    retrying = false
                                }
                            }
                        }
                    },
                    icon = Icons.Rounded.Refresh,
                    emphasis = S5ActionEmphasis.Secondary,
                    style = S5ButtonStyle.Outlined,
                    enabled = !retrying,
                    modifier = Modifier.padding(top = S5Theme.spacing.small),
                )
            }
        }
    }
}
