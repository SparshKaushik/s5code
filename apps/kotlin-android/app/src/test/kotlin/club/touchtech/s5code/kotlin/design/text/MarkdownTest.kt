package club.touchtech.s5code.kotlin.design.text

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MarkdownTest {

    @Test
    fun `parses headings paragraphs and rules`() {
        val blocks =
            parseMarkdown(
                """
                # Title

                Body text.

                ---
                """
                    .trimIndent()
            )
        assertEquals(3, blocks.size)
        val heading = blocks[0] as MdBlock.Heading
        assertEquals(1, heading.level)
        assertEquals("Title", heading.text.single().text)
        assertTrue(blocks[1] is MdBlock.Paragraph)
        assertEquals(MdBlock.Rule, blocks[2])
    }

    @Test
    fun `joins soft-wrapped paragraph lines`() {
        val blocks = parseMarkdown("one\ntwo\nthree")
        val paragraph = blocks.single() as MdBlock.Paragraph
        assertEquals("one two three", paragraph.text.single().text)
    }

    @Test
    fun `parses fenced code with language and trims blank edges`() {
        val blocks =
            parseMarkdown(
                """
                ```kotlin

                val x = 1

                ```
                """
                    .trimIndent()
            )
        val code = blocks.single() as MdBlock.Code
        assertEquals("kotlin", code.language)
        assertEquals(listOf("val x = 1"), code.lines)
    }

    @Test
    fun `fenced code keeps markdown syntax literal`() {
        val blocks = parseMarkdown("```\n# not a heading\n- not a list\n```")
        val code = blocks.single() as MdBlock.Code
        assertEquals(listOf("# not a heading", "- not a list"), code.lines)
    }

    @Test
    fun `parses bullet and task list items`() {
        val blocks = parseMarkdown("- plain\n- [x] done\n- [ ] pending")
        val bullets = blocks.single() as MdBlock.Bullets
        assertEquals(3, bullets.items.size)
        assertEquals(null, bullets.items[0].checked)
        assertEquals(true, bullets.items[1].checked)
        assertEquals(false, bullets.items[2].checked)
        assertEquals("done", bullets.items[1].text.single().text)
    }

    @Test
    fun `parses ordered lists`() {
        val blocks = parseMarkdown("1. first\n2. second")
        val bullets = blocks.single() as MdBlock.Bullets
        assertTrue(bullets.ordered)
        assertEquals("1.", bullets.items[0].marker)
        assertEquals("second", bullets.items[1].text.single().text)
    }

    @Test
    fun `parses block quotes recursively`() {
        val blocks = parseMarkdown("> quoted **bold**\n> more")
        val quote = blocks.single() as MdBlock.Quote
        val paragraph = quote.blocks.single() as MdBlock.Paragraph
        assertTrue(paragraph.text.any { it.bold })
    }

    @Test
    fun `parses pipe tables`() {
        val blocks =
            parseMarkdown(
                """
                | Token | Use |
                | --- | --- |
                | Hero | one per screen |
                | Primary | commit actions |
                """
                    .trimIndent()
            )
        val table = blocks.single() as MdBlock.Table
        assertEquals(listOf("Token", "Use"), table.header.map { it.single().text })
        assertEquals(listOf(MdTableAlignment.Start, MdTableAlignment.Start), table.alignments)
        assertEquals(2, table.rows.size)
        assertEquals("one per screen", table.rows[0][1].single().text)
    }

    @Test
    fun `parses table alignment and escaped or code pipes`() {
        val table =
            parseMarkdown(
                """
                | Left | Center | Right |
                | :--- | :----: | ----: |
                | a\|b | `x|y` | **bold** |
                """.trimIndent()
            ).single() as MdBlock.Table

        assertEquals(
            listOf(MdTableAlignment.Start, MdTableAlignment.Center, MdTableAlignment.End),
            table.alignments,
        )
        assertEquals("a|b", table.rows[0][0].single().text)
        assertTrue(table.rows[0][1].single().code)
        assertEquals("x|y", table.rows[0][1].single().text)
        assertTrue(table.rows[0][2].single().bold)
    }

    @Test
    fun `normalizes short and long table rows to the header width`() {
        val table =
            parseMarkdown(
                """
                A | B
                --- | ---
                | one |
                two | three | ignored
                """.trimIndent()
            ).single() as MdBlock.Table
        assertEquals(2, table.rows[0].size)
        assertEquals("", table.rows[0][1].joinToString("") { it.text })
        assertEquals(listOf("two", "three"), table.rows[1].map { it.single().text })
    }

    @Test
    fun `a malformed delimiter remains paragraph text`() {
        val blocks = parseMarkdown("A | B\n-- | ---\nvalue | row")
        assertTrue(blocks.none { it is MdBlock.Table })
        assertEquals("A | B -- | --- value | row", (blocks.single() as MdBlock.Paragraph).text.single().text)
    }

    @Test
    fun `parses Markdown images out of paragraph text`() {
        val paragraph =
            parseMarkdown("Before ![diagram](https://example.com/diagram.png \"Architecture\") after")
                .single() as MdBlock.Paragraph
        assertEquals("Before  after", paragraph.text.single().text)
        assertEquals(
            MdImage("diagram", "https://example.com/diagram.png", "Architecture"),
            paragraph.images.single(),
        )
    }

    @Test
    fun `inline parses emphasis code and links`() {
        val spans = parseInline("a **b** `c` [d](https://e.dev) ~~f~~ *g*")
        assertTrue(spans.any { it.bold && it.text == "b" })
        assertTrue(spans.any { it.code && it.text == "c" })
        assertTrue(spans.any { it.link == "https://e.dev" && it.text == "d" })
        assertTrue(spans.any { it.strike && it.text == "f" })
        assertTrue(spans.any { it.italic && it.text == "g" })
    }

    @Test
    fun `unclosed markers stay literal`() {
        assertEquals("a * b", parseInline("a * b").joinToString("") { it.text })
        assertEquals("a ` b", parseInline("a ` b").joinToString("") { it.text })
        assertEquals("[a](b", parseInline("[a](b").joinToString("") { it.text })
    }

    @Test
    fun `round trips text content for a mixed document`() {
        val source =
            """
            # H

            para with `code`

            - one
            - two
            """
                .trimIndent()
        val text =
            parseMarkdown(source).joinToString(" ") { block ->
                when (block) {
                    is MdBlock.Heading -> block.text.joinToString("") { it.text }
                    is MdBlock.Paragraph ->
                        block.text.joinToString("") { it.text } + block.images.joinToString("") { it.alt }
                    is MdBlock.Bullets -> block.items.joinToString(" ") { item ->
                        item.text.joinToString("") { it.text }
                    }
                    else -> ""
                }
            }
        assertEquals("H para with code one two", text)
    }

    @Test
    fun `handles empty and whitespace input`() {
        assertTrue(parseMarkdown("").isEmpty())
        assertTrue(parseMarkdown("\n\n   \n").isEmpty())
    }
}
