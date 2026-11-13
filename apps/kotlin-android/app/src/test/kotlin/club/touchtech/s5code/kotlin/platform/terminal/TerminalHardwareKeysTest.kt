package club.touchtech.s5code.kotlin.platform.terminal

import android.view.KeyEvent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class TerminalHardwareKeysTest {
    @Test
    fun `arrow keys retain ctrl alt and shift modifiers for Ghostty`() {
        val request =
            TerminalHardwareKeys.request(
                keyCode = KeyEvent.KEYCODE_DPAD_UP,
                action = KeyEvent.ACTION_DOWN,
                metaState = KeyEvent.META_CTRL_ON or KeyEvent.META_ALT_ON or KeyEvent.META_SHIFT_ON,
                repeatCount = 0,
                text = null,
                unshiftedCodepoint = 0,
            )
        assertNotNull(request)
        assertEquals(TerminalHardwareKeys.ACTION_PRESS, request!!.action)
        assertEquals(0b111, request.modifiers)
    }

    @Test
    fun `repeat and release actions survive normalization`() {
        val repeated =
            TerminalHardwareKeys.request(
                KeyEvent.KEYCODE_F5,
                KeyEvent.ACTION_DOWN,
                0,
                2,
                null,
                0,
            )
        val released =
            TerminalHardwareKeys.request(
                KeyEvent.KEYCODE_F5,
                KeyEvent.ACTION_UP,
                0,
                0,
                null,
                0,
            )
        assertEquals(TerminalHardwareKeys.ACTION_REPEAT, repeated!!.action)
        assertEquals(TerminalHardwareKeys.ACTION_RELEASE, released!!.action)
    }

    @Test
    fun `control characters are never sent as layout text`() {
        val request =
            TerminalHardwareKeys.request(
                KeyEvent.KEYCODE_C,
                KeyEvent.ACTION_DOWN,
                KeyEvent.META_CTRL_ON,
                0,
                "\u0003",
                'c'.code,
            )
        assertNull(request!!.text)
        assertEquals('c'.code, request.unshiftedCodepoint)
    }

    @Test
    fun `Android media keys remain available to the system`() {
        assertNull(
            TerminalHardwareKeys.request(
                KeyEvent.KEYCODE_VOLUME_UP,
                KeyEvent.ACTION_DOWN,
                0,
                0,
                null,
                0,
            )
        )
    }
}
