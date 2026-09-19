package club.touchtech.s5code.kotlin.design.component

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.OpenInNew
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import kotlinx.coroutines.delay
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.theme.S5PillShape
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.ActionProgress
import club.touchtech.s5code.kotlin.model.ActionProgressPhase

/** Floating progress/result banner for VCS and rewind operations. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5ActionProgressOverlay(
    progress: ActionProgress?,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val haptics = LocalHapticFeedback.current
    val uriHandler = LocalUriHandler.current
    val current by rememberUpdatedState(progress)

    LaunchedEffect(progress?.id, progress?.phase) {
        when (progress?.phase) {
            ActionProgressPhase.Success -> haptics.performHapticFeedback(HapticFeedbackType.Confirm)
            ActionProgressPhase.Error -> haptics.performHapticFeedback(HapticFeedbackType.Reject)
            else -> Unit
        }
        if (progress?.phase != null && progress.phase != ActionProgressPhase.Running) {
            delay(ACTION_RESULT_VISIBLE_MILLIS)
            // A newer action may replace this result while the timer runs.
            if (current?.id == progress.id && current?.phase == progress.phase) onDismiss()
        }
    }

    AnimatedVisibility(
        visible = progress != null,
        modifier = modifier,
        enter = fadeIn() + scaleIn(initialScale = 0.96f),
        exit = fadeOut() + scaleOut(targetScale = 0.96f),
    ) {
        current?.let { active ->
            val error = active.phase == ActionProgressPhase.Error
            Surface(
                modifier =
                    Modifier.fillMaxWidth()
                        .clickable {
                            if (active.linkUrl != null) uriHandler.openUri(active.linkUrl)
                            else if (active.phase != ActionProgressPhase.Running) onDismiss()
                        },
                shape = MaterialTheme.shapes.extraLarge,
                color =
                    if (error) MaterialTheme.colorScheme.errorContainer
                    else MaterialTheme.colorScheme.surfaceContainerHigh,
                contentColor =
                    if (error) MaterialTheme.colorScheme.onErrorContainer
                    else MaterialTheme.colorScheme.onSurface,
                tonalElevation = 4.dp,
                shadowElevation = 6.dp,
            ) {
                Row(
                    Modifier.padding(
                        horizontal = S5Theme.spacing.large,
                        vertical = S5Theme.spacing.medium,
                    ),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                ) {
                    AnimatedContent(
                        targetState = active.phase,
                        transitionSpec = { fadeIn() togetherWith fadeOut() },
                        label = "action phase",
                    ) { phase ->
                        when (phase) {
                            ActionProgressPhase.Running ->
                                LoadingIndicator(Modifier.size(24.dp))
                            ActionProgressPhase.Success ->
                                Box(
                                    Modifier.size(24.dp),
                                    contentAlignment = Alignment.Center,
                                ) {
                                    Icon(
                                        Icons.Rounded.Check,
                                        contentDescription = "Succeeded",
                                        tint = S5Theme.status.settled,
                                    )
                                }
                            ActionProgressPhase.Error ->
                                Icon(
                                    Icons.Rounded.ErrorOutline,
                                    contentDescription = "Failed",
                                    modifier = Modifier.size(24.dp),
                                )
                        }
                    }
                    Column(Modifier.weight(1f)) {
                        Text(
                            active.label,
                            style = MaterialTheme.typography.labelLargeEmphasized,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        active.description?.let { description ->
                            Text(
                                description,
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                    when {
                        active.linkUrl != null ->
                            Icon(
                                Icons.AutoMirrored.Rounded.OpenInNew,
                                contentDescription = "Open result",
                                modifier = Modifier.size(18.dp),
                            )
                        active.phase != ActionProgressPhase.Running ->
                            Icon(
                                Icons.Rounded.Close,
                                contentDescription = "Dismiss",
                                modifier = Modifier.size(18.dp),
                            )
                    }
                }
            }
        }
    }
}

internal const val ACTION_RESULT_VISIBLE_MILLIS = 4_000L
