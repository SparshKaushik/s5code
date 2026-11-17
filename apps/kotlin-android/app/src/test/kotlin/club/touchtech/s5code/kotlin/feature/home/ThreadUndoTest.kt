package club.touchtech.s5code.kotlin.feature.home

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class ThreadUndoTest {

    private val thread = HomeFixture.threads.first()

    @Test
    fun `undoing a snooze only wakes the thread`() {
        assertEquals(listOf(ThreadUndoStep.Unsnooze), planThreadUndo(ThreadUndoAction.Snoozed, thread))
    }

    @Test
    fun `undoing a settle restores the pin and then the snooze the settle cleared`() {
        val before = thread.copy(pinned = true, pinOrderKey = "a0", snoozedUntil = "2026-04-11T09:00:00Z")
        assertEquals(
            listOf(
                ThreadUndoStep.Unsettle,
                ThreadUndoStep.Pin("a0"),
                ThreadUndoStep.Snooze("2026-04-11T09:00:00Z"),
            ),
            planThreadUndo(ThreadUndoAction.Settled, before),
        )
        assertEquals(
            listOf(ThreadUndoStep.Unsettle),
            planThreadUndo(ThreadUndoAction.Settled, thread.copy(pinned = false, snoozedUntil = null)),
        )
    }

    @Test
    fun `undoing an unpin re-pins at the old key`() {
        assertEquals(
            listOf(ThreadUndoStep.Pin("a1")),
            planThreadUndo(ThreadUndoAction.Unpinned, thread.copy(pinned = true, pinOrderKey = "a1", snoozedUntil = null)),
        )
    }

    @Test
    fun `an expired snooze is skipped rather than rejected`() = runBlocking {
        val ran = mutableListOf<ThreadUndoStep>()
        val plan =
            listOf(ThreadUndoStep.Pin(null), ThreadUndoStep.Snooze("2026-04-10T11:00:00Z"))
        executeThreadUndo(plan, { java.time.Instant.parse("2026-04-10T12:00:00Z").toEpochMilli() }) { ran += it }
        assertEquals(listOf<ThreadUndoStep>(ThreadUndoStep.Pin(null)), ran)
    }

    @Test
    fun `a failed step stops the rest of the plan`() {
        val ran = mutableListOf<ThreadUndoStep>()
        assertThrows(IllegalStateException::class.java) {
            runBlocking {
                executeThreadUndo(listOf(ThreadUndoStep.Unsettle, ThreadUndoStep.Pin(null)), { 0L }) { step ->
                    ran += step
                    error("refused")
                }
            }
        }
        assertEquals(listOf<ThreadUndoStep>(ThreadUndoStep.Unsettle), ran)
    }
}
