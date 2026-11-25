package club.touchtech.s5code.kotlin.feature.thread

import org.junit.Assert.assertEquals
import org.junit.Test

/** An open queued-message edit whose run starts must never overwrite the user's other draft. */
class QueuedEditExitTest {
    @Test
    fun `an untouched edit just closes`() {
        assertEquals(QueuedEditExit.Clean, queuedEditExit("fix it", "fix it", threadDraftEmpty = false))
        assertEquals(QueuedEditExit.Clean, queuedEditExit("fix it", "fix it", threadDraftEmpty = true))
    }

    @Test
    fun `a changed edit moves into an empty composer`() {
        assertEquals(QueuedEditExit.Kept, queuedEditExit("fix it", "fix it properly", threadDraftEmpty = true))
    }

    @Test
    fun `a changed edit is dropped when the composer holds other work`() {
        assertEquals(QueuedEditExit.Discarded, queuedEditExit("fix it", "fix it properly", threadDraftEmpty = false))
    }

    @Test
    fun `a cleared edit has nothing to put back`() {
        assertEquals(QueuedEditExit.Discarded, queuedEditExit("fix it", "  ", threadDraftEmpty = true))
    }
}
