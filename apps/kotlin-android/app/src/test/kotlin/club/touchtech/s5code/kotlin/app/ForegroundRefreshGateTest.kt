package club.touchtech.s5code.kotlin.app

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ForegroundRefreshGateTest {
    @Test
    fun `cold start does not restart sessions but return from background does`() {
        var stopped = false

        var transition = shouldRefreshAfterLifecycleEvent(stopped, "start")
        stopped = transition.first
        assertFalse(transition.second)
        transition = shouldRefreshAfterLifecycleEvent(stopped, "stop")
        stopped = transition.first
        assertFalse(transition.second)
        transition = shouldRefreshAfterLifecycleEvent(stopped, "start")
        assertTrue(transition.second)
    }

    @Test
    fun `resume noise refreshes at most once per stop`() {
        val afterStop = shouldRefreshAfterLifecycleEvent(false, "stop")
        val firstStart = shouldRefreshAfterLifecycleEvent(afterStop.first, "start")
        val secondStart = shouldRefreshAfterLifecycleEvent(firstStart.first, "start")
        val resume = shouldRefreshAfterLifecycleEvent(secondStart.first, "other")

        assertTrue(firstStart.second)
        assertFalse(secondStart.second)
        assertFalse(resume.second)
    }
}
