package club.touchtech.s5code.kotlin.app

import androidx.lifecycle.Lifecycle
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ForegroundRefreshGateTest {
    @Test
    fun `cold start does not restart sessions but return from background does`() {
        val gate = ForegroundRefreshGate()

        assertNull(gate.onEvent(Lifecycle.Event.ON_START))
        assertNull(gate.onEvent(Lifecycle.Event.ON_STOP))
        assertNotNull(gate.onEvent(Lifecycle.Event.ON_START))
    }

    @Test
    fun `resume noise refreshes at most once per stop`() {
        val gate = ForegroundRefreshGate()

        gate.onEvent(Lifecycle.Event.ON_STOP)
        assertNotNull(gate.onEvent(Lifecycle.Event.ON_START))
        assertNull(gate.onEvent(Lifecycle.Event.ON_START))
        assertNull(gate.onEvent(Lifecycle.Event.ON_RESUME))
    }

    @Test
    fun `the reported duration is the real time spent stopped`() {
        val gate = ForegroundRefreshGate()

        gate.onEvent(Lifecycle.Event.ON_STOP)
        Thread.sleep(15)
        val duration = gate.onEvent(Lifecycle.Event.ON_START)

        assertNotNull(duration)
        assertTrue(duration!! >= 15)
    }
}
