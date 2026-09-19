package club.touchtech.s5code.kotlin.feature.thread

/**
 * Whether the transcript should keep pinning itself to the newest entry.
 *
 * Ported from the web client's live-follow rules (`ChatView.tsx`,
 * `timelineScrollModeRef` and `showScrollToBottom`): scrolling away from the live
 * edge stops the follow, and returning to the edge resumes it. A phone needs the
 * same behavior for the same reason — an agent that streams while you are reading
 * history must not yank the viewport.
 *
 * Indices are as a reversed [androidx.compose.foundation.lazy.LazyColumn] reports
 * them, so index 0 is the newest row and offset grows as it scrolls off the bottom.
 *
 * The two thresholds are hysteresis. A single threshold sits exactly where the
 * autoscroll animation settles, so a growing transcript flips the state on every
 * appended row and the jump button strobes.
 */
fun shouldFollowTail(
    firstVisibleIndex: Int,
    firstVisibleOffsetPx: Int,
    wasFollowing: Boolean,
): Boolean {
    if (firstVisibleIndex > 0) return false
    return if (wasFollowing) firstVisibleOffsetPx <= FOLLOW_EXIT_PX
    else firstVisibleOffsetPx <= FOLLOW_ENTER_PX
}

/**
 * How close to the newest row counts as "at the live edge" when resuming follow.
 * Roughly a finger's slop, so letting go a few pixels short still re-pins.
 */
private const val FOLLOW_ENTER_PX = 24

/**
 * How far the newest row may scroll off before follow is dropped. Larger than the
 * enter threshold on purpose: this is the gap that keeps an appended row from
 * toggling the state.
 */
private const val FOLLOW_EXIT_PX = 96
