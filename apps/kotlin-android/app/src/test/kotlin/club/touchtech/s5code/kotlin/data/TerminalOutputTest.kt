package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.TerminalStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class TerminalOutputTest {

    @Test
    fun `wire statuses map onto the model`() {
        assertEquals(TerminalStatus.Starting, terminalStatusOf("starting"))
        assertEquals(TerminalStatus.Running, terminalStatusOf("running"))
        assertEquals(TerminalStatus.Exited, terminalStatusOf("exited"))
        assertEquals(TerminalStatus.Closed, terminalStatusOf("closed"))
        assertEquals(TerminalStatus.Error, terminalStatusOf("error"))
    }

    @Test
    fun `an unrecognised status is an error, not a guess at running`() {
        assertEquals(TerminalStatus.Error, terminalStatusOf("hibernating"))
    }

    @Test
    fun `only starting and running count as live`() {
        assertTrue(TerminalStatus.Starting.live)
        assertTrue(TerminalStatus.Running.live)
        assertTrue(!TerminalStatus.Exited.live)
        assertTrue(!TerminalStatus.Closed.live)
        assertTrue(!TerminalStatus.Error.live)
    }

    @Test
    fun `grid size divides the surface by the cell`() {
        assertEquals(40 to 20, terminalGridSize(400f, 400f, 10f, 20f))
    }

    @Test
    fun `grid size falls back before the surface is measured`() {
        assertEquals(
            DEFAULT_TERMINAL_COLS to DEFAULT_TERMINAL_ROWS,
            terminalGridSize(400f, 400f, 0f, 0f),
        )
    }

    @Test
    fun `grid size stays inside the contract's bounds`() {
        // A zero-height surface mid-layout would otherwise send rows=0, which the
        // contract rejects and the server answers with a validation failure.
        assertEquals(1 to 1, terminalGridSize(2f, 2f, 10f, 20f))
        assertEquals(1_000 to 500, terminalGridSize(100_000f, 100_000f, 1f, 1f))
    }

    @Test
    fun `output past the cursor appends even after the front was trimmed`() {
        val first = readTerminalOutputUpdate("abc", resetVersion = 1, end = 3, cursor = null)
        assertEquals(TerminalOutputUpdate.Reset("abc"), first.first)
        // The scrollback dropped "ab" and gained "de": only "de" is new.
        val trimmed = readTerminalOutputUpdate("cde", resetVersion = 1, end = 5, cursor = first.second)
        assertEquals(TerminalOutputUpdate.Append("de"), trimmed.first)
        assertEquals(TerminalOutputCursor(1, 5), trimmed.second)
        assertEquals(TerminalOutputUpdate.None, readTerminalOutputUpdate("cde", 1, 5, trimmed.second).first)
    }

    @Test
    fun `a reset or a cursor outside the retained tail replays history`() {
        val cursor = TerminalOutputCursor(1, 5)
        assertEquals(TerminalOutputUpdate.Reset("xy"), readTerminalOutputUpdate("xy", 2, 2, cursor).first)
        // Fell behind by more than the retained tail: the gap cannot be appended.
        assertEquals(TerminalOutputUpdate.Reset("hij"), readTerminalOutputUpdate("hij", 1, 10, cursor).first)
    }
}
