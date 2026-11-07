package club.touchtech.s5code.kotlin.transport

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** How a session recovers: resume probing by route and time away, and reads made while unreachable. */
class ConnectionRecoveryTest {

    @Test
    fun `loopback and LAN hosts answer quickly`() {
        listOf(
            "http://localhost:3773",
            "http://127.0.0.1:3773",
            "http://[::1]:3773",
            "http://192.168.1.20:3773",
            "http://10.0.0.5",
            "http://172.20.1.1",
            "http://169.254.3.4",
            "http://devbox.local:3773",
            "http://devbox:3773",
            "http://[fe80::1]:3773",
            "http://[fd12:3456::1]:3773",
        ).forEach { assertTrue(it, answersQuickly(it)) }
    }

    @Test
    fun `tailnet, VPN, relay and public hosts do not`() {
        listOf(
            "https://devbox.tail1234.ts.net",
            "http://[fd7a:115c:a1e0::1]:3773",
            "http://100.101.102.103:3773",
            "https://relay.example.com/env/abc",
            "http://8.8.8.8",
            "http://172.32.0.1",
            "not a url",
        ).forEach { assertFalse(it, answersQuickly(it)) }
    }

    @Test
    fun `a long resume always probes quickly and a short one gives remote routes longer`() {
        assertEquals(3_000L, EnvironmentSession.probeTimeoutMs(60_000, quickRoute = false))
        assertEquals(3_000L, EnvironmentSession.probeTimeoutMs(60_000, quickRoute = true))
        assertEquals(3_000L, EnvironmentSession.probeTimeoutMs(2_000, quickRoute = true))
        assertEquals(15_000L, EnvironmentSession.probeTimeoutMs(2_000, quickRoute = false))
    }

    @Test
    fun `a read fails at once only while a retry recovers from a failure or the device is offline`() {
        val failure = "Could not reach that address."
        assertEquals(failure, SessionState(phase = SessionPhase.Backoff, lastError = failure).unreachableQueryError())
        assertEquals(failure, SessionState(phase = SessionPhase.Connecting, lastError = failure).unreachableQueryError())
        assertEquals("The device is offline.", SessionState(phase = SessionPhase.Offline).unreachableQueryError())
        // A dropped live session reconnects without a failure; reads wait for it.
        assertNull(SessionState(phase = SessionPhase.Backoff).unreachableQueryError())
        assertNull(SessionState(phase = SessionPhase.Connecting).unreachableQueryError())
        assertNull(SessionState(phase = SessionPhase.Idle, lastError = failure).unreachableQueryError())
    }
}
