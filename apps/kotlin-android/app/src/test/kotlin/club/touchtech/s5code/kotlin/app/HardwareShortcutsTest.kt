package club.touchtech.s5code.kotlin.app

import android.view.KeyEvent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class HardwareShortcutsTest {
    @Test
    fun `command or control N opens a new task`() {
        assertEquals(
            S5HardwareShortcut.NewTask,
            shortcut(KeyEvent.KEYCODE_N, ctrl = true),
        )
        assertEquals(
            S5HardwareShortcut.NewTask,
            shortcut(KeyEvent.KEYCODE_N, meta = true),
        )
    }

    @Test
    fun `command or control K focuses search`() {
        assertEquals(S5HardwareShortcut.FocusSearch, shortcut(KeyEvent.KEYCODE_K, ctrl = true))
        assertEquals(S5HardwareShortcut.FocusSearch, shortcut(KeyEvent.KEYCODE_K, meta = true))
    }

    @Test
    fun `escape is global but modified or repeated noise is ignored`() {
        assertEquals(S5HardwareShortcut.Escape, shortcut(KeyEvent.KEYCODE_ESCAPE))
        assertNull(shortcut(KeyEvent.KEYCODE_N))
        assertNull(shortcut(KeyEvent.KEYCODE_N, ctrl = true, alt = true))
        assertNull(shortcut(KeyEvent.KEYCODE_N, ctrl = true, repeat = 1))
        assertNull(shortcut(KeyEvent.KEYCODE_N, ctrl = true, action = KeyEvent.ACTION_UP))
    }

    @Test
    fun `composer submit requires command or control Enter`() {
        assertTrue(submit(KeyEvent.KEYCODE_ENTER, ctrl = true))
        assertTrue(submit(KeyEvent.KEYCODE_NUMPAD_ENTER, meta = true))
        assertFalse(submit(KeyEvent.KEYCODE_ENTER))
        assertFalse(submit(KeyEvent.KEYCODE_ENTER, ctrl = true, alt = true))
        assertFalse(submit(KeyEvent.KEYCODE_ENTER, ctrl = true, repeat = 1))
        assertFalse(submit(KeyEvent.KEYCODE_ENTER, ctrl = true, action = KeyEvent.ACTION_UP))
    }

    private fun shortcut(
        keyCode: Int,
        action: Int = KeyEvent.ACTION_DOWN,
        repeat: Int = 0,
        ctrl: Boolean = false,
        meta: Boolean = false,
        alt: Boolean = false,
    ) = resolveHardwareShortcut(keyCode, action, repeat, ctrl, meta, alt)

    private fun submit(
        keyCode: Int,
        action: Int = KeyEvent.ACTION_DOWN,
        repeat: Int = 0,
        ctrl: Boolean = false,
        meta: Boolean = false,
        alt: Boolean = false,
    ) = isComposerSubmitShortcut(keyCode, action, repeat, ctrl, meta, alt)
}
