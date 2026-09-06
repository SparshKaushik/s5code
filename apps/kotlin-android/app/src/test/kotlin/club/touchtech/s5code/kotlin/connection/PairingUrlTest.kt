package club.touchtech.s5code.kotlin.connection

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Parity tests against `resolveRemotePairingTarget` in
 * `packages/shared/src/remote.ts`. The token is one-time, so a URL parsed loosely
 * burns the credential and then fails the exchange; every case here is one the
 * shared helper already decides.
 */
class PairingUrlTest {

    private fun valid(input: String): PairingTarget {
        val result = parsePairingUrl(input)
        assertTrue("expected valid for $input, got $result", result is PairingUrlResult.Valid)
        return (result as PairingUrlResult.Valid).target
    }

    private fun error(input: String): PairingUrlError {
        val result = parsePairingUrl(input)
        assertTrue("expected invalid for $input, got $result", result is PairingUrlResult.Invalid)
        return (result as PairingUrlResult.Invalid).reason
    }

    @Test
    fun `accepts the startup pairing url`() {
        val target = valid("http://macbook.local:4488/pair#token=abcdefgh1234")
        assertEquals("http://macbook.local:4488/", target.httpBaseUrl)
        assertEquals("ws://macbook.local:4488/", target.wsBaseUrl)
        assertEquals("macbook.local:4488", target.host)
        assertEquals("abcdefgh1234", target.credential)
        assertEquals(false, target.secure)
    }

    @Test
    fun `accepts https and marks it secure`() {
        val target = valid("https://s5.example.dev/pair#token=abcdefgh1234")
        assertEquals("https://s5.example.dev/", target.httpBaseUrl)
        assertEquals("wss://s5.example.dev/", target.wsBaseUrl)
        assertTrue(target.secure)
    }

    /** ws/wss are accepted and normalized, matching the shared protocol set. */
    @Test
    fun `accepts websocket schemes and normalizes them`() {
        val target = valid("ws://h:4488/?token=abcdefgh1234")
        assertEquals("http://h:4488/", target.httpBaseUrl)
        assertEquals("ws://h:4488/", target.wsBaseUrl)
    }

    /** A hash token outranks a query one, the same order the helper checks. */
    @Test
    fun `a fragment token wins over a query token`() {
        assertEquals("fromhash", valid("http://h:4488/?token=fromquery#token=fromhash").credential)
        assertEquals("fromquery", valid("http://h:4488/?token=fromquery").credential)
    }

    /**
     * Hosted pairing: the credential belongs to the backend named by `host`, not
     * to the web app that served the link. Getting this backwards would send the
     * token to the wrong origin.
     */
    @Test
    fun `hosted pairing targets the host parameter`() {
        val target =
            valid("https://app.t3.codes/pair?host=macbook.local:4488&label=Laptop#token=abcdefgh1234")
        assertEquals("https://macbook.local:4488/", target.httpBaseUrl)
        assertEquals("abcdefgh1234", target.credential)
        assertEquals("Laptop", target.label)
    }

    /** A bare host in the `host` parameter defaults to https, as upstream does. */
    @Test
    fun `hosted pairing defaults a bare backend host to https`() {
        val target = valid("https://app.t3.codes/pair?host=example.dev#token=abcdefgh1234")
        assertEquals("https://example.dev/", target.httpBaseUrl)
    }

    @Test
    fun `ignores surrounding whitespace and extra params`() {
        val target = valid("  http://h:4488/pair?foo=bar#token=abcdefgh1234&baz=1  ")
        assertEquals("abcdefgh1234", target.credential)
        assertNull(target.label)
    }

    @Test
    fun `rejects empty input`() {
        assertEquals(PairingUrlError.Empty, error("   "))
    }

    @Test
    fun `rejects non urls and unsupported schemes`() {
        assertEquals(PairingUrlError.NotAUrl, error("macbook.local:4488?token=abcdefgh1234"))
        assertEquals(PairingUrlError.UnsupportedScheme, error("s5code://pair?token=abcdefgh1234"))
    }

    @Test
    fun `rejects a missing host`() {
        assertEquals(PairingUrlError.MissingHost, error("http://?token=abcdefgh1234"))
        assertEquals(PairingUrlError.MissingHost, error("http://:4488/?token=abcdefgh1234"))
    }

    @Test
    fun `rejects a missing token`() {
        assertEquals(PairingUrlError.MissingCode, error("http://macbook.local:4488/"))
        assertEquals(PairingUrlError.MissingCode, error("http://macbook.local:4488/#token="))
    }

    @Test
    fun `scheme and parameter names are case insensitive`() {
        assertEquals("http://h:4488/", valid("HTTP://h:4488/#TOKEN=abcdefgh1234").httpBaseUrl)
    }

    @Test
    fun `host plus code builds the same target`() {
        val result = pairingTargetFor("macbook.local:4488", "abcdefgh1234")
        val target = (result as PairingUrlResult.Valid).target
        assertEquals("https://macbook.local:4488/", target.httpBaseUrl)
        assertEquals("abcdefgh1234", target.credential)
    }

    @Test
    fun `host plus code reports what is missing`() {
        val noHost = pairingTargetFor("  ", "abcdefgh1234")
        assertEquals(
            PairingUrlError.MissingHostForCode,
            (noHost as PairingUrlResult.Invalid).reason,
        )
        val noCode = pairingTargetFor("macbook.local", " ")
        assertEquals(PairingUrlError.MissingCode, (noCode as PairingUrlResult.Invalid).reason)
    }
}
