package club.touchtech.s5code.kotlin.transport

import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The retry ladder is pinned to RN's `RETRY_DELAYS_MS` (3s/4s/8s/16s) plus up
 * to 50% jitter: each rung's value lands in `[base, base * 1.5)` and the last
 * rung holds at 16s rather than growing forever.
 */
class EnvironmentSessionBackoffTest {
    @Test
    fun `rungs match the RN retry delays`() {
        val bases = longArrayOf(3_000L, 4_000L, 8_000L, 16_000L)
        bases.forEachIndexed { rung, base ->
            repeat(64) {
                val delay = EnvironmentSession.backoffMillis(rung)
                assertTrue("rung $rung below base: $delay", delay >= base)
                assertTrue("rung $rung above jitter bound: $delay", delay <= base + base / 2)
            }
        }
    }

    @Test
    fun `the ladder caps at the last rung`() {
        repeat(64) {
            val delay = EnvironmentSession.backoffMillis(12)
            assertTrue(delay >= 16_000L)
            assertTrue(delay <= 16_000L + 8_000L)
        }
    }
}
