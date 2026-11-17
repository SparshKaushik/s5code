package club.touchtech.s5code.kotlin.data

import org.junit.Assert.assertEquals
import org.junit.Test

/** `formatResetLabel` in `packages/shared/src/usageLimits.ts`: the last minute never reads "resets in 0m". */
class UsageLimitsResetLabelTest {

    private val now = 1_760_000_000_000L

    @Test
    fun `a minute or more away counts down`() {
        assertEquals("resets in 1m", formatResetLabel(now + 60_000, now))
        assertEquals("resets in 2h 13m", formatResetLabel(now + (2 * 60 + 13) * 60_000L, now))
    }

    @Test
    fun `inside the final minute it resets momentarily`() {
        assertEquals("Resets momentarily", formatResetLabel(now + 59_999, now))
        assertEquals("Resets momentarily", formatResetLabel(now + 1, now))
    }

    @Test
    fun `a reset that has arrived or passed resets now`() {
        assertEquals("resets now", formatResetLabel(now, now))
        assertEquals("resets now", formatResetLabel(now - 5_000, now))
    }
}
