package club.touchtech.s5code.kotlin.feature.home

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.OpenInNew
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import club.touchtech.s5code.kotlin.data.ApprovalDecision
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5ProjectIcon
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.feature.thread.ApprovalCard
import club.touchtech.s5code.kotlin.feature.thread.UserInputCard
import club.touchtech.s5code.kotlin.model.HomeListItem
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.UserInputAnswer

/**
 * Actionable approval gate embedded in Home.
 *
 * The compact thread identity line prevents a pending request from becoming an
 * anonymous command when several environments need attention at once. The shared
 * thread card below owns the actual response behavior, so Home and Thread cannot
 * drift on the decisions they send.
 */
@Composable
fun HomeApprovalCard(
    item: HomeListItem.PendingApprovalCard,
    resolveProjectIconUrl: suspend (Project) -> String?,
    onOpenThread: () -> Unit,
    onDecision: (ApprovalDecision) -> Unit,
    submitting: Boolean,
    modifier: Modifier = Modifier,
) {
    S5Card(tone = S5CardTone.Hero, modifier = modifier.fillMaxWidth()) {
        Column(
            Modifier.padding(S5Theme.spacing.medium),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            PendingThreadIdentity(item.thread.title, item.project, item.environmentLabel, resolveProjectIconUrl)
            ApprovalCard(
                approval = item.approval,
                onDecision = onDecision,
                submitting = submitting,
            )
            S5Button(
                text = "Open thread",
                onClick = onOpenThread,
                icon = Icons.AutoMirrored.Rounded.OpenInNew,
                emphasis = S5ActionEmphasis.Secondary,
                style = S5ButtonStyle.Text,
            )
        }
    }
}

/** Structured-input counterpart of [HomeApprovalCard]. */
@Composable
fun HomeUserInputCard(
    item: HomeListItem.PendingInputCard,
    resolveProjectIconUrl: suspend (Project) -> String?,
    onOpenThread: () -> Unit,
    onSubmit: (Map<String, UserInputAnswer>) -> Unit,
    submitting: Boolean,
    modifier: Modifier = Modifier,
) {
    S5Card(tone = S5CardTone.Hero, modifier = modifier.fillMaxWidth()) {
        Column(
            Modifier.padding(S5Theme.spacing.medium),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            PendingThreadIdentity(item.thread.title, item.project, item.environmentLabel, resolveProjectIconUrl)
            UserInputCard(request = item.request, onSubmit = onSubmit, submitting = submitting)
            S5Button(
                text = "Open thread",
                onClick = onOpenThread,
                icon = Icons.AutoMirrored.Rounded.OpenInNew,
                emphasis = S5ActionEmphasis.Secondary,
                style = S5ButtonStyle.Text,
            )
        }
    }
}

@Composable
private fun PendingThreadIdentity(
    title: String,
    project: Project?,
    environmentLabel: String?,
    resolveProjectIconUrl: suspend (Project) -> String?,
) {
    Row(
        Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        S5ProjectIcon(project = project, resolveUrl = resolveProjectIconUrl)
        Column(Modifier.weight(1f)) {
            Text(
                title,
                style = MaterialTheme.typography.titleSmallEmphasized,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            listOfNotNull(project?.title, environmentLabel)
                .takeIf { it.isNotEmpty() }
                ?.let { labels ->
                    Text(
                        labels.joinToString(" · "),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
        }
    }
}
