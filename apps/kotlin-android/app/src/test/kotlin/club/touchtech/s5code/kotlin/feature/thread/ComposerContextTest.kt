package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.model.FeedEntry
import org.junit.Test
import org.junit.Assert.assertEquals

/**
 * `t3-context://` reference rendering, mirroring `UserMessageContent` in the RN
 * feed: a record the message carries keeps its label; a missing one is marked
 * unavailable rather than promising a payload that is not there.
 */
class ComposerContextTest {
    @Test
    fun `a reference with a backing record renders its label`() {
        val text = "Look at [Main.kt line 12](t3-context://v1/file/ctx_1) please"
        val records =
            mapOf("ctx_1" to FeedEntry.ContextRecordLabel(kind = "file", label = "Main.kt"))
        assertEquals(
            "Look at Main.kt line 12 please",
            replaceComposerContextReferences(text, records),
        )
    }

    @Test
    fun `a reference without a record is marked unavailable`() {
        val text = "Check [old note](t3-context://v1/review-comment/ctx_9)"
        assertEquals(
            "Check old note (unavailable)",
            replaceComposerContextReferences(text, emptyMap()),
        )
    }

    @Test
    fun `malformed hrefs and plain markdown links pass through untouched`() {
        val text = "See [docs](https://example.com) and [bad](t3-context://v2/x/y)"
        assertEquals(text, replaceComposerContextReferences(text, emptyMap()))
    }

    @Test
    fun `multiple references each resolve independently`() {
        val text =
            "Compare [A](t3-context://v1/terminal/ctx_1) and [B](t3-context://v1/terminal/ctx_2)"
        val records =
            mapOf("ctx_1" to FeedEntry.ContextRecordLabel(kind = "terminal", label = "A"))
        assertEquals(
            "Compare A and B (unavailable)",
            replaceComposerContextReferences(text, records),
        )
    }
}
