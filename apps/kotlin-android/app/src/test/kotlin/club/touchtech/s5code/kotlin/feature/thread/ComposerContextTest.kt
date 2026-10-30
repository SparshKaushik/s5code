package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.model.FeedEntry
import org.junit.Test
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull

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

    @Test
    fun `a line-leading path does not open the slash menu`() {
        assertNull(detectComposerTrigger("/etc/hosts"))
        assertNull(detectComposerTrigger("/tmp/build.sh"))
        assertNull(detectComposerTrigger("first line\n/tmp/"))
    }

    @Test
    fun `slash commands and model queries still trigger`() {
        assertEquals(
            ComposerTrigger(ComposerTriggerKind.SlashCommand, "rev", 0, 4),
            detectComposerTrigger("/rev"),
        )
        assertEquals(
            ComposerTrigger(ComposerTriggerKind.SlashModel, "openai/gpt", 0, 17),
            detectComposerTrigger("/model openai/gpt"),
        )
    }

    @Test
    fun `typing a price does not open the skill menu`() {
        listOf(
            "It costs $5", "budget $9.99", "only $.99", "about €20", "raise $10k", "$1,000",
            "£2e6", "raise $10k.", "is it $5?", "(about $9.99)",
        ).forEach { text -> assertNull(text, detectComposerTrigger(text)) }
        listOf("$", "$1password", "$2fa").forEach { text ->
            assertEquals(text, ComposerTriggerKind.Skill, detectComposerTrigger(text)?.kind)
            assertEquals(text, text.substring(1), detectComposerTrigger(text)?.query)
        }
    }
}
