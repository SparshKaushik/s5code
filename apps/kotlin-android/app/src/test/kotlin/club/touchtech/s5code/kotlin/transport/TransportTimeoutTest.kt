package club.touchtech.s5code.kotlin.transport

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.yield
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TransportTimeoutTest {
    @Test
    fun `a timeout fails as a transport error, not as the caller's cancellation`() = runTest {
        val failure = runCatching { withTransportTimeout(15_000, "Timed out.") { awaitCancellation() } }.exceptionOrNull()
        assertTrue(failure is RpcTransportClosed)
        assertFalse(failure is CancellationException)
        assertEquals("Timed out.", failure?.message)
    }

    @Test
    fun `the caller's own cancellation still cancels`() = runTest {
        var failure: Throwable? = null
        val job = launch {
            try {
                withTransportTimeout(15_000, "Timed out.") { awaitCancellation() }
            } catch (error: Throwable) {
                failure = error
                throw error
            }
        }
        yield()
        job.cancel()
        job.join()
        assertTrue(failure is CancellationException)
    }

    @Test
    fun `a block that finishes in time returns its value`() = runTest {
        assertEquals(7, withTransportTimeout(15_000, "Timed out.") { 7 })
    }
}
