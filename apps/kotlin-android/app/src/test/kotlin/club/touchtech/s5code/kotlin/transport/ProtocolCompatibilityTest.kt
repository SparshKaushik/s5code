package club.touchtech.s5code.kotlin.transport

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Which side of an orchestration protocol mismatch the message blames, and when pairing still goes ahead. */
class ProtocolCompatibilityTest {

    @Test
    fun `the client's own protocol is compatible`() {
        assertNull(orchestrationProtocolCompatibilityError(ORCHESTRATION_PROTOCOL_VERSION, "devbox"))
    }

    @Test
    fun `a server without a version speaks protocol 1 and needs an update`() {
        assertEquals(
            "This client requires a newer server. Update S5 Code on devbox to connect.",
            orchestrationProtocolCompatibilityError(null, "devbox"),
        )
    }

    @Test
    fun `a newer server asks for a newer app`() {
        assertEquals(
            "This client is not supported by this server. Update your app or use a compatible release to connect to devbox.",
            orchestrationProtocolCompatibilityError(ORCHESTRATION_PROTOCOL_VERSION + 1, "devbox"),
        )
    }

    private fun descriptor(version: Int?, selfUpdate: String? = null, desktopAppUpdate: Boolean = false) =
        EnvironmentDescriptorDto(
            environmentId = "env-1",
            label = "devbox",
            orchestrationProtocolVersion = version,
            capabilities =
                EnvironmentCapabilitiesDto(serverSelfUpdate = selfUpdate, desktopAppUpdate = desktopAppUpdate),
        )

    @Test
    fun `pairing a compatible server is never blocked`() {
        assertNull(pairingBlockedByProtocol(descriptor(ORCHESTRATION_PROTOCOL_VERSION)))
    }

    @Test
    fun `an outdated server that can update itself still pairs`() {
        assertNull(pairingBlockedByProtocol(descriptor(1, selfUpdate = "npm")))
        assertNull(pairingBlockedByProtocol(descriptor(1, selfUpdate = "desktop-managed", desktopAppUpdate = true)))
    }

    @Test
    fun `an outdated server with no update path is blocked`() {
        assertEquals(
            "This client requires a newer server. Update S5 Code on devbox to connect.",
            pairingBlockedByProtocol(descriptor(1)),
        )
        assertEquals(
            "This client requires a newer server. Update S5 Code on devbox to connect.",
            pairingBlockedByProtocol(descriptor(1, selfUpdate = "desktop-managed")),
        )
    }

    @Test
    fun `a newer server is blocked even when it can update itself`() {
        assertEquals(
            "This client is not supported by this server. Update your app or use a compatible release to connect to devbox.",
            pairingBlockedByProtocol(descriptor(ORCHESTRATION_PROTOCOL_VERSION + 1, selfUpdate = "npm")),
        )
    }
}
