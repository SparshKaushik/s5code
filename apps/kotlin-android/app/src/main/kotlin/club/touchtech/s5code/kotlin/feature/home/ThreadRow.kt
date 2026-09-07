package club.touchtech.s5code.kotlin.feature.home

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.CallSplit
import androidx.compose.material.icons.automirrored.rounded.MergeType
import androidx.compose.material.icons.rounded.EditNote
import androidx.compose.material.icons.rounded.PushPin
import androidx.compose.material.icons.rounded.Sync
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5ProjectIcon
import club.touchtech.s5code.kotlin.design.component.S5ProviderMark
import club.touchtech.s5code.kotlin.design.component.S5StatusPill
import club.touchtech.s5code.kotlin.design.component.highlightedText
import club.touchtech.s5code.kotlin.design.component.statusPresentation
import club.touchtech.s5code.kotlin.design.theme.S5PillShape
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.data.elapsedLabel
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.PullRequestState
import club.touchtech.s5code.kotlin.model.SearchMatch
import club.touchtech.s5code.kotlin.model.SearchMatchSource
import club.touchtech.s5code.kotlin.model.ThreadStatus
import club.touchtech.s5code.kotlin.model.ThreadSummary

/**
 * One thread row. Hierarchy comes from the project's own icon, the status pill's
 * color pair, and surface containment — attention-needing threads get the hero
 * container so they read first without animation.
 *
 * The leading slot is the project, not the agent, matching the RN row: which
 * codebase a thread belongs to is what a user scans a list for, and the agent is
 * a detail of how the work is being done. The provider keeps a small glyph on the
 * meta line.
 */
@Composable
fun ThreadRow(
    thread: ThreadSummary,
    project: Project?,
    environmentLabel: String?,
    onClick: () -> Unit,
    resolveProjectIconUrl: suspend (Project) -> String?,
    modifier: Modifier = Modifier,
    /** First line of the thread's unsent draft, when it has one. */
    draftPreview: String? = null,
    /** Visible field that satisfied Home search, highlighted inline below the title. */
    searchMatch: SearchMatch? = null,
    searchQuery: String = "",
    trailing: @Composable (() -> Unit)? = null,
) {
    val status = statusPresentation(thread.status)
    val elapsedLabel = rememberLiveElapsedLabel(thread)
    val needsAttention =
        thread.status == ThreadStatus.AwaitingApproval || thread.status == ThreadStatus.AwaitingInput
    val titleMatch = searchMatch?.takeIf { it.source == SearchMatchSource.Title }
    S5Card(
        modifier = modifier.fillMaxWidth(),
        tone = if (needsAttention) S5CardTone.Hero else S5CardTone.Standard,
        onClick = onClick,
    ) {
        Row(
            Modifier.padding(S5Theme.spacing.large),
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            S5ProjectIcon(
                project = project,
                resolveUrl = resolveProjectIconUrl,
                size = 32.dp,
            )
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    if (thread.pinned) {
                        Icon(
                            Icons.Rounded.PushPin,
                            contentDescription = "Pinned",
                            modifier = Modifier.size(14.dp),
                            tint = MaterialTheme.colorScheme.primary,
                        )
                    }
                    Text(
                        text =
                            if (titleMatch == null) androidx.compose.ui.text.AnnotatedString(thread.title)
                            else
                                highlightedText(
                                    text = thread.title,
                                    query = searchQuery,
                                    normalColor = MaterialTheme.colorScheme.onSurface,
                                    highlightColor = MaterialTheme.colorScheme.primary,
                                ),
                        style = MaterialTheme.typography.titleSmallEmphasized,
                        // One line, like the RN list. A wrapped title makes rows
                        // different heights, which turns an even list into a
                        // ragged one and costs the row's real signal (status,
                        // excerpt, project) vertical space.
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                }
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    TurnStatusIndicator(thread.status)
                    S5StatusPill(
                        label = elapsedLabel?.let { "${status.label} · $it" } ?: status.label,
                        containerColor = status.container,
                        contentColor = status.content,
                    )
                    Text(
                        thread.updatedLabel,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                val excerpt = thread.lastError ?: thread.excerpt
                val subtitleMatch =
                    searchMatch?.takeIf {
                        it.source == SearchMatchSource.Excerpt ||
                            it.source == SearchMatchSource.UserMessage ||
                            it.source == SearchMatchSource.AssistantMessage
                    }
                if (subtitleMatch != null) {
                    SearchMatchLine(subtitleMatch, searchQuery)
                } else if (excerpt != null) {
                    Text(
                        text = excerpt,
                        style = MaterialTheme.typography.bodySmall,
                        color =
                            if (thread.lastError != null) MaterialTheme.colorScheme.error
                            else MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                if (
                    searchMatch != null &&
                        searchMatch !== titleMatch &&
                        searchMatch !== subtitleMatch
                ) {
                    SearchMatchLine(searchMatch, searchQuery)
                }
                if (draftPreview != null) DraftLine(draftPreview)
                ThreadMetaRow(thread, project, environmentLabel)
            }
            if (trailing != null) {
                Box(Modifier.padding(start = S5Theme.spacing.tiny)) { trailing() }
            }
        }
    }
}

/**
 * Only a visible working row ticks, once per second. This keeps the Home card
 * honest without rebuilding every thread projection or running a process-wide
 * high-frequency clock while Home is not composed.
 */
@Composable
private fun rememberLiveElapsedLabel(thread: ThreadSummary): String? {
    val startedAt =
        thread.activeTurnStartedAtMillis?.takeIf { thread.status == ThreadStatus.Working }
            ?: return thread.elapsedLabel
    var nowMillis by remember(startedAt) { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(startedAt) {
        while (true) {
            val elapsed = System.currentTimeMillis() - startedAt
            kotlinx.coroutines.delay((1_000 - elapsed.mod(1_000L)).coerceAtLeast(1L))
            nowMillis = System.currentTimeMillis()
        }
    }
    return elapsedLabel(startedAt, nowMillis)
}

/**
 * Turn liveness without a perpetual spinner. Status changes animate once; a
 * working turn's sync glyph rotates on entry and then comes to rest, so a long
 * run does not force the list to repaint indefinitely.
 */
@Composable
private fun TurnStatusIndicator(status: ThreadStatus) {
    val presentation = statusPresentation(status)
    val rotation = remember { Animatable(0f) }
    LaunchedEffect(status) {
        rotation.snapTo(0f)
        if (status == ThreadStatus.Working) {
            rotation.animateTo(360f, animationSpec = tween(durationMillis = 650))
        }
    }
    Box(
        Modifier.size(15.dp).clip(CircleShape).background(presentation.accent),
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            if (status == ThreadStatus.Working) Icons.Rounded.Sync else presentation.icon,
            contentDescription = presentation.label,
            modifier =
                Modifier.size(9.dp).graphicsLayer {
                    rotationZ = rotation.value
                },
            tint = presentation.onAccent,
        )
    }
}

@Composable
private fun SearchMatchLine(match: SearchMatch, query: String) {
    val normal = MaterialTheme.colorScheme.onSurfaceVariant
    val highlighted = MaterialTheme.colorScheme.onSurface
    val prefix =
        when (match.source) {
            SearchMatchSource.UserMessage -> "You: "
            SearchMatchSource.AssistantMessage -> "Agent: "
            else -> "${match.source.label}: "
        }
    Text(
        text =
            androidx.compose.ui.text.buildAnnotatedString {
                append(prefix)
                append(
                    highlightedText(
                        text = match.text,
                        query = query,
                        normalColor = normal,
                        highlightColor = highlighted,
                    )
                )
            },
        style = MaterialTheme.typography.bodySmall,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
    )
}

/**
 * The thread's unsent draft, one line.
 *
 * Below the excerpt rather than replacing it: the excerpt is what the agent said and
 * the draft is what you were about to say, and a row that hid the former to show the
 * latter would make the list read differently depending on where your cursor was
 * last. The pencil and the tinted text are the whole signal, matching the web
 * sidebar's amber draft row — no pill, because this is not a status.
 */
@Composable
private fun DraftLine(preview: String) {
    Row(
        Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
    ) {
        Icon(
            Icons.Rounded.EditNote,
            contentDescription = "Unsent draft",
            modifier = Modifier.size(14.dp),
            tint = S5Theme.status.approval,
        )
        Text(
            preview,
            style = MaterialTheme.typography.bodySmall,
            color = S5Theme.status.approval,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/** Branch, project, environment, diffstat, and PR state in one dense line. */
@Composable
private fun ThreadMetaRow(thread: ThreadSummary, project: Project?, environmentLabel: String?) {
    val meta = S5Theme.code.inlineTechnical
    Row(
        Modifier.fillMaxWidth().padding(top = S5Theme.spacing.tiny),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        if (thread.branch != null) {
            Surface(
                modifier = Modifier.weight(1f, fill = false),
                shape = S5PillShape,
                color = MaterialTheme.colorScheme.surfaceContainerHigh,
                contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
            ) {
                Row(
                    Modifier.padding(
                        horizontal = S5Theme.spacing.small,
                        vertical = S5Theme.spacing.tiny,
                    ),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
                ) {
                    Icon(
                        Icons.AutoMirrored.Rounded.CallSplit,
                        contentDescription = "Branch",
                        modifier = Modifier.size(12.dp),
                    )
                    Text(
                        thread.branch,
                        style = meta,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
        val label = listOfNotNull(project?.title, environmentLabel).joinToString(" · ")
        if (label.isNotEmpty()) {
            Text(
                label,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (thread.changedFiles > 0) {
            Text(
                "+${thread.additions}",
                style = meta,
                color = S5Theme.status.added,
            )
            Text("−${thread.deletions}", style = meta, color = S5Theme.status.removed)
        }
        val pr = thread.pullRequest
        if (pr != null) {
            Icon(
                if (pr.state == PullRequestState.Merged) Icons.AutoMirrored.Rounded.MergeType
                else Icons.AutoMirrored.Rounded.CallSplit,
                contentDescription = "Pull request",
                modifier = Modifier.size(12.dp),
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                "#${pr.number}",
                style = meta,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        // The agent, last and quiet. It matters when scanning for "which of these did
        // Claude run", and never more than the branch or the project.
        S5ProviderMark(
            provider = thread.provider,
            contentDescription = thread.provider.label,
            size = 12.dp,
            tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
        )
    }
}
