package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.theme.S5Theme

/** One revealed swipe action: what it looks like and what it does. */
data class S5SwipeAction(
    val label: String,
    val icon: ImageVector,
    val containerColor: Color,
    val contentColor: Color,
    val onAction: () -> Unit,
)

/**
 * A row with up to two swipe actions, one per edge.
 *
 * Built on `SwipeToDismissBox` rather than a hand-rolled draggable: the M3
 * component already owns the anchored drag, the velocity threshold, and the RTL
 * mirroring, and reimplementing those is how a list ends up with a gesture that
 * feels almost right.
 *
 * Two decisions worth recording:
 *
 * - **The row snaps back.** `SwipeToDismissBox` is a dismiss component, so its
 *   natural end state is "gone". Here the action fires and the box resets, because
 *   settle and snooze change a thread's state rather than remove a row from
 *   existence — and the list re-partitions on its own once the state lands. A row
 *   left dismissed would be a second, stale source of truth for a moment.
 * - **The background is static.** No progress-driven scale, stretch, or color
 *   ramp. Those are the continuously repainting transforms the perf rules call
 *   out, and on a list of rows they are paid per visible row.
 */
@Composable
fun S5SwipeableRow(
    /** Trailing-edge action (swipe right-to-left in LTR). */
    endAction: S5SwipeAction?,
    /** Leading-edge action (swipe left-to-right in LTR). */
    startAction: S5SwipeAction?,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    if (endAction == null && startAction == null) {
        Box(modifier) { content() }
        return
    }

    // rememberUpdatedState so a re-render between the gesture starting and the
    // threshold being crossed cannot fire a stale lambda against a moved row.
    val end by rememberUpdatedState(endAction)
    val start by rememberUpdatedState(startAction)
    val haptics = LocalHapticFeedback.current
    val state =
        rememberSwipeToDismissBoxState(
            positionalThreshold = { distance -> distance * SWIPE_ACTION_THRESHOLD },
        )

    // One tactile detent when the drag crosses or leaves the commit threshold.
    // `targetValue` changes at the anchor threshold, unlike offset/progress which
    // would vibrate throughout the gesture.
    LaunchedEffect(state) {
        var armed = false
        snapshotFlow { state.targetValue }
            .collect { target ->
                val crossed = target != SwipeToDismissBoxValue.Settled
                if (crossed != armed) {
                    haptics.performHapticFeedback(
                        if (crossed) HapticFeedbackType.GestureThresholdActivate
                        else HapticFeedbackType.SegmentTick
                    )
                    armed = crossed
                }
            }
    }

    // If the available actions change (e.g. thread status changed from Snoozed to
    // Active, or row was rebound in LazyColumn), force the box back to Settled so
    // it cannot remain dismissed off-screen with a stale action background.
    LaunchedEffect(end?.label, start?.label) {
        if (state.currentValue != SwipeToDismissBoxValue.Settled) {
            state.snapTo(SwipeToDismissBoxValue.Settled)
        }
    }

    // Fire on commit, then reset immediately. Snapping back to Settled before
    // invoking onAction ensures the row is already resting at 0-offset when the
    // list re-partitions or animates the item across shelves, preventing the card
    // from remaining stuck off-screen.
    LaunchedEffect(state.currentValue) {
        when (state.currentValue) {
            SwipeToDismissBoxValue.EndToStart -> {
                haptics.performHapticFeedback(HapticFeedbackType.Confirm)
                val action = end?.onAction
                state.snapTo(SwipeToDismissBoxValue.Settled)
                action?.invoke()
            }
            SwipeToDismissBoxValue.StartToEnd -> {
                haptics.performHapticFeedback(HapticFeedbackType.Confirm)
                val action = start?.onAction
                state.snapTo(SwipeToDismissBoxValue.Settled)
                action?.invoke()
            }
            SwipeToDismissBoxValue.Settled -> Unit
        }
    }

    SwipeToDismissBox(
        state = state,
        modifier = modifier,
        enableDismissFromEndToStart = endAction != null,
        enableDismissFromStartToEnd = startAction != null,
        backgroundContent = {
            // Which edge is being revealed decides which action is drawn. Reading
            // dismissDirection rather than the offset keeps this a single
            // recomposition per direction change instead of one per frame.
            val action =
                when (state.dismissDirection) {
                    SwipeToDismissBoxValue.EndToStart -> end
                    SwipeToDismissBoxValue.StartToEnd -> start
                    SwipeToDismissBoxValue.Settled -> null
                }
            if (action != null) {
                SwipeActionBackground(
                    action = action,
                    alignment =
                        if (state.dismissDirection == SwipeToDismissBoxValue.EndToStart) {
                            Alignment.CenterEnd
                        } else {
                            Alignment.CenterStart
                        },
                )
            }
        },
        content = { content() },
    )
}

@Composable
private fun SwipeActionBackground(action: S5SwipeAction, alignment: Alignment) {
    Surface(
        Modifier.fillMaxSize(),
        shape = MaterialTheme.shapes.large,
        color = action.containerColor,
        contentColor = action.contentColor,
    ) {
        Box(Modifier.fillMaxSize(), contentAlignment = alignment) {
            Column(
                Modifier.padding(horizontal = S5Theme.spacing.xLarge),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.hair),
            ) {
                Icon(action.icon, contentDescription = null, modifier = Modifier.size(22.dp))
                Text(action.label, style = MaterialTheme.typography.labelSmallEmphasized)
            }
        }
    }
}

/** A deliberate, slightly resistant commit point rather than the M3 half-row default. */
private const val SWIPE_ACTION_THRESHOLD = 0.62f
