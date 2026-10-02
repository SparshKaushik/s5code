package club.touchtech.s5code.kotlin.feature.home

import club.touchtech.s5code.kotlin.model.ThreadStatus
import club.touchtech.s5code.kotlin.model.ThreadSummary

/** What a swipe on a home row commits. */
enum class ThreadSwipeAction {
    Settle,
    Unsettle,
    Snooze,
    Unsnooze,
    Archive,
}

/**
 * The two swipe actions a row offers, or fewer.
 *
 * Ported from `resolveThreadListV2SwipeActions` in
 * `apps/mobile/src/features/threads/threadListV2.ts`. Two rules from there carry
 * over unchanged because both are about not lying to the user:
 *
 * - A row on the settled shelf offers un-settle, not settle. A snoozed row offers
 *   wake, not snooze. The action names the state change, so it has to know the
 *   state it is in.
 * - A capability the server does not advertise is not offered. On this client a
 *   command the server does not understand is a protocol defect that kills the
 *   socket, so this gates the write rather than merely dimming a button.
 *
 * Deliberately not ported: the full-swipe-to-delete shortcut. Delete has no undo
 * here, and a gesture that destroys a thread on an over-swipe is the wrong
 * default; delete stays in the row's overflow menu.
 */
data class ThreadSwipeActions(
    /** Revealed by swiping from the trailing edge. Null when nothing applies. */
    val end: ThreadSwipeAction?,
    /** Revealed by swiping from the leading edge. */
    val start: ThreadSwipeAction?,
)

/** Whether this thread can be snoozed right now, matching `canSnooze`. */
fun threadSnoozable(thread: ThreadSummary): Boolean =
    // Mirrors `canSnooze` in `packages/client-runtime/src/state/threadSettled.ts`:
    // a thread waiting on the user is asking a question, and hiding it until
    // tomorrow is how that question gets lost. The queued-turn grace period that
    // function also applies needs `latestUserMessageAt`, which this client's
    // summary does not carry — the effect is that a just-queued thread can be
    // snoozed here a few seconds earlier than on RN.
    thread.status != ThreadStatus.AwaitingApproval &&
        thread.status != ThreadStatus.AwaitingInput &&
        thread.status != ThreadStatus.Queued

fun threadSwipeActions(
    thread: ThreadSummary,
    settlementSupported: Boolean,
    snoozeSupported: Boolean,
): ThreadSwipeActions {
    if (thread.status == ThreadStatus.Snoozed) {
        // A snoozed row has one job: come back. Offering settle next to it would be
        // two ways to make the same row leave the list.
        return ThreadSwipeActions(
            end = if (snoozeSupported) ThreadSwipeAction.Unsnooze else null,
            start = null,
        )
    }
    val end =
        when {
            settlementSupported && thread.status == ThreadStatus.Settled ->
                ThreadSwipeAction.Unsettle
            settlementSupported -> ThreadSwipeAction.Settle
            // Pre-settlement servers still have archive, which is the same intent
            // with a heavier hand. Matches the RN fallback.
            else -> ThreadSwipeAction.Archive
        }
    val start =
        if (snoozeSupported && thread.status != ThreadStatus.Settled && threadSnoozable(thread)) {
            ThreadSwipeAction.Snooze
        } else {
            null
        }
    return ThreadSwipeActions(end = end, start = start)
}

/** Verb shown on the revealed action, and read out by TalkBack. */
val ThreadSwipeAction.label: String
    get() =
        when (this) {
            ThreadSwipeAction.Settle -> "Settle"
            ThreadSwipeAction.Unsettle -> "Reopen"
            ThreadSwipeAction.Snooze -> "Snooze"
            ThreadSwipeAction.Unsnooze -> "Wake"
            ThreadSwipeAction.Archive -> "Archive"
        }

/**
 * The row-menu command id this action performs.
 *
 * Settle and un-settle share one command because the gateway call is a toggle
 * against the thread's current state; the two enum entries exist so the *label*
 * can tell the truth about which way it will go.
 */
val ThreadSwipeAction.command: String
    get() =
        when (this) {
            ThreadSwipeAction.Settle,
            ThreadSwipeAction.Unsettle -> "settle"
            ThreadSwipeAction.Snooze,
            ThreadSwipeAction.Unsnooze -> "snooze"
            ThreadSwipeAction.Archive -> "archive"
        }
