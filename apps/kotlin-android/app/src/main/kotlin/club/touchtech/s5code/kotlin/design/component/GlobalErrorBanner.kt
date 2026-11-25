package club.touchtech.s5code.kotlin.design.component

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.AppErrorNotice
import kotlinx.coroutines.delay

/** Global transient error, outside every destination's scrolling content. */
@Composable
fun S5GlobalErrorBanner(
    error: AppErrorNotice?,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
    autoDismissMillis: Long = ERROR_AUTO_DISMISS_MILLIS,
) {
    val haptics = LocalHapticFeedback.current
    val dismiss by rememberUpdatedState(onDismiss)
    LaunchedEffect(error?.id) {
        if (error == null) return@LaunchedEffect
        haptics.performHapticFeedback(HapticFeedbackType.Reject)
        delay(autoDismissMillis)
        dismiss()
    }

    AnimatedVisibility(
        visible = error != null,
        modifier = modifier,
        enter = fadeIn() + slideInVertically { -it / 2 },
        exit = fadeOut() + slideOutVertically { -it / 2 },
    ) {
        error?.let { notice ->
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = MaterialTheme.shapes.large,
                color = MaterialTheme.colorScheme.errorContainer,
                contentColor = MaterialTheme.colorScheme.onErrorContainer,
                shadowElevation = 5.dp,
            ) {
                Row(
                    Modifier.padding(
                        start = S5Theme.spacing.medium,
                        end = S5Theme.spacing.tiny,
                        top = S5Theme.spacing.small,
                        bottom = S5Theme.spacing.small,
                    ),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    Icon(
                        Icons.Rounded.ErrorOutline,
                        contentDescription = null,
                        modifier = Modifier.size(20.dp),
                    )
                    Text(
                        notice.message,
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.weight(1f),
                    )
                    S5IconButton(
                        icon = Icons.Rounded.Close,
                        label = "Dismiss error",
                        onClick = onDismiss,
                    )
                }
            }
        }
    }
}

private const val ERROR_AUTO_DISMISS_MILLIS = 6_000L
