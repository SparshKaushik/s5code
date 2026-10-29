package club.touchtech.s5code.kotlin.design.text

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Mirrors mobile's `markdownHtml.test.ts`. */
class MarkdownHtmlTest {

    @Test
    fun `anchors become links and unsafe hrefs keep plain text`() {
        assertEquals("See [docs](<https://example.com/a?b=1&c=2>) now", markdownWithHtml("See <a href=\"https://example.com/a?b=1&amp;c=2\">docs</a> now"))
        assertEquals("Run this now", markdownWithHtml("Run <a href=\"javascript:alert(1)\">this</a> now"))
        val spans = parseInline(markdownWithHtml("<a href='https://example.com'>Open</a>"))
        assertEquals("https://example.com", spans.single().link)
    }

    @Test
    fun `a linked image keeps the image`() {
        assertEquals(
            "[![Review](<https://example.com/banner.svg>)](<https://example.com>)",
            markdownWithHtml("<a href='https://example.com'><img src='https://example.com/banner.svg' alt='Review' /></a>"),
        )
        val paragraph = parseMarkdown(markdownWithHtml("<img src=\"https://example.com/a b.png\" alt=\"A\">")).single() as MdBlock.Paragraph
        assertEquals(listOf(MdImage("A", "https://example.com/a%20b.png")), paragraph.images)
    }

    @Test
    fun `details and breaks become blocks, but a table row stays on one line`() {
        val blocks = parseMarkdown(markdownWithHtml("<details><summary>Logs</summary>\nAll green<br>Done</details>"))
        assertEquals(listOf("Logs"), (blocks[0] as MdBlock.Paragraph).text.filter { it.bold }.map { it.text })
        assertEquals(3, blocks.size)

        val table =
            parseMarkdown(
                markdownWithHtml(
                    "| Status | Preview |\n| --- | --- |\n| Done | <a href='https://commit.example.com'>Commit</a><br><a href='https://example.com/?a=1&#124;2'>Branch</a> |"
                )
            ).single() as MdBlock.Table
        val cell = table.rows.single()[1]
        assertEquals(listOf("https://commit.example.com", null, "https://example.com/?a=1%7C2"), cell.map { it.link })
    }

    @Test
    fun `code keeps its literal tags`() {
        val source = "```html\n<img src=\"literal.png\">\n```\nUse `<img src=\"inline.png\">` and <b>bold</b>"
        assertEquals("```html\n<img src=\"literal.png\">\n```\nUse `<img src=\"inline.png\">` and bold", markdownWithHtml(source))
    }

    @Test
    fun `autolinks and angle destinations are not tags`() {
        val source = "See <https://example.com/a> or <dev@example.com>, [docs](<span>) and <b>bold</b>"
        assertEquals("See <https://example.com/a> or <dev@example.com>, [docs](<span>) and bold", markdownWithHtml(source))
    }

    @Test
    fun `a break inside a list item or quote stays in it`() {
        assertEquals("- one two\n> three four", markdownWithHtml("- one<br>two\n> three<br/>four"))
        assertEquals("1. one two", markdownWithHtml("1. one<br>two"))
    }

    @Test
    fun `parentheses in a destination are encoded`() {
        assertEquals("[x](<https://example.com/a%28b%29>)", markdownWithHtml("<a href=\"https://example.com/a(b)\">x</a>"))
    }

    @Test
    fun `an unclosed fence in an HTML block stays code through the end`() {
        // Odd, so the last fence is left open.
        val fences = List(40_001) { "```x y" }.joinToString("\n")
        val result = markdownWithHtml("<details>\n<img src=\"before.png\">\n$fences\n<img src=\"after.png\">\n</details>")
        assertTrue(result.contains("![](<before.png>)"))
        assertTrue(!result.contains("<details>"))
        assertTrue(result.endsWith("<img src=\"after.png\">\n</details>"))
    }
}
