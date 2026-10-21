package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.design.text.MarkdownLinkPresentation
import club.touchtech.s5code.kotlin.design.text.resolveMarkdownLinkPresentation
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Ported from `packages/shared/src/threadLinks.test.ts`. */
class ThreadLinksTest {

    private fun ref(environmentId: String, threadId: String = "chat-1") = ThreadLinkRef(environmentId, threadId)

    @Test
    fun `takes the thread id verbatim, percent escapes included`() {
        assertEquals("mcp:1234", parseThreadLinkHref("t3-thread://v1/mcp:1234"))
        assertEquals(
            "thread:delegated-task:mcp%3A1",
            parseThreadLinkHref("t3-thread://v1/thread:delegated-task:mcp%3A1"),
        )
    }

    @Test
    fun `rejects other links and an empty id`() {
        assertNull(parseThreadLinkHref("https://t3.codes"))
        assertNull(parseThreadLinkHref("t3-thread://v1/"))
        assertNull(parseThreadLinkHref("t3-thread://v1/ "))
    }

    @Test
    fun `decodes like decodeURIComponent`() {
        assertEquals("provider:1", percentDecodedThreadLinkId("provider%3A1"))
        assertNull(percentDecodedThreadLinkId("plain"))
        // `+` is not a space in a URI component, and a malformed escape is no id.
        assertNull(percentDecodedThreadLinkId("a+b"))
        assertNull(percentDecodedThreadLinkId("bad%zz"))
        assertNull(percentDecodedThreadLinkId("bad%+1"))
        assertEquals("é", percentDecodedThreadLinkId("%C3%A9"))
        assertNull(percentDecodedThreadLinkId("%C3"))
    }

    @Test
    fun `prefers the local literal id, then the local decoded id, over remote matches`() {
        val source = ref("local", "provider%3A1")
        val decoded = ref("local", "provider:1")
        val remote = ref("remote", "provider%3A1")
        assertEquals(source, resolveThreadLinkRef(source, listOf(remote, decoded, source)))
        assertEquals(decoded, resolveThreadLinkRef(source, listOf(remote, decoded)))
    }

    @Test
    fun `opens the unique matching chat in another environment`() {
        val destination = ref("remote")
        assertEquals(destination, resolveThreadLinkRef(ref("local"), listOf(ref("local", "other-chat"), destination)))
    }

    @Test
    fun `preserves a remote literal id and decodes only when that id is absent`() {
        val source = ref("local", "provider%3A1")
        val literal = ref("remote", "provider%3A1")
        val decoded = ref("other-remote", "provider:1")
        assertEquals(literal, resolveThreadLinkRef(source, listOf(decoded, literal)))
        assertEquals(decoded, resolveThreadLinkRef(source, listOf(decoded)))
    }

    @Test
    fun `leaves ambiguous or missing destinations unresolved`() {
        val source = ref("local", "provider%3A1")
        val literal = ref("remote", "provider%3A1")
        val decoded = ref("remote", "provider:1")
        assertNull(resolveThreadLinkRef(source, listOf(literal, ref("other-remote", literal.threadId), decoded)))
        assertNull(resolveThreadLinkRef(source, listOf(decoded, ref("other-remote", decoded.threadId))))
        assertNull(resolveThreadLinkRef(source, listOf(ref("remote", "other-chat"))))
    }

    @Test
    fun `markdown presents thread links as their own kind`() {
        assertEquals(
            MarkdownLinkPresentation.Thread("t3-thread://v1/mcp:1", "mcp:1"),
            resolveMarkdownLinkPresentation("t3-thread://v1/mcp:1"),
        )
        assertEquals(MarkdownLinkPresentation.Link(null), resolveMarkdownLinkPresentation("t3-thread://v1/"))
    }
}
