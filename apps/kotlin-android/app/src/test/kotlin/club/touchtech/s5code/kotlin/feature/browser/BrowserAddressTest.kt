package club.touchtech.s5code.kotlin.feature.browser

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Port of the shared `preview` tests for `normalizePreviewUrl`. */
class BrowserAddressTest {

    @Test
    fun `treats bare loopback hosts as http and public hosts as https`() {
        assertEquals("http://localhost:5173/", normalizePreviewUrl("localhost:5173"))
        assertEquals("http://127.0.0.1:3000/", normalizePreviewUrl("127.0.0.1:3000"))
        assertEquals("https://example.com/", normalizePreviewUrl("example.com"))
    }

    @Test
    fun `respects explicit schemes`() {
        assertEquals("https://localhost:5173/", normalizePreviewUrl("https://localhost:5173"))
        assertEquals("http://example.com/path?q=1", normalizePreviewUrl("http://example.com/path?q=1"))
    }

    @Test
    fun `opens the outer address when it contains another URL`() {
        for ((input, expected) in
            listOf(
                "localhost:3000/?next=https://example.com" to "http://localhost:3000/?next=https://example.com",
                "127.0.0.1:3000/#https://example.com" to "http://127.0.0.1:3000/#https://example.com",
                "example.com/https://example.org" to "https://example.com/https://example.org",
            )) {
            assertEquals(input, expected, normalizePreviewUrl(input))
        }
    }

    @Test
    fun `rejects empty, non-web and unparseable input`() {
        assertNull(normalizePreviewUrl("   "))
        assertNull(normalizePreviewUrl("ftp://example.com"))
        assertNull(normalizePreviewUrl("https://user:password@example.com:bad/path?access_token=secret#fragment"))
    }
}
