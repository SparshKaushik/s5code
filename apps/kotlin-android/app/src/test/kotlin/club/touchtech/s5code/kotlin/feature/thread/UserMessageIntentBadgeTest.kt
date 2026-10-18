package club.touchtech.s5code.kotlin.feature.thread

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class UserMessageIntentBadgeTest {

    @Test
    fun `a plain turn start has no badge`() {
        assertNull(userMessageIntentBadge(null))
        assertNull(userMessageIntentBadge("turn_start"))
        assertNull(userMessageIntentBadge("something_new"))
    }

    @Test
    fun `a queued message keeps its from-queue badge`() {
        val badge = userMessageIntentBadge("queued_turn")!!
        assertEquals("from queue", badge.label)
        assertEquals(UserMessageIntentTone.Queued, badge.tone)
    }

    @Test
    fun `steers and promoted queue entries read as steers`() {
        assertEquals("steer", userMessageIntentBadge("steer")!!.label)
        assertEquals(UserMessageIntentTone.Steer, userMessageIntentBadge("steer")!!.tone)
        val promoted = userMessageIntentBadge("promoted_queued_to_steer")!!
        assertEquals("queued → steer", promoted.label)
        assertEquals(UserMessageIntentTone.Steer, promoted.tone)
    }
}
