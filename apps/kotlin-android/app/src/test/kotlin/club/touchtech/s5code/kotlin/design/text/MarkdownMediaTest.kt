package club.touchtech.s5code.kotlin.design.text

import org.junit.Assert.assertEquals
import org.junit.Test

/** Mirrors the embed cases of mobile's `markdownMedia.test.ts`. */
class MarkdownMediaTest {

    @Test
    fun `direct URLs play audio, hand documents off, and stay images otherwise`() {
        assertEquals(
            MarkdownEmbed.Audio("https://cdn.example.com/sound%20one.mp3?sig=a", "sound one.mp3"),
            classifyMarkdownEmbed("https://cdn.example.com/sound%20one.mp3?sig=a", "/repo"),
        )
        // Protocol-relative URLs become https.
        assertEquals(
            MarkdownEmbed.Document("https://cdn.example.com/report.pdf#page=2", "report.pdf"),
            classifyMarkdownEmbed("//cdn.example.com/report.pdf#page=2", "/repo"),
        )
        assertEquals(MarkdownEmbed.Image("https://example.com/render?id=42"), classifyMarkdownEmbed("https://example.com/render?id=42", null))
        assertEquals(MarkdownEmbed.Image("https://example.com/report.unknown"), classifyMarkdownEmbed("https://example.com/report.unknown", null))
        assertEquals(MarkdownEmbed.Image("https://example.com/"), classifyMarkdownEmbed("https://example.com/", null))
    }

    @Test
    fun `only image data URLs render, and blob or other schemes do not`() {
        assertEquals(MarkdownEmbed.Image("data:image/png;base64,AAAA"), classifyMarkdownEmbed("data:image/png;base64,AAAA", null))
        listOf("data:text/csv,a,b", "blob:https://x/1", "javascript:report.csv", "ftp://server/report.csv", "~/report.csv", "#x", "")
            .forEach { assertEquals(it, MarkdownEmbed.Unavailable, classifyMarkdownEmbed(it, "/repo")) }
    }

    @Test
    fun `host paths open in the viewer only inside the workspace`() {
        assertEquals(MarkdownEmbed.WorkspaceFile("docs/frame#one.png", "frame#one.png"), classifyMarkdownEmbed("./docs/frame%23one.png", "/repo"))
        assertEquals(MarkdownEmbed.WorkspaceFile("sound.mp3", "sound.mp3"), classifyMarkdownEmbed("/repo/sound.mp3", "/repo"))
        assertEquals(MarkdownEmbed.WorkspaceFile("a/b.png", "b.png"), classifyMarkdownEmbed("file:///repo/a/b.png", "/repo"))
        assertEquals(MarkdownEmbed.Unavailable, classifyMarkdownEmbed("/tmp/frame.png", "/repo"))
        assertEquals(MarkdownEmbed.Unavailable, classifyMarkdownEmbed("image.png", null))
        assertEquals(MarkdownEmbed.Unavailable, classifyMarkdownEmbed("../outside.png", "/repo"))
    }

    @Test
    fun `network paths load only from the project's own share`() {
        listOf("\\\\server\\share\\frame.png", "//server/share/frame.png".let { "file:$it" }, "\\\\?\\C:\\frame.png").forEach {
            assertEquals(it, MarkdownEmbed.Unavailable, classifyMarkdownEmbed(it, "/repo"))
        }
        assertEquals(
            MarkdownEmbed.WorkspaceFile("frame.png", "frame.png"),
            classifyMarkdownEmbed("\\\\server\\share\\frame.png", "\\\\server\\share"),
        )
    }
}
