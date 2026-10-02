package club.touchtech.s5code.kotlin.feature.thread

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ListAlt
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.FeedEntry
import club.touchtech.s5code.kotlin.model.PlanStep
import club.touchtech.s5code.kotlin.model.PlanStepState
import club.touchtech.s5code.kotlin.model.ThreadStatus

/** The plan's current step plus its progress, or null when there is no plan. */
data class ActivePlan(val step: PlanStep, val done: Int, val total: Int)

/**
 * Latest plan in the transcript, reduced to what a one-line summary needs.
 *
 * "Latest" matters: providers republish the whole plan every time it changes, so
 * an earlier `PlanUpdate` in the feed is a stale snapshot, not a second plan.
 */
fun activePlan(feed: List<FeedEntry>): ActivePlan? {
    val plan = feed.lastOrNull { it is FeedEntry.PlanUpdate } as? FeedEntry.PlanUpdate ?: return null
    if (plan.steps.isEmpty()) return null
    val step =
        plan.steps.firstOrNull { it.state == PlanStepState.Active }
            ?: plan.steps.firstOrNull { it.state == PlanStepState.Pending }
            ?: plan.steps.last()
    return ActivePlan(
        step = step,
        done = plan.steps.count { it.state == PlanStepState.Done },
        total = plan.steps.size,
    )
}

/**
 * Whether the transcript is scrolled to the start of the thread.
 *
 * The tall header belongs to the top of the content and nowhere else. Anchoring
 * it to the live tail instead meant it opened on any small scroll back, which on
 * a long thread is most of the time.
 *
 * The transcript is reversed, so index 0 is the newest entry at the bottom and the
 * *last* index is the oldest at the top: "scrolled to the top" is
 * [lastVisibleIndex] reaching [lastIndex].
 *
 * Measured in items rather than pixels on purpose. Expanding the header shortens
 * the content viewport, which moves the scroll offset, which is the input — a
 * pixel threshold feeds back on itself and the header oscillates. An item-wide
 * dead zone ([wasAtTop] holds the answer while the oldest entry is one item out of
 * view) is larger than any height the header can give up.
 */
fun atHistoryTop(lastVisibleIndex: Int, lastIndex: Int, wasAtTop: Boolean): Boolean =
    when {
        lastIndex < 0 -> false
        wasAtTop -> lastVisibleIndex >= lastIndex - 1
        else -> lastVisibleIndex >= lastIndex
    }

/**
 * Whether a plan strip has anything to say.
 *
 * The strip reports what the agent is doing *now*. Once the turn is over that is
 * nothing, and a strip still naming the last step reads as work in progress — a
 * lying label, which is worse than no label. A blocked turn is not a finished one:
 * an approval or input request still has an agent mid-plan behind it.
 */
fun planBarApplies(status: ThreadStatus): Boolean =
    when (status) {
        ThreadStatus.Working,
        ThreadStatus.Queued,
        ThreadStatus.AwaitingApproval,
        ThreadStatus.AwaitingInput -> true
        ThreadStatus.Idle,
        ThreadStatus.Failed,
        ThreadStatus.Settled,
        ThreadStatus.Snoozed -> false
    }

/**
 * Pinned one-line plan status under the thread header.
 *
 * The plan card scrolls away with the rest of the transcript, and it holds the
 * one thing worth keeping while you read output further down: what the agent
 * thinks it is doing right now. Shown while the header is collapsed, which is
 * where the strip has space and where you are watching live work.
 */
@Composable
fun ActivePlanBar(plan: ActivePlan?, visible: Boolean, modifier: Modifier = Modifier) {
    // The bar keeps rendering the last plan it had while it animates out, so a
    // plan that disappears mid-collapse does not snap the header shut.
    val lastPlan = remember { arrayOfNulls<ActivePlan>(1) }
    if (plan != null) lastPlan[0] = plan
    val shown = plan ?: lastPlan[0]

    AnimatedVisibility(
        visible = visible && plan != null,
        enter = fadeIn() + expandVertically(),
        exit = fadeOut() + shrinkVertically(),
        modifier = modifier,
    ) {
        if (shown != null) {
            Surface(color = MaterialTheme.colorScheme.surfaceContainer) {
                Column {
                    Row(
                        Modifier.fillMaxWidth()
                            .padding(
                                horizontal = S5Theme.spacing.gutter,
                                vertical = S5Theme.spacing.small,
                            ),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                    ) {
                        Icon(
                            Icons.AutoMirrored.Rounded.ListAlt,
                            contentDescription = "Current plan step",
                            modifier = Modifier.size(16.dp),
                            tint = S5Theme.status.working,
                        )
                        Text(
                            shown.step.text,
                            style = MaterialTheme.typography.labelLarge,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f),
                        )
                        Text(
                            "${shown.done}/${shown.total}",
                            style = MaterialTheme.typography.labelMediumEmphasized,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    HorizontalDivider()
                }
            }
        }
    }
}
