package club.touchtech.s5code.kotlin.platform.notifications

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationRoutingTest {
    @Test
    fun `relay deep links retain allowed thread destinations`() {
        assertEquals(
            "/threads/env-1/thread-2/review",
            notificationPath(mapOf("deepLink" to "/threads/env-1/thread-2/review")),
        )
    }

    @Test
    fun `ids provide a safe fallback and are encoded once`() {
        assertEquals(
            "/threads/env%201/thread%2F2",
            notificationPath(mapOf("environmentId" to "env 1", "threadId" to "thread/2")),
        )
    }

    @Test
    fun `notifications cannot open arbitrary or stateful routes`() {
        assertNull(notificationPath(mapOf("deepLink" to "/settings")))
        assertNull(notificationPath(mapOf("deepLink" to "/threads/e/t/git-confirm")))
        assertNull(notificationPath(mapOf("deepLink" to "//evil.example")))
        assertNull(notificationPath(mapOf("deepLink" to "/threads/e/t?x=1")))
    }

    @Test
    fun `generation and timestamp ordering reject stale updates`() {
        assertTrue(
            shouldAcceptLiveUpdateEvent(
                currentGeneration = "g2",
                receivedGeneration = "g2",
                eventAt = "2026-08-29T12:00:01Z",
                previousEventAt = "2026-08-29T12:00:00Z",
            )
        )
        assertFalse(
            shouldAcceptLiveUpdateEvent(
                currentGeneration = "g2",
                receivedGeneration = "g1",
                eventAt = "2026-08-29T12:00:02Z",
                previousEventAt = null,
            )
        )
        assertFalse(
            shouldAcceptLiveUpdateEvent(
                currentGeneration = "g2",
                receivedGeneration = "g2",
                eventAt = "2026-08-29T12:00:00Z",
                previousEventAt = "2026-08-29T12:00:00Z",
            )
        )
        assertFalse(
            shouldAcceptLiveUpdateEvent(
                currentGeneration = "g2",
                receivedGeneration = "g2",
                eventAt = "not-an-instant",
                previousEventAt = null,
            )
        )
    }
}
