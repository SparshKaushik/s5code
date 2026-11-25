package club.touchtech.s5code.kotlin.design.component

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PlaybackInterruptionsTest {
    @Test
    fun `a request with no interruption since may start`() {
        val interruptions = PlaybackInterruptions()
        interruptions.interrupt()
        val mayStart = interruptions.request()
        assertTrue(mayStart())
    }

    @Test
    fun `an interruption while loading keeps the clip from starting after return`() {
        val interruptions = PlaybackInterruptions()
        val mayStart = interruptions.request()
        interruptions.interrupt()
        assertFalse(mayStart())
        assertTrue(interruptions.request()())
    }
}
