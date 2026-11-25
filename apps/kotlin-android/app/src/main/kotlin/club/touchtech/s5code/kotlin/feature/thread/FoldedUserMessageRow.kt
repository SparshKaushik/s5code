package club.touchtech.s5code.kotlin.feature.thread

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.ExpandLess
import androidx.compose.material.icons.rounded.ExpandMore
import androidx.compose.material.icons.rounded.Forum
import androidx.compose.material.icons.rounded.Schedule
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.FeedEntry

/** Attributions whose prompts fold to one line: other agents and automations. */
internal fun foldsUserMessage(entry: FeedEntry.UserMessage): Boolean =
    entry.attribution == "Agent" || entry.attribution == "Automation"

/**
 * The one-line summary of a folded prompt. An automation leads with its task's
 * title (`ThreadAutomationMessageRow`), else the prompt's first line; an agent
 * message shows its first non-blank line (`AgentMessageFoldHeader`).
 */
internal fun foldedUserMessageSummary(entry: FeedEntry.UserMessage, taskTitle: String?): String {
    val text = replaceComposerContextReferences(entry.text, entry.contextRecords, markUnavailable = false)
    return if (entry.attribution == "Automation") {
        taskTitle?.takeIf(String::isNotBlank) ?: text.trim().lineSequence().firstOrNull().orEmpty()
    } else {
        text.lineSequence().firstOrNull { it.isNotBlank() }?.trim().orEmpty()
    }
}

/**
 * Messages from other agents and automation prompts fold to one line so the
 * user's own messages stay easy to find (`AgentMessageFoldHeader` and
 * `ThreadAutomationMessageRow` in mobile `ThreadFeed.tsx`). Expanded, the full
 * bubble follows with a link to the sending thread or the automation.
 */
@Composable
internal fun FoldedUserMessageRow(
    entry: FeedEntry.UserMessage,
    expanded: Boolean,
    onToggle: () -> Unit,
    /** The sending thread's title, for an agent message; null falls back to "Agent". */
    senderTitle: String?,
    taskTitle: String?,
    onOpenThread: (String) -> Unit,
    onOpenAutomation: () -> Unit,
    modifier: Modifier = Modifier,
    bubble: @Composable () -> Unit,
) {
    val automation = entry.attribution == "Automation"
    val label = if (automation) "Automation" else senderTitle?.trim()?.takeIf(String::isNotEmpty) ?: "Agent"
    val summary = remember(entry.text, entry.contextRecords, taskTitle) { foldedUserMessageSummary(entry, taskTitle) }
    Column(modifier.fillMaxWidth(), horizontalAlignment = Alignment.End) {
        Surface(
            shape = MaterialTheme.shapes.largeIncreased,
            color = MaterialTheme.colorScheme.primaryContainer,
            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
            modifier = Modifier.widthIn(max = 520.dp),
        ) {
            Row(
                Modifier
                    .clickable(role = Role.Button, onClick = onToggle)
                    .semantics {
                        contentDescription = "$label: $summary"
                        stateDescription = if (expanded) "Expanded" else "Collapsed"
                    }
                    .heightIn(min = 40.dp)
                    .padding(horizontal = 14.dp, vertical = S5Theme.spacing.small),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Icon(
                    if (automation) Icons.Rounded.Schedule else Icons.Rounded.Forum,
                    contentDescription = null,
                    modifier = Modifier.size(14.dp),
                    tint = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.7f),
                )
                Text(
                    label,
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.7f),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.widthIn(max = 180.dp),
                )
                Text(
                    summary,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
                Icon(
                    if (expanded) Icons.Rounded.ExpandLess else Icons.Rounded.ExpandMore,
                    contentDescription = null,
                    modifier = Modifier.size(14.dp),
                    tint = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.7f),
                )
            }
        }
        if (expanded) {
            Column(
                Modifier.padding(top = S5Theme.spacing.tiny),
                horizontalAlignment = Alignment.End,
            ) {
                bubble()
                val senderThreadId = entry.senderThreadId
                when {
                    automation && entry.scheduledTaskId != null ->
                        S5Button(
                            text = "Open automation",
                            onClick = onOpenAutomation,
                            emphasis = S5ActionEmphasis.Secondary,
                            style = S5ButtonStyle.Text,
                        )
                    !automation && senderThreadId != null ->
                        S5Button(
                            text = "Open thread",
                            onClick = { onOpenThread(senderThreadId) },
                            emphasis = S5ActionEmphasis.Secondary,
                            style = S5ButtonStyle.Text,
                        )
                }
            }
        }
    }
}
