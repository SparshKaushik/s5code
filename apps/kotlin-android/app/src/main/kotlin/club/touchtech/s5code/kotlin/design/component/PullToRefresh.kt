package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.layout.BoxScope
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.pulltorefresh.PullToRefreshDefaults
import androidx.compose.material3.pulltorefresh.rememberPullToRefreshState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback

/**
 * Material pull-to-refresh with one threshold haptic and one completion haptic.
 * Keeping this in the design layer gives every refresh surface the same detent.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5PullToRefreshBox(
    isRefreshing: Boolean,
    onRefresh: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable BoxScope.() -> Unit,
) {
    val state = rememberPullToRefreshState()
    val haptics = LocalHapticFeedback.current
    val refreshing by rememberUpdatedState(isRefreshing)

    LaunchedEffect(state) {
        var armed = false
        snapshotFlow { state.distanceFraction >= 1f }
            .collect { overThreshold ->
                if (overThreshold != armed && !refreshing) {
                    haptics.performHapticFeedback(
                        if (overThreshold) HapticFeedbackType.GestureThresholdActivate
                        else HapticFeedbackType.SegmentTick
                    )
                    armed = overThreshold
                }
            }
    }
    LaunchedEffect(isRefreshing) {
        if (!isRefreshing) return@LaunchedEffect
        haptics.performHapticFeedback(HapticFeedbackType.Confirm)
    }

    PullToRefreshBox(
        isRefreshing = isRefreshing,
        onRefresh = onRefresh,
        modifier = modifier,
        state = state,
        indicator = {
            PullToRefreshDefaults.LoadingIndicator(
                state = state,
                isRefreshing = isRefreshing,
            )
        },
        content = content,
    )
}
