package club.touchtech.s5code.kotlin.design.text

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

class HighlightTest {

    @Before
    fun installTextMate() {
        TextMateHighlighter.installClasspathForTest()
    }

    @After
    fun resetTextMate() {
        TextMateHighlighter.resetForTest()
    }

    @Test
    fun `resolves languages from fence labels and paths`() {
        assertEquals(CodeLanguage.Kotlin, codeLanguageOf("kt"))
        assertEquals(CodeLanguage.Tsx, codeLanguageOf("TSX"))
        assertEquals(CodeLanguage.Shell, codeLanguageOf("bash"))
        assertEquals(CodeLanguage.Python, codeLanguageOf("python"))
        assertEquals(CodeLanguage.Rust, codeLanguageOf("rust"))
        assertEquals(CodeLanguage.Haskell, codeLanguageOf("haskell"))
        assertEquals(CodeLanguage.Vue, codeLanguageOf("vue"))
        assertEquals(CodeLanguage.Scss, codeLanguageOf("scss"))
        assertEquals(CodeLanguage.Graphql, codeLanguageOf("graphql"))
        assertEquals(CodeLanguage.Hcl, codeLanguageOf("terraform"))
        assertEquals(CodeLanguage.PowerShell, codeLanguageOf("pwsh"))
        assertEquals(CodeLanguage.Mdx, codeLanguageOf("mdx"))
        assertEquals(CodeLanguage.Plain, codeLanguageOf(null))
        assertEquals(CodeLanguage.Kotlin, codeLanguageOfPath("a/b/Theme.kt"))
        assertEquals(CodeLanguage.Docker, codeLanguageOfPath("docker/Dockerfile"))
        assertEquals(CodeLanguage.Make, codeLanguageOfPath("Makefile"))
        assertEquals(CodeLanguage.Cmake, codeLanguageOfPath("src/CMakeLists.txt"))
        assertEquals(CodeLanguage.Plain, codeLanguageOfPath("LICENSE"))
    }

    @Test
    fun `covers the mobile Shiki language and alias catalog`() {
        val labels =
            listOf(
                "javascript", "typescript", "jsx", "tsx", "python", "rust", "go", "java", "kotlin",
                "swift", "objective-c", "c", "cpp", "csharp", "php", "ruby", "lua", "perl", "r",
                "dart", "scala", "elixir", "haskell", "clojure", "ocaml", "fsharp", "erlang", "zig",
                "nim", "html", "css", "scss", "less", "xml", "svg", "vue", "svelte", "astro", "json",
                "jsonc", "yaml", "toml", "ini", "bash", "shellscript", "powershell", "fish", "sql",
                "graphql", "prisma", "docker", "hcl", "nix", "markdown", "mdx", "tex", "diff", "regex",
                "viml", "makefile", "cmake", "groovy", "mjs", "cts", "py", "rb", "rs", "sh", "zsh",
                "c++", "c#", "dockerfile", "objc", "ps1", "hs", "exs", "erl", "clj", "ml", "fs", "tf",
            )
        assertTrue(labels.all { codeLanguageOf(it) != CodeLanguage.Plain })
    }

    @Test
    fun `tokenizes keywords strings and numbers`() {
        val tokens = highlightLine("val x = \"hi\" + 42", CodeLanguage.Kotlin)
        assertTrue(tokens.any { it.text == "val" && it.kind == CodeTokenKind.Keyword })
        assertTrue(tokens.any { it.text == "\"hi\"" && it.kind == CodeTokenKind.StringLiteral })
        assertTrue(tokens.any { it.text == "42" && it.kind == CodeTokenKind.Number })
    }

    @Test
    fun `tokenizes trailing line comments`() {
        val tokens = highlightLine("val x = 1 // note", CodeLanguage.Kotlin)
        assertEquals("// note", tokens.last().text)
        assertEquals(CodeTokenKind.Comment, tokens.last().kind)
    }

    @Test
    fun `does not treat a comment marker inside a string as a comment`() {
        val tokens = highlightLine("val url = \"https://s5.dev\"", CodeLanguage.Kotlin)
        assertTrue(tokens.none { it.kind == CodeTokenKind.Comment })
        assertTrue(tokens.any { it.kind == CodeTokenKind.StringLiteral && it.text.contains("//") })
    }

    @Test
    fun `recognizes profile-specific comments and escaped strings`() {
        val python = highlightLine("def run(): # note", CodeLanguage.Python)
        assertTrue(python.any { it.text == "def" && it.kind == CodeTokenKind.Keyword })
        assertEquals("# note", python.last().text)
        assertEquals(CodeTokenKind.Comment, python.last().kind)

        val sql = highlightLine("SELECT 'it''s' FROM users -- note", CodeLanguage.Sql)
        assertTrue(sql.any { it.text.equals("select", ignoreCase = true) && it.kind == CodeTokenKind.Keyword })
        assertEquals(CodeTokenKind.Comment, sql.last().kind)

        val escaped = "const value = \"not \\\"done\\\"\" // note"
        val tokens = highlightLine(escaped, CodeLanguage.TypeScript)
        assertEquals(escaped, tokens.joinToString("") { it.text })
        assertEquals(CodeTokenKind.Comment, tokens.last().kind)
    }

    @Test
    fun `recognizes markup and block comments`() {
        val markup = highlightLine("<section class=\"hero\">", CodeLanguage.Html)
        assertTrue(markup.any { it.text.contains("section") && it.kind == CodeTokenKind.Keyword })
        val css = highlightLine("color: red; /* fallback */", CodeLanguage.Css)
        assertEquals(CodeTokenKind.Comment, css.last().kind)
        assertEquals("/* fallback */", css.last().text)
    }

    @Test
    fun `persistent grammar state highlights multiline comments`() {
        val lines = listOf("val value = 1 /* open", "still comment */ val next = 2")
        val highlighted = highlightLines(lines, CodeLanguage.Kotlin)
        assertEquals(lines[0], highlighted[0].joinToString("") { it.text })
        assertEquals(lines[1], highlighted[1].joinToString("") { it.text })
        assertTrue(highlighted[1].first().text.contains("still comment"))
        assertEquals(CodeTokenKind.Comment, highlighted[1].first().kind)
        assertTrue(highlighted[1].any { it.text.contains("val") && it.kind == CodeTokenKind.Keyword })
    }

    @Test
    fun `TextMate scopes distinguish declarations and annotations`() {
        val kotlin = highlightLine("@Composable data class Card(val count: Int)", CodeLanguage.Kotlin)
        assertTrue(kotlin.any { it.text == "@Composable" && it.kind == CodeTokenKind.Annotation })
        assertTrue(kotlin.any { it.text == "data" && it.kind == CodeTokenKind.Keyword })
        assertTrue(kotlin.any { it.text == "Card" && it.kind == CodeTokenKind.Keyword })

        val json = highlightLine("{\"enabled\": true, \"count\": 12}", CodeLanguage.Json)
        assertTrue(json.any { it.text == "\"enabled\"" && it.kind == CodeTokenKind.StringLiteral })
        assertTrue(json.any { it.text == "12" && it.kind == CodeTokenKind.Number })
    }

    @Test
    fun `uninstalled engine fails closed to plain text`() {
        TextMateHighlighter.resetForTest()
        val line = "val answer = 42"
        val tokens = highlightLine(line, CodeLanguage.Kotlin)
        assertEquals(listOf(CodeToken(line, CodeTokenKind.Plain)), tokens)
    }

    @Test
    fun `preserves the whole line when concatenated`() {
        val line = "    fun main(args: Array<String>) { // entry"
        val tokens = highlightLine(line, CodeLanguage.Kotlin)
        assertEquals(line, tokens.joinToString("") { it.text })
    }

    @Test
    fun `unterminated string does not drop characters`() {
        val line = "val broken = \"open"
        assertEquals(line, highlightLine(line, CodeLanguage.Kotlin).joinToString("") { it.text })
    }

    @Test
    fun `plain language returns a single token`() {
        val tokens = highlightLine("anything at all", CodeLanguage.Plain)
        assertEquals(1, tokens.size)
        assertEquals(CodeTokenKind.Plain, tokens.single().kind)
    }

    @Test
    fun `word diff isolates the changed run`() {
        val (removed, added) = wordDiff("val a = 1", "val a = 2")
        assertEquals(1, removed.size)
        assertEquals(1, added.size)
        assertEquals("1", "val a = 1".substring(removed.single()))
        assertEquals("2", "val a = 2".substring(added.single()))
    }

    @Test
    fun `word diff skips excessively long lines`() {
        val longOld = "a".repeat(1_001)
        val longNew = "b".repeat(1_001)
        val (removed, added) = wordDiff(longOld, longNew)
        assertTrue(removed.isEmpty())
        assertTrue(added.isEmpty())
    }

    @Test
    fun `word diff on identical lines finds nothing`() {
        val (removed, added) = wordDiff("same", "same")
        assertTrue(removed.isEmpty())
        assertTrue(added.isEmpty())
    }
}
