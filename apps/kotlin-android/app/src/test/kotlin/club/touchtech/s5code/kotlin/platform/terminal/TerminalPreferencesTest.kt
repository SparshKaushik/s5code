package club.touchtech.s5code.kotlin.platform.terminal

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

class TerminalPreferencesTest {
    @Test
    fun `app theme follows current app appearance`() {
        assertEquals(TerminalTheme.light(), resolveTerminalTheme("App", appDark = false))
        assertEquals(TerminalTheme.dark(), resolveTerminalTheme("App", appDark = true))
    }

    @Test
    fun `explicit themes do not follow app appearance`() {
        assertEquals(TerminalTheme.light(), resolveTerminalTheme("Light", appDark = true))
        assertEquals(TerminalTheme.dark(), resolveTerminalTheme("Dark", appDark = false))
        assertNotEquals(
            resolveTerminalTheme("Light", appDark = true),
            resolveTerminalTheme("Dark", appDark = false),
        )
    }

    @Test
    fun `Pierre palette contains all ANSI and bright slots`() {
        assertEquals(16, TerminalTheme.light().palette.size)
        assertEquals(16, TerminalTheme.dark().palette.size)
    }
}
