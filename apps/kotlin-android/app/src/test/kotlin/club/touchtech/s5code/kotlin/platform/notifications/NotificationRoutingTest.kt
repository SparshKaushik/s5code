package club.touchtech.s5code.kotlin.platform.notifications

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class NotificationRoutingTest {
    @Test
    fun `relay alert paths retain allowed thread destinations`() {
        assertEquals(
            "/threads/env-1/thread-2/review",
            notificationPath(mapOf("alert_path" to "/threads/env-1/thread-2/review")),
        )
        assertEquals(
            "/threads/env-1/thread-2",
            notificationPath(mapOf("activity_path" to "/threads/env-1/thread-2")),
        )
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
        assertNull(notificationPath(mapOf("alert_path" to "/settings")))
        assertNull(notificationPath(mapOf("activity_path" to "/threads/e/t/git-confirm")))
        assertNull(notificationPath(mapOf("deepLink" to "//evil.example")))
        assertNull(notificationPath(mapOf("alert_path" to "/threads/e/t?x=1")))
    }
}
