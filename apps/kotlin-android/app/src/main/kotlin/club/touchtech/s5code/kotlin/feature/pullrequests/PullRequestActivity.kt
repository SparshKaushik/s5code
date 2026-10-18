package club.touchtech.s5code.kotlin.feature.pullrequests

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Chat
import androidx.compose.material.icons.automirrored.rounded.Comment
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.Commit
import androidx.compose.material.icons.rounded.Description
import androidx.compose.material.icons.rounded.SwapVert
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.data.PullRequestActivityFilter
import club.touchtech.s5code.kotlin.data.PullRequestActivityKind
import club.touchtech.s5code.kotlin.data.PullRequestTimelineEvent
import club.touchtech.s5code.kotlin.data.filterPullRequestActivity
import club.touchtech.s5code.kotlin.data.pullRequestTimeline
import club.touchtech.s5code.kotlin.data.relativeLabel
import club.touchtech.s5code.kotlin.design.component.S5FilterMenu
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.component.S5MenuOption
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5ToggleButton
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.transport.wire.PullRequestActivityDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestActorDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestCommentDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestCommitDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewThreadDto

private const val COMMENT_COLLAPSED_HEIGHT = 240

/**
 * The Activity segment: filter controls, then comments, discussions and
 * commits by date. A discussion opens its thread; a commit, its diff.
 */
fun LazyListScope.pullRequestActivityItems(
    activity: PullRequestActivityDto?,
    filter: PullRequestActivityFilter,
    onFilterChange: (PullRequestActivityFilter) -> Unit,
    loading: Boolean,
    error: String?,
    onOpenDiscussion: (threadId: String) -> Unit,
    onOpenCommit: (oid: String) -> Unit,
) {
    if (activity == null) {
        item(key = "activity-state") {
            if (error != null) {
                S5Notice(icon = Icons.Rounded.CloudOff, text = error)
            } else if (loading) {
                Box(Modifier.fillMaxWidth().padding(S5Theme.spacing.xxLarge), contentAlignment = Alignment.Center) { S5InlineLoading() }
            } else {
                Text(
                    "Activity isn't saved on this device yet.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        return
    }
    val filtered = filterPullRequestActivity(activity, filter)
    val timeline = pullRequestTimeline(filtered.activity, filter.newestFirst)
    item(key = "activity-filters") { ActivityFilters(filter, filtered.hiddenCount, onFilterChange) }
    if (error != null) item(key = "activity-error") { S5Notice(icon = Icons.Rounded.CloudOff, text = error) }
    if (timeline.events.isEmpty()) {
        item(key = "activity-empty") {
            Text(
                "No activity matches these filters.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(vertical = S5Theme.spacing.large),
            )
        }
    }
    items(timeline.events, key = { it.key }) { event ->
        when (event) {
            is PullRequestTimelineEvent.Comment -> TimelineComment(event.comment)
            is PullRequestTimelineEvent.Discussion ->
                TimelineDiscussion(event.thread, onClick = { onOpenDiscussion(event.thread.id) })
            is PullRequestTimelineEvent.Commit -> TimelineCommit(event.commit, onClick = { onOpenCommit(event.commit.oid) })
        }
    }
    if (timeline.truncated) {
        item(key = "activity-truncated") {
            Text(
                "More activity is available on the provider.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(vertical = S5Theme.spacing.medium),
            )
        }
    }
}

@Composable
private fun ActivityFilters(filter: PullRequestActivityFilter, hiddenCount: Int, onChange: (PullRequestActivityFilter) -> Unit) {
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        S5FilterMenu(
            label = filter.kind.label,
            active = filter.kind != PullRequestActivityKind.All,
            options = PullRequestActivityKind.entries.map { S5MenuOption(it.name, it.label, selected = it == filter.kind) },
            onSelect = { onChange(filter.copy(kind = PullRequestActivityKind.valueOf(it))) },
        )
        S5ToggleButton(text = "Bot comments", checked = filter.bots, onCheckedChange = { onChange(filter.copy(bots = it)) })
        S5ToggleButton(
            text = "Resolved or dismissed",
            checked = filter.resolved,
            onCheckedChange = { onChange(filter.copy(resolved = it)) },
        )
        S5ToggleButton(
            text = if (filter.newestFirst) "Newest" else "Oldest",
            checked = false,
            icon = Icons.Rounded.SwapVert,
            onCheckedChange = { onChange(filter.copy(newestFirst = !filter.newestFirst)) },
        )
        if (hiddenCount > 0) {
            Text("$hiddenCount hidden", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** One timeline row: a tonal glyph, then its content. */
@Composable
private fun TimelineEvent(icon: ImageVector, tint: Color, content: @Composable () -> Unit) {
    Row(horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium)) {
        Box(
            Modifier.size(32.dp).clip(CircleShape).background(MaterialTheme.colorScheme.surfaceContainerHigh),
            contentAlignment = Alignment.Center,
        ) {
            Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size(16.dp))
        }
        Column(Modifier.weight(1f).padding(top = S5Theme.spacing.tiny), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
            content()
        }
    }
}

@Composable
private fun EventHeader(actor: PullRequestActorDto?, action: String, date: String) {
    val now = remember(date) { System.currentTimeMillis() }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
        PullRequestAvatar(actor, size = 22.dp)
        Text(
            buildAnnotatedString {
                withStyle(SpanStyle(fontWeight = FontWeight.Medium)) { append(actor?.login ?: "Unknown author") }
                withStyle(SpanStyle(color = MaterialTheme.colorScheme.onSurfaceVariant)) { append(" $action") }
            },
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.weight(1f),
        )
        Text(relativeLabel(date, now), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun TimelineComment(comment: PullRequestCommentDto) {
    val approved = comment.reviewState?.uppercase() == "APPROVED"
    val changes = comment.reviewState?.uppercase()?.replace("-", "_") == "CHANGES_REQUESTED"
    TimelineEvent(
        icon = if (approved) Icons.Rounded.Check else if (changes) Icons.Rounded.Description else Icons.AutoMirrored.Rounded.Comment,
        tint = if (approved) S5Theme.status.settled else if (changes) S5Theme.status.failed else MaterialTheme.colorScheme.onSurfaceVariant,
    ) {
        EventHeader(comment.author, if (approved) "approved" else if (changes) "requested changes" else "commented", comment.createdAt)
        comment.path?.let {
            Text(it, style = S5Theme.code.code, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
        if (comment.body.isNotBlank()) {
            Box(
                Modifier.fillMaxWidth()
                    .clip(MaterialTheme.shapes.large)
                    .background(MaterialTheme.colorScheme.surfaceContainer)
                    .padding(S5Theme.spacing.medium)
            ) {
                CollapsibleMarkdown(comment.body, COMMENT_COLLAPSED_HEIGHT.dp)
            }
        }
    }
}

@Composable
private fun TimelineDiscussion(thread: PullRequestReviewThreadDto, onClick: () -> Unit) {
    val first = thread.comments.firstOrNull()
    TimelineEvent(
        icon = Icons.AutoMirrored.Rounded.Chat,
        tint = if (thread.isResolved) S5Theme.status.settled else MaterialTheme.colorScheme.onSurfaceVariant,
    ) {
        EventHeader(first?.author, "started a discussion", first?.createdAt.orEmpty())
        Column(
            Modifier.fillMaxWidth()
                .clip(MaterialTheme.shapes.large)
                .background(MaterialTheme.colorScheme.surfaceContainer)
                .clickable(onClick = onClick)
                .padding(S5Theme.spacing.medium),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
        ) {
            Text(
                thread.path + (thread.line?.let { ":$it" } ?: ""),
                style = S5Theme.code.code,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                (if (thread.isResolved) "Resolved" else "Unresolved") +
                    (if (thread.isOutdated) " · Outdated" else "") +
                    " · ${thread.commentCount ?: thread.comments.size} comments",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun TimelineCommit(commit: PullRequestCommitDto, onClick: () -> Unit) {
    TimelineEvent(icon = Icons.Rounded.Commit, tint = MaterialTheme.colorScheme.onSurfaceVariant) {
        Row(
            modifier = Modifier.fillMaxWidth().clip(MaterialTheme.shapes.small).clickable(onClick = onClick),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
            PullRequestAvatar(commit.authors?.firstOrNull(), size = 22.dp)
            Text(commit.messageHeadline, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            Text(commit.oid.take(7), style = S5Theme.code.code, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}
