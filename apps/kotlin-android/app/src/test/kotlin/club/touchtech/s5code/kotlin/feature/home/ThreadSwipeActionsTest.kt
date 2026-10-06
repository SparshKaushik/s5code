package club.touchtech.s5code.kotlin.feature.home

import club.touchtech.s5code.kotlin.model.ThreadStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Which swipe actions a home row offers.
 *
 * Every case here is one where offering the wrong action would either lie about
 * what will happen or send a command the server cannot answer, which on this
 * client kills the socket rather than returning an error.
 */
class ThreadSwipeActionsTest {

    private fun thread(status: ThreadStatus) =
        HomeFixture.threads.firstOrNull { it.status == status }
            ?: HomeFixture.threads.first().copy(status = status)

    private fun actions(
        status: ThreadStatus,
        settlement: Boolean = true,
        snooze: Boolean = true,
    ) = threadSwipeActions(thread(status), settlement, snooze)

    @Test
    fun `an active thread offers settle and snooze`() {
        val result = actions(ThreadStatus.Working)
        assertEquals(ThreadSwipeAction.Settle, result.end)
        assertEquals(ThreadSwipeAction.Snooze, result.start)
    }

    @Test
    fun `a settled thread offers reopen instead of settle`() {
        val result = actions(ThreadStatus.Settled)
        assertEquals(ThreadSwipeAction.Unsettle, result.end)
        // Snoozing something already out of the list is a no-op the user cannot
        // see, so it is not offered.
        assertNull(result.start)
    }

    @Test
    fun `a snoozed thread offers only wake`() {
        val result = actions(ThreadStatus.Snoozed)
        assertEquals(ThreadSwipeAction.Unsnooze, result.end)
        assertNull(result.start)
    }

    @Test
    fun `a snoozed thread on a server without snooze offers nothing`() {
        // The row is snoozed, so settle would be the wrong verb, and wake is
        // exactly the command this server does not have.
        val result = actions(ThreadStatus.Snoozed, snooze = false)
        assertNull(result.end)
        assertNull(result.start)
    }

    @Test
    fun `a server without settlement falls back to archive`() {
        assertEquals(ThreadSwipeAction.Archive, actions(ThreadStatus.Working, settlement = false).end)
    }

    @Test
    fun `a server without snooze offers no snooze`() {
        assertNull(actions(ThreadStatus.Working, snooze = false).start)
    }

    @Test
    fun `threads waiting on the user cannot be snoozed`() {
        // Hiding a question until tomorrow is how the question gets lost.
        assertFalse(threadSnoozable(thread(ThreadStatus.AwaitingApproval)))
        assertFalse(threadSnoozable(thread(ThreadStatus.AwaitingInput)))
        assertFalse(threadSnoozable(thread(ThreadStatus.Queued)))
        assertTrue(threadSnoozable(thread(ThreadStatus.Working)))
        assertTrue(threadSnoozable(thread(ThreadStatus.Idle)))
        assertTrue(threadSnoozable(thread(ThreadStatus.Failed)))

        assertNull(actions(ThreadStatus.AwaitingApproval).start)
        assertEquals(ThreadSwipeAction.Settle, actions(ThreadStatus.AwaitingApproval).end)
    }

    @Test
    fun `settle and reopen share one command so the toggle stays single-sided`() {
        assertEquals("settle", ThreadSwipeAction.Settle.command)
        assertEquals("settle", ThreadSwipeAction.Unsettle.command)
        assertEquals("snooze", ThreadSwipeAction.Snooze.command)
        assertEquals("snooze", ThreadSwipeAction.Unsnooze.command)
        assertEquals("archive", ThreadSwipeAction.Archive.command)
    }

    @Test
    fun `every action names the change it makes`() {
        // The label is the only thing telling the user which way a toggle will go.
        assertEquals("Settle", ThreadSwipeAction.Settle.label)
        assertEquals("Reopen", ThreadSwipeAction.Unsettle.label)
        assertEquals("Snooze", ThreadSwipeAction.Snooze.label)
        assertEquals("Wake", ThreadSwipeAction.Unsnooze.label)
        assertEquals("Archive", ThreadSwipeAction.Archive.label)
    }
}
