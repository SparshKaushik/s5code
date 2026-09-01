package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.model.TurnInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * The working row is the only thing on screen when a provider is thinking and has
 * emitted nothing, so these cases decide whether the transcript looks alive or dead.
 */
class ActiveWorkTest {
    @Test
    fun `a running turn with no output still reports work in flight`() {
        val turn = TurnInfo("turn-1", "running", startedAtMillis = 5_000, completedAtMillis = null)
        assertEquals(5_000L, activeWorkStartedAtMillis(turn, "running", null))
    }

    @Test
    fun `a completed turn on a still-running session is not settled`() {
        // The provider's completion arrives before the orchestrator finishes the
        // turn's follow-up work. Treating this as settled goes quiet mid-turn.
        val turn = TurnInfo("turn-1", "completed", startedAtMillis = 1_000, completedAtMillis = 4_000)
        assertEquals(1_000L, activeWorkStartedAtMillis(turn, "running", null))
    }

    @Test
    fun `a completed turn on an idle session is settled`() {
        val turn = TurnInfo("turn-1", "completed", startedAtMillis = 1_000, completedAtMillis = 4_000)
        assertNull(activeWorkStartedAtMillis(turn, "idle", null))
    }

    @Test
    fun `a starting session with no turn yet falls back to the session clock`() {
        // The window between pressing send and the provider accepting the turn. RN
        // uses its own send timestamp here; the Kotlin client has no such handle.
        assertEquals(9_000L, activeWorkStartedAtMillis(null, "starting", 9_000))
    }

    @Test
    fun `a ready session with a stale unsettled turn does not run a timer forever`() {
        val turn = TurnInfo("turn-1", "running", startedAtMillis = 1_000, completedAtMillis = null)
        assertNull(activeWorkStartedAtMillis(turn, "ready", null))
    }

    @Test
    fun `an errored session stops the timer`() {
        val turn = TurnInfo("turn-1", "running", startedAtMillis = 1_000, completedAtMillis = null)
        assertNull(activeWorkStartedAtMillis(turn, "error", null))
    }

    @Test
    fun `a thread that has never run reports nothing`() {
        assertNull(activeWorkStartedAtMillis(null, null, null))
        assertNull(activeWorkStartedAtMillis(null, "idle", 1_000))
    }

    @Test
    fun `a turn that never started is not settled`() {
        val turn = TurnInfo("turn-1", "queued", startedAtMillis = null, completedAtMillis = null)
        assertEquals(false, isLatestTurnSettled(turn, null))
        // and with no start of its own it falls through to the session's clock
        assertEquals(7_000L, activeWorkStartedAtMillis(turn, "starting", 7_000))
    }

    @Test
    fun `no session at all trusts the turn record`() {
        val turn = TurnInfo("turn-1", "completed", startedAtMillis = 1_000, completedAtMillis = 2_000)
        assertEquals(true, isLatestTurnSettled(turn, null))
        assertNull(activeWorkStartedAtMillis(turn, null, null))
    }

    @Test
    fun `the label reads the way the other clients read`() {
        assertEquals("Working for 1ms", workingLabel(1_000, 1_000))
        assertEquals("Working for 900ms", workingLabel(1_000, 1_900))
        assertEquals("Working for 2.5s", workingLabel(0, 2_500))
        assertEquals("Working for 45s", workingLabel(0, 45_000))
        assertEquals("Working for 2m 5s", workingLabel(0, 125_000))
    }

    @Test
    fun `a clock that jumps backwards does not produce a negative duration`() {
        assertEquals("Working for 1ms", workingLabel(9_000, 1_000))
    }
}
