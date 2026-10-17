package club.touchtech.s5code.kotlin.cloud

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Parity with `resolveCloudPublicConfig` and `normalizeSecureRelayUrl` on mobile.
 *
 * The rule worth pinning is the cleartext rejection: a relay reached over http
 * would carry account tokens in the clear, so an http origin has to read as
 * "not configured" rather than being used.
 */
class CloudPublicConfigTest {

    private fun config(
        key: String? = "pk_test_key",
        template: String? = "s5-relay",
        relay: String? = "https://relay.example.dev/",
    ) = CloudPublicConfig(publishableKey = key, jwtTemplate = template, relayUrl = relay)

    @Test
    fun `all three values are required`() {
        assertTrue(config().configured)
        assertFalse(config(key = null).configured)
        assertFalse(config(template = null).configured)
        assertFalse(config(relay = null).configured)
    }

    @Test
    fun `https relay origins normalize to a trailing slash`() {
        assertEquals(
            "https://relay.example.dev/",
            CloudPublicConfig.normalizeSecureRelayUrl("https://relay.example.dev"),
        )
        assertEquals(
            "https://relay.example.dev/",
            CloudPublicConfig.normalizeSecureRelayUrl("https://relay.example.dev/some/path"),
        )
        assertEquals(
            "https://relay.example.dev:8443/",
            CloudPublicConfig.normalizeSecureRelayUrl("https://relay.example.dev:8443"),
        )
    }

    @Test
    fun `cleartext and malformed relay urls are rejected`() {
        assertNull(CloudPublicConfig.normalizeSecureRelayUrl("http://relay.example.dev"))
        assertNull(CloudPublicConfig.normalizeSecureRelayUrl("relay.example.dev"))
        assertNull(CloudPublicConfig.normalizeSecureRelayUrl("wss://relay.example.dev"))
        assertNull(CloudPublicConfig.normalizeSecureRelayUrl(""))
    }
}
