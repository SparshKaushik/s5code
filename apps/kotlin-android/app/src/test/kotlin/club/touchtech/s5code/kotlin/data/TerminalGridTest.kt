package club.touchtech.s5code.kotlin.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test

class TerminalGridTest {
    @Test
    fun `the shared grid follows frames that carry both edges`() {
        val attached = nextTerminalGrid(null, 130, 15)
        assertEquals(130 to 15, attached)
        assertEquals(54 to 15, nextTerminalGrid(attached, 54, 15))
        // An older server's activity has no geometry and keeps what was reported.
        assertSame(attached, nextTerminalGrid(attached, null, null))
        assertSame(attached, nextTerminalGrid(attached, 80, null))
        assertNull(nextTerminalGrid(null, null, null))
    }

    @Test
    fun `the view draws the shared grid clamped to its own fit`() {
        assertEquals(60 to 20, drawnTerminalGrid(60 to 20, null))
        assertEquals(54 to 15, drawnTerminalGrid(60 to 20, 54 to 15))
        assertEquals(60 to 20, drawnTerminalGrid(60 to 20, 130 to 40))
        assertEquals(60 to 15, drawnTerminalGrid(60 to 20, 130 to 15))
    }
}
