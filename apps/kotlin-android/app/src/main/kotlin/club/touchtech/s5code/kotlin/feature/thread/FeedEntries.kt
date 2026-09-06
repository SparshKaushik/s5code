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
import androidx.compose.material.icons.rounded.AutoAwesome
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.ExpandLess
import androidx.compose.material.icons.rounded.ExpandMore
import androidx.compose.material.icons.rounded.Image
import androidx.compose.material.icons.rounded.Psychology
import androidx.compose.material.icons.rounded.RadioButtonUnchecked
import androidx.compose.material.icons.rounded.Build
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
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
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.feature.review.ReviewCommentInlineCard
import club.touchtech.s5code.kotlin.feature.review.ReviewCommentMessageSegment
import club.touchtech.s5code.kotlin.feature.review.parseReviewCommentMessageSegments
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.FeedEntry
import club.touchtech.s5code.kotlin.model.PlanStepState
import club.touchtech.s5code.kotlin.model.ToolState
import kotlinx.coroutines.delay

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
            UserBubble(entry, modifier, resolveTranscriptAttachment)

        is FeedEntry.AgentMessage ->
            AgentMessage(
                entry,
                onCopy,
                modifier,
                workspaceRoot,
                onOpenFile,
                resolveTranscriptAttachment,
            )

        is FeedEntry.Reasoning -> ReasoningRow(entry, modifier)

        is FeedEntry.ToolCall -> ToolRow(entry, onCopy, modifier)

        is FeedEntry.PlanUpdate -> PlanCard(entry, modifier)

        is FeedEntry.Subagent -> SubagentRow(entry, modifier)

        is FeedEntry.ErrorEntry -> ErrorRow(entry, modifier)
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
    val label = workToggleLabel(row)
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
            label,
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
                val segments = remember(entry.text) { parseReviewCommentMessageSegments(entry.text) }
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
                    Text(entry.text, style = MaterialTheme.typography.bodyMedium)
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
 * Thumbnail for an image already sent on a turn. Falls back to a labeled chip
 * when the URI is no longer readable, which is expected once a clipboard grant
 * expires.
 */
@Composable
private fun SentAttachmentThumbnail(
    attachment: ComposerAttachment,
    resolveUrl: suspend () -> String?,
    onPreview: (String) -> Unit,
) {
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

/** Reasoning is collapsed by default: it is context, not the answer. */
@Composable
private fun ReasoningRow(entry: FeedEntry.Reasoning, modifier: Modifier) {
    // Keyed on the entry: a reused row must not inherit the previous entry's
    // disclosure state when the list recycles it.
    var expanded by remember(entry.id) { mutableStateOf(false) }
    S5Card(tone = S5CardTone.Receded, onClick = { expanded = !expanded }, modifier = modifier.fillMaxWidth()) {
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
                    "Thinking",
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
                Text(
                    entry.text,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = S5Theme.spacing.tiny),
                )
            }
        }
    }
}

@Composable
private fun ToolRow(entry: FeedEntry.ToolCall, onCopy: (String) -> Unit, modifier: Modifier) {
    // A chevron on a row with nothing behind it is a promise the row cannot keep.
    val canExpand = entry.detail.isNotBlank()
    var expanded by remember(entry.id) { mutableStateOf(false) }
    val tint =
        when (entry.state) {
            ToolState.Running -> S5Theme.status.working
            ToolState.Succeeded -> S5Theme.status.settled
            ToolState.Failed -> S5Theme.status.failed
        }
    S5Card(
        tone = S5CardTone.Standard,
        onClick = if (canExpand) ({ expanded = !expanded }) else null,
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
                Text(entry.name, style = S5Theme.code.codeEmphasized)
                Text(
                    entry.summary,
                    style = S5Theme.code.code,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
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
            AnimatedVisibility(
                visible = expanded,
                enter = fadeIn() + expandVertically(),
                exit = fadeOut() + shrinkVertically(),
            ) {
                Box(Modifier.padding(top = S5Theme.spacing.small)) {
                    S5CodeBlock(
                        lines = entry.detail.lines(),
                        onCopy = { onCopy(entry.detail) },
                    )
                }
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
private fun SubagentRow(entry: FeedEntry.Subagent, modifier: Modifier) {
    // Only worth a disclosure when there is something the one-line form hides.
    val canExpand = entry.task.isNotBlank()
    var expanded by remember(entry.id) { mutableStateOf(false) }
    Column(
        modifier
            .fillMaxWidth()
            .clip(MaterialTheme.shapes.small)
            .then(if (canExpand) Modifier.clickable { expanded = !expanded } else Modifier)
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
                    Icons.Rounded.CheckCircle,
                    contentDescription = null,
                    modifier = Modifier.size(16.dp),
                    tint = S5Theme.status.settled,
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
            Text(
                entry.task,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier =
                    Modifier.padding(start = 24.dp, top = S5Theme.spacing.tiny),
            )
        }
    }
}

@Composable
private fun ErrorRow(entry: FeedEntry.ErrorEntry, modifier: Modifier) {
    Surface(
        modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.errorContainer,
        contentColor = MaterialTheme.colorScheme.onErrorContainer,
    ) {
        Row(
            Modifier.padding(S5Theme.spacing.medium),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Icon(Icons.Rounded.ErrorOutline, contentDescription = null, modifier = Modifier.size(18.dp))
            Text(entry.message, style = MaterialTheme.typography.bodySmall)
        }
    }
}
