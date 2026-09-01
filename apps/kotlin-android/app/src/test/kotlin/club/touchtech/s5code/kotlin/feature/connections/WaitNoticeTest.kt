package club.touchtech.s5code.kotlin.feature.connections

import club.touchtech.s5code.kotlin.model.ConnectionState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * What a waiting screen says.
 *
 * The rules worth pinning down are the ones about lying: a spinner on a dead
 * connection, and "no threads" on a connection that has not landed yet.
 */
class WaitNoticeTest {

    private fun notice(
        vararg states: ConnectionState,
        label: String? = "devbox",
        hasContent: Boolean = false,
    ) = waitNotice(states.toList(), label, "threads", hasContent)

    @Test
    fun `connected with content on screen has nothing to say`() {
        assertNull(notice(ConnectionState.Connected, hasContent = true))
    }

    @Test
    fun `connected with nothing loaded yet is loading`() {
        val result = notice(ConnectionState.Connected)!!
        assertEquals(WaitPhase.Loading, result.phase)
        assertTrue(result.spinning)
    }

    @Test
    fun `no environments at all is not a wait state`() {
        // Onboarding, not a connection problem. The caller's own empty state owns it.
        assertNull(waitNotice(emptyList(), null, "threads", hasContent = false))
    }

    @Test
    fun `no sessions yet on a paired device is still connecting`() {
        // The frames between a cold start and the first session. Without this the
        // home screen answered "no threads yet", which is the empty state lying
        // about a restore in progress.
        val result =
            waitNotice(
                emptyList(),
                null,
                "threads",
                hasContent = false,
                awaitingEnvironments = true,
            )!!
        assertEquals(WaitPhase.Connecting, result.phase)
        assertTrue(result.spinning)
        assertEquals("Connecting to the environment", result.title)
    }

    @Test
    fun `one connected environment wins over an unreachable one`() {
        // A working screen must not carry a warning about a different environment;
        // that belongs on the connections screen.
        assertNull(
            notice(ConnectionState.Offline, ConnectionState.Connected, hasContent = true)
        )
        assertEquals(
            WaitPhase.Loading,
            notice(ConnectionState.Offline, ConnectionState.Connected)!!.phase,
        )
    }

    @Test
    fun `connecting beats recovering beats auth beats offline`() {
        assertEquals(
            WaitPhase.Connecting,
            notice(ConnectionState.Offline, ConnectionState.Connecting)!!.phase,
        )
        assertEquals(
            WaitPhase.Reconnecting,
            notice(ConnectionState.Offline, ConnectionState.Recovering)!!.phase,
        )
        assertEquals(
            WaitPhase.SignInNeeded,
            notice(ConnectionState.Offline, ConnectionState.AuthRequired)!!.phase,
        )
    }

    @Test
    fun `stalled phases never spin`() {
        assertFalse(notice(ConnectionState.Offline)!!.spinning)
        assertFalse(notice(ConnectionState.AuthRequired)!!.spinning)
    }

    @Test
    fun `retrying phases spin`() {
        assertTrue(notice(ConnectionState.Connecting)!!.spinning)
        assertTrue(notice(ConnectionState.Recovering)!!.spinning)
    }

    @Test
    fun `offline copy accurately describes restored chats`() {
        val result = notice(ConnectionState.Offline)!!
        assertTrue(result.detail.contains("Cached chats"))
        assertFalse(result.detail.contains("already loaded"))
    }

    @Test
    fun `the environment label appears when there is one and reads generically when not`() {
        assertEquals("Connecting to devbox", notice(ConnectionState.Connecting)!!.title)
        assertEquals(
            "Connecting to the environment",
            notice(ConnectionState.Connecting, label = null)!!.title,
        )
    }

    @Test
    fun `the detail names the resource being waited on`() {
        val transcript =
            waitNotice(listOf(ConnectionState.Connecting), "devbox", "transcript", false)!!
        assertTrue(transcript.detail.contains("transcript"))
    }

    @Test
    fun `the pill says syncing rather than loading`() {
        // The pill only exists over content, and content plus "Loading" reads as if
        // what is on screen is not real.
        assertEquals("Syncing…", waitPillLabel(notice(ConnectionState.Connected)!!))
    }

    @Test
    fun `the pill keeps the connection title for connection phases`() {
        assertEquals("You are offline", waitPillLabel(notice(ConnectionState.Offline)!!))
    }
}
