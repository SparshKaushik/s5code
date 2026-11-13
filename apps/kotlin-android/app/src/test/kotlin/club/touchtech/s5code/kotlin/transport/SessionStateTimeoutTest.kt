package club.touchtech.s5code.kotlin.transport

import java.io.IOException
import java.io.InterruptedIOException
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

/** A slow host must not read as read-only: the session-state read waits longer instead of failing. */
class SessionStateTimeoutTest {

    private val timedOut = EnvironmentHttpError.unreachable(InterruptedIOException("timeout"))

    @Test
    fun `a timeout retries with a doubled allowance until the host answers`() = runTest {
        val allowances = mutableListOf<Long>()
        val result =
            retryingTimeouts(6_000, 60_000) { timeoutMs ->
                allowances += timeoutMs
                if (allowances.size < 3) throw timedOut
                "answered"
            }
        assertEquals("answered", result)
        assertEquals(listOf(6_000L, 12_000L, 24_000L), allowances)
    }

    @Test
    fun `it gives up once a wait at the cap also times out`() {
        val allowances = mutableListOf<Long>()
        val error =
            assertThrows(EnvironmentHttpError::class.java) {
                kotlinx.coroutines.runBlocking {
                    retryingTimeouts(15_000, 60_000) { timeoutMs ->
                        allowances += timeoutMs
                        throw timedOut
                    }
                }
            }
        assertEquals(timedOut, error)
        assertEquals(listOf(15_000L, 30_000L, 60_000L), allowances)
    }

    @Test
    fun `other failures are not retried`() {
        var calls = 0
        assertThrows(EnvironmentHttpError::class.java) {
            kotlinx.coroutines.runBlocking {
                retryingTimeouts(6_000, 60_000) {
                    calls++
                    throw EnvironmentHttpError.unreachable(IOException("connection refused"))
                }
            }
        }
        assertEquals(1, calls)
    }
}
