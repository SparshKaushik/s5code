package club.touchtech.s5code.kotlin.transport

import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * One-shot reads survive a connection handoff, like the client-runtime's
 * `pendingOnceSessionReplaced`: a drop on a replaced socket reruns the read on
 * the next one, while a failure on the still-current socket reaches the caller.
 */
class EnvironmentSessionHandoffTest {
    private class FakeSocket(val id: Int, var replaced: Boolean = false)

    @Test
    fun `a read whose socket was replaced reruns on the next connection`() = runTest {
        val sockets = listOf(FakeSocket(1), FakeSocket(2))
        val requested = mutableListOf<FakeSocket?>()
        val result =
            EnvironmentSession.callAcrossHandoffs<FakeSocket, String>(
                awaitConnection = { replacing ->
                    requested += replacing
                    sockets[requested.size - 1]
                },
                handedOff = { it.replaced },
            ) { socket ->
                if (socket.id == 1) {
                    socket.replaced = true
                    throw RpcTransportClosed("The environment closed the connection.")
                }
                "snapshot from ${socket.id}"
            }
        assertEquals("snapshot from 2", result)
        assertNull(requested[0])
        assertSame(sockets[0], requested[1])
    }

    @Test
    fun `a transport failure on the current socket reaches the caller`() = runTest {
        var calls = 0
        val failure =
            runCatching {
                EnvironmentSession.callAcrossHandoffs<FakeSocket, String>(
                    awaitConnection = { FakeSocket(1) },
                    handedOff = { it.replaced },
                ) {
                    calls++
                    throw RpcTransportClosed("The environment did not respond.")
                }
            }.exceptionOrNull()
        assertTrue(failure is RpcTransportClosed)
        assertEquals(1, calls)
    }

    @Test
    fun `a server failure is never retried`() = runTest {
        var calls = 0
        val failure =
            runCatching {
                EnvironmentSession.callAcrossHandoffs<FakeSocket, String>(
                    awaitConnection = { FakeSocket(1, replaced = true) },
                    handedOff = { it.replaced },
                ) {
                    calls++
                    throw RpcFailure(RpcFailureKind.Fail, "NotFound", "Thread removed")
                }
            }.exceptionOrNull()
        assertTrue(failure is RpcFailure)
        assertEquals(1, calls)
    }

    @Test
    fun `a read that keeps losing its socket eventually fails`() = runTest {
        var calls = 0
        val failure =
            runCatching {
                EnvironmentSession.callAcrossHandoffs<FakeSocket, String>(
                    awaitConnection = { FakeSocket(calls) },
                    handedOff = { true },
                ) {
                    calls++
                    throw RpcTransportClosed("The server stopped answering keepalives.")
                }
            }.exceptionOrNull()
        assertTrue(failure is RpcTransportClosed)
        assertEquals(EnvironmentSession.MAX_QUERY_HANDOFFS + 1, calls)
    }
}
