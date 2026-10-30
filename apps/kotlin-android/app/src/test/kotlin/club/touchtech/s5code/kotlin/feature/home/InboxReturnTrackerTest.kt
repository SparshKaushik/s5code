package club.touchtech.s5code.kotlin.feature.home

import club.touchtech.s5code.kotlin.model.ThreadStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** `createInboxReturnTracker` in client-runtime state/threadInbox.ts. */
class InboxReturnTrackerTest {
    private val working = HomeFixture.threads.first { it.status == ThreadStatus.Working }
    private val idle = working.copy(status = ThreadStatus.Idle)

    @Test
    fun `a thread that stops working stays on the shelf through the grace`() {
        val tracker = InboxReturnTracker()
        assertTrue(tracker.observe(listOf(working), 0).isWorking(working))
        val held = tracker.observe(listOf(idle), 100)
        assertTrue(held.isWorking(idle))
        assertEquals(100 + WORKING_EXIT_GRACE_MS, held.nextDeadlineMillis)
        assertNull(held.returnedAt(idle))

        // Re-observed at the deadline, it returns to the inbox, stamped then.
        val returned = tracker.observe(listOf(idle), 100 + WORKING_EXIT_GRACE_MS)
        assertFalse(returned.isWorking(idle))
        assertEquals(100 + WORKING_EXIT_GRACE_MS, returned.returnedAt(idle))
        assertNull(returned.nextDeadlineMillis)
    }

    @Test
    fun `a run that continues inside the grace never leaves the shelf`() {
        val tracker = InboxReturnTracker()
        tracker.observe(listOf(working), 0)
        tracker.observe(listOf(idle), 100)
        val resumed = tracker.observe(listOf(working), 500)
        assertTrue(resumed.isWorking(working))
        assertNull(resumed.returnedAt(working))
        assertNull(resumed.nextDeadlineMillis)
    }

    @Test
    fun `needing the user or a move ends the hold at once`() {
        for (next in listOf(
            working.copy(status = ThreadStatus.AwaitingApproval),
            idle.copy(lastError = "boom"),
            idle.copy(pinned = true),
            working.copy(status = ThreadStatus.Settled),
            idle.copy(activeOrderKey = "moved"),
        )) {
            val tracker = InboxReturnTracker()
            tracker.observe(listOf(working), 0)
            val observed = tracker.observe(listOf(next), 100)
            assertFalse(next.toString(), observed.isWorking(next))
            assertNull(observed.nextDeadlineMillis)
        }
    }

    @Test
    fun `turning the shelf off forgets held threads`() {
        val tracker = InboxReturnTracker()
        tracker.observe(listOf(working), 0)
        tracker.observe(null, 50)
        val observed = tracker.observe(listOf(idle), 100)
        assertFalse(observed.isWorking(idle))
        assertNull(observed.nextDeadlineMillis)
    }
}
