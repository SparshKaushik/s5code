package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.TerminalStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The VT emulator behind the terminal screen.
 *
 * These are the sequences a real shell session emits within the first second:
 * bracketed-paste mode, an SGR-coloured prompt, a title OSC, and the erase
 * sequences readline uses while you type. Getting any of them wrong shows up as
 * literal escape text in the transcript, which is exactly the bug this replaces.
 */
class TerminalOutputTest {

    @Test
    fun `bracketed paste mode does not print`() {
        assertEquals(listOf("$ "), terminalLines("\u001b[?2004h$ "))
    }

    @Test
    fun `sgr colour codes are dropped but text is kept`() {
        assertEquals(
            listOf("user@host:~$ "),
            terminalLines("\u001b[01;32muser@host\u001b[00m:\u001b[01;34m~\u001b[00m$ "),
        )
    }

    @Test
    fun `an osc title sequence is consumed whole`() {
        assertEquals(listOf("ready"), terminalLines("\u001b]0;bash\u0007ready"))
    }

    @Test
    fun `an osc terminated by string terminator is consumed whole`() {
        assertEquals(listOf("ready"), terminalLines("\u001b]0;bash\u001b\\ready"))
    }

    @Test
    fun `carriage return overwrites without truncating, as a real terminal does`() {
        // "doneing" is correct, not a bug: CR only moves the cursor. This is why
        // shells follow a redraw with an erase, covered by the next test.
        assertEquals(listOf("doneing"), terminalLines("working\rdone"))
    }

    @Test
    fun `erase to end of line clears what carriage return left behind`() {
        assertEquals(listOf("done"), terminalLines("working\rdone\u001b[K"))
    }

    @Test
    fun `backspace and delete-character shorten the line like readline does`() {
        assertEquals(listOf("git st"), terminalLines("git sta\b\u001b[P"))
    }

    @Test
    fun `tabs advance to the next eight column stop`() {
        assertEquals(listOf("a       b"), terminalLines("a\tb"))
    }

    @Test
    fun `newlines start lines and the buffer keeps them`() {
        assertEquals(listOf("one", "two", ""), terminalLines("one\ntwo\n"))
    }

    @Test
    fun `clear screen drops the scrollback`() {
        assertEquals(listOf("fresh"), terminalLines("old output\n\u001b[2Jfresh"))
    }

    @Test
    fun `absolute cursor addressing is relative to the viewport, not the scrollback`() {
        // `ESC[H` means row 1 of the screen. With a 3-row viewport and 5 lines of
        // history, that is the third-from-last line, not the top of the session.
        val lines = terminalLines("1\n2\n3\n4\n5\u001b[Hx", rows = 3)
        assertEquals(listOf("1", "2", "x", "4", "5"), lines)
    }

    @Test
    fun `a sequence split across chunks is not printed as text`() {
        val emulator = TerminalEmulator()
        emulator.feed("green \u001b[01")
        emulator.feed(";32mtext")
        assertEquals(listOf("green text"), emulator.snapshot())
    }

    @Test
    fun `an osc split across chunks is not printed as text`() {
        val emulator = TerminalEmulator()
        emulator.feed("\u001b]0;my ti")
        emulator.feed("tle\u0007ok")
        assertEquals(listOf("ok"), emulator.snapshot())
    }

    @Test
    fun `cursor forward pads with spaces rather than overwriting`() {
        assertEquals(listOf("ab   c"), terminalLines("ab\u001b[3Cc"))
    }

    @Test
    fun `a snapshot reset drops the previous transcript`() {
        val emulator = TerminalEmulator()
        emulator.feed("first session\n")
        emulator.reset()
        emulator.feed("second")
        assertEquals(listOf("second"), emulator.snapshot())
    }

    @Test
    fun `scrollback is bounded and the cursor follows the drop`() {
        val emulator = TerminalEmulator(rows = 4, maxScrollback = 10)
        repeat(30) { emulator.feed("line $it\n") }
        val snapshot = emulator.snapshot()
        assertEquals(10, snapshot.size)
        assertTrue(snapshot.first().startsWith("line 2"))
        // Writing after the trim still lands on the last line rather than
        // silently going out of bounds.
        emulator.feed("tail")
        assertEquals("tail", emulator.snapshot().last())
    }

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
            TerminalEmulator.DEFAULT_COLS to TerminalEmulator.DEFAULT_ROWS,
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
}
