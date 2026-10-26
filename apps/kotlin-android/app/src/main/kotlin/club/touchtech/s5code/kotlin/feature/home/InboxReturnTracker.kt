package club.touchtech.s5code.kotlin.feature.home

import club.touchtech.s5code.kotlin.model.ThreadStatus
import club.touchtech.s5code.kotlin.model.ThreadSummary

/**
 * How long a thread that stopped working keeps reading as working
 * (`WORKING_EXIT_GRACE_MS` in client-runtime state/threadInbox.ts). A run can
 * end moments before its continuation starts, and a server auto-settle lands
 * inside this window, so the thread goes straight from Working to Settled
 * instead of flashing in the inbox.
 */
const val WORKING_EXIT_GRACE_MS = 2_000L

/**
 * `isThreadWorking`: running, or waiting on work it will resume from (another
 * thread, background tasks). A thread that needs the user is never working.
 */
fun isThreadWorking(thread: ThreadSummary): Boolean =
    thread.status == ThreadStatus.Working || thread.status == ThreadStatus.Waiting

private fun ThreadSummary.trackerKey() = "${environmentId.value}:${id.value}"

/**
 * `createInboxReturnTracker`: holds a thread on the Working shelf for
 * [WORKING_EXIT_GRACE_MS] after it stops working, and remembers when each
 * thread came back so the inbox can order by return. Only shelf placement is
 * held: a request for the user, a failure, or a move by the user (pin, settle,
 * snooze, archive) ends the hold at once.
 *
 * Kept at module scope ([homeInboxReturns]) like RN's, so placement survives
 * the list leaving composition. [observe] is pure apart from the tracker's own
 * state; the caller re-observes at [Observation.nextDeadlineMillis].
 */
class InboxReturnTracker {
    private class State(val working: Boolean, val deadline: Long?, val placement: String)

    private val states = HashMap<String, State>()
    private val returns = HashMap<String, Long>()

    class Observation(
        private val working: Map<String, Boolean>,
        private val returns: Map<String, Long>,
        /** When a held thread's grace ends; re-observe then. */
        val nextDeadlineMillis: Long?,
    ) {
        fun isWorking(thread: ThreadSummary): Boolean = working[thread.trackerKey()] ?: isThreadWorking(thread)

        fun returnedAt(thread: ThreadSummary): Long? = returns[thread.trackerKey()]
    }

    /** Observes the current shells, or clears everything with null while the shelf is off. */
    @Synchronized
    fun observe(threads: List<ThreadSummary>?, nowMillis: Long): Observation {
        if (threads == null) {
            states.clear()
            returns.clear()
            return Observation(emptyMap(), emptyMap(), null)
        }
        val present = HashSet<String>()
        for (thread in threads) {
            val key = thread.trackerKey()
            present += key
            val placement =
                listOf(
                        thread.archived,
                        thread.pinned,
                        thread.status == ThreadStatus.Settled,
                        thread.snoozedUntil,
                        thread.activeOrderKey,
                    )
                    .joinToString("|")
            val previous = states[key]
            val actionable =
                thread.status == ThreadStatus.AwaitingApproval ||
                    thread.status == ThreadStatus.AwaitingInput ||
                    thread.status == ThreadStatus.Failed ||
                    thread.status == ThreadStatus.Limited ||
                    thread.hasPendingRuntimeRequest ||
                    thread.lastError != null
            val moved = previous != null && previous.placement != placement
            val excluded =
                thread.archived ||
                    thread.pinned ||
                    thread.status == ThreadStatus.Settled ||
                    thread.status == ThreadStatus.Snoozed
            val actual = isThreadWorking(thread)
            val deadline =
                if (!actual && previous?.working == true && !actionable && !moved && !excluded) {
                    previous.deadline ?: (nowMillis + WORKING_EXIT_GRACE_MS)
                } else {
                    null
                }
            val working = actual || (deadline != null && deadline > nowMillis)
            if (previous?.working == true && !working) returns[key] = nowMillis
            states[key] = State(working, if (working) deadline else null, placement)
        }
        states.keys.retainAll(present)
        returns.keys.retainAll(present)
        return Observation(
            working = states.mapValues { it.value.working },
            returns = HashMap(returns),
            nextDeadlineMillis = states.values.mapNotNull { it.deadline }.minOrNull(),
        )
    }
}

val homeInboxReturns = InboxReturnTracker()
