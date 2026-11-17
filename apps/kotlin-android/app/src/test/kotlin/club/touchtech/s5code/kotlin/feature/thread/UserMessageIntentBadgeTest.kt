package club.touchtech.s5code.kotlin.feature.thread

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class UserMessageIntentBadgeTest {

    @Test
    fun `plain and queued turns have no badge`() {
        assertNull(userMessageIntentBadge(null))
        assertNull(userMessageIntentBadge("turn_start"))
        assertNull(userMessageIntentBadge("queued_turn"))
        assertNull(userMessageIntentBadge("something_new"))
    }

    @Test
    fun `steers and promoted queue entries read as steers`() {
        val steer = userMessageIntentBadge("steer")!!
        assertEquals("Steer", steer.label)
        assertEquals("Steered the active turn", steer.accessibilityLabel)
        val promoted = userMessageIntentBadge("promoted_queued_to_steer")!!
        assertEquals("Steer", promoted.label)
        assertEquals("Originally queued, then promoted to steer the active turn", promoted.accessibilityLabel)
    }
}
