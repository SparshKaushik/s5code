package club.touchtech.s5code.kotlin.design.text

import org.junit.Assert.assertEquals
import org.junit.Test

class CodexDirectivesTest {

    @Test
    fun `a follow-up directive shows its label`() {
        assertEquals(
            "Next: Run the tests now",
            renderCodexInlineDirectivesAsMarkdown(
                """Next: :codex-followup[Run the tests]{prompt="Run the tests" id=a1} now""",
            ),
        )
    }

    @Test
    fun `a follow-up without a label or prompt stays as written`() {
        val noPrompt = """:codex-followup[Run]{prompt=""}"""
        val noLabel = """:codex-followup{prompt="Run"}"""
        assertEquals(noPrompt, renderCodexInlineDirectivesAsMarkdown(noPrompt))
        assertEquals(noLabel, renderCodexInlineDirectivesAsMarkdown(noLabel))
    }

    @Test
    fun `a file citation becomes a link to the path and line`() {
        assertEquals(
            "See [my\\_file.kt](<src/my_file.kt#L12>).",
            renderCodexInlineDirectivesAsMarkdown(
                """See :codex-file-citation{path="src/my_file.kt" line_range_start="12"}.""",
            ),
        )
        assertEquals(
            "[a#b](<a%23b>)",
            renderCodexInlineDirectivesAsMarkdown(""":codex-file-citation{path="a#b" line_range_start="0"}"""),
        )
    }

    @Test
    fun `directives inside code are text`() {
        val source = "`:codex-followup[x]{prompt=\"y\"}`\n```\n:codex-followup[x]{prompt=\"y\"}\n```"
        assertEquals(source, renderCodexInlineDirectivesAsMarkdown(source))
    }

    @Test
    fun `the parsed citation link opens the path without escapes`() {
        val spans = parseInline(renderCodexInlineDirectivesAsMarkdown(""":codex-file-citation{path="src/my_file.kt"}"""))
        assertEquals("my_file.kt", spans.single().text)
        assertEquals("src/my_file.kt", spans.single().link)
    }

    @Test
    fun `citation paths with parentheses and brackets stay whole`() {
        val group = parseInline(renderCodexInlineDirectivesAsMarkdown(""":codex-file-citation{path="app/(marketing)/page.tsx"}"""))
        assertEquals("page.tsx", group.single().text)
        assertEquals("app/(marketing)/page.tsx", group.single().link)
        val dynamic = parseInline(renderCodexInlineDirectivesAsMarkdown(""":codex-file-citation{path="pages/[id].tsx" line_range_start="3"} next"""))
        assertEquals("[id].tsx", dynamic.first().text)
        assertEquals("pages/[id].tsx#L3", dynamic.first().link)
        assertEquals(" next", dynamic.last().text)
    }
}
