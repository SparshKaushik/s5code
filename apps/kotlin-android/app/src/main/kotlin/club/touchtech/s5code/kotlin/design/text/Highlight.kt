package club.touchtech.s5code.kotlin.design.text

/**
 * Grammar-backed syntax highlighting used by Markdown fences, source files, and
 * diffs. Labels mirror the React Native Shiki catalog, while packaged TextMate
 * assets keep the implementation native and deterministic: no JavaScript or
 * runtime grammar downloads. Unsupported labels still degrade to plain text.
 */
enum class CodeTokenKind {
    Plain,
    Keyword,
    StringLiteral,
    Number,
    Comment,
    Annotation,
    Punctuation,
}

data class CodeToken(val text: String, val kind: CodeTokenKind)

/** Exact bundled TextMate grammar resolved from Markdown labels and file extensions. */
enum class CodeLanguage(val grammarScope: String?) {
    JavaScript("source.js"),
    TypeScript("source.ts"),
    Jsx("source.js.jsx"),
    Tsx("source.tsx"),
    Python("source.python"),
    Rust("source.rust"),
    Go("source.go"),
    Java("source.java"),
    Kotlin("source.kotlin"),
    Swift("source.swift"),
    ObjectiveC("source.objc"),
    C("source.c"),
    Cpp("source.cpp"),
    CSharp("source.cs"),
    Php("source.php"),
    Ruby("source.ruby"),
    Lua("source.lua"),
    Perl("source.perl"),
    R("source.r"),
    Dart("source.dart"),
    Scala("source.scala"),
    Elixir("source.elixir"),
    Haskell("source.haskell"),
    Clojure("source.clojure"),
    Ocaml("source.ocaml"),
    FSharp("source.fsharp"),
    Erlang("source.erlang"),
    Zig("source.zig"),
    Nim("source.nim"),
    Html("text.html.basic"),
    Css("source.css"),
    Scss("source.css.scss"),
    Less("source.css.less"),
    Xml("text.xml"),
    Vue("text.html.vue"),
    Svelte("source.svelte"),
    Astro("source.astro"),
    Json("source.json"),
    Jsonc("source.json.comments"),
    Yaml("source.yaml"),
    Toml("source.toml"),
    Ini("source.ini"),
    Shell("source.shell"),
    PowerShell("source.powershell"),
    Fish("source.fish"),
    Sql("source.sql"),
    Graphql("source.graphql"),
    Prisma("source.prisma"),
    Docker("source.dockerfile"),
    Hcl("source.hcl"),
    Nix("source.nix"),
    Markdown("text.html.markdown"),
    Mdx("source.mdx"),
    Latex("text.tex.latex"),
    Diff("source.diff"),
    Regex("source.regexp.python"),
    Vim("source.viml"),
    Make("source.makefile"),
    Cmake("source.cmake"),
    Groovy("source.groovy"),
    Plain(null),
}

private val LANGUAGE_BY_LABEL: Map<String, CodeLanguage> by lazy {
    buildMap {
        fun aliases(language: CodeLanguage, vararg labels: String) = labels.forEach { put(it, language) }
        aliases(CodeLanguage.JavaScript, "javascript", "js", "mjs", "cjs")
        aliases(CodeLanguage.TypeScript, "typescript", "ts", "mts", "cts")
        aliases(CodeLanguage.Jsx, "jsx")
        aliases(CodeLanguage.Tsx, "tsx")
        aliases(CodeLanguage.Python, "python", "py")
        aliases(CodeLanguage.Rust, "rust", "rs")
        aliases(CodeLanguage.Go, "go")
        aliases(CodeLanguage.Java, "java")
        aliases(CodeLanguage.Kotlin, "kotlin", "kt", "kts")
        aliases(CodeLanguage.Swift, "swift")
        aliases(CodeLanguage.ObjectiveC, "objective-c", "objectivec", "objc", "obj-c")
        aliases(CodeLanguage.C, "c", "h")
        aliases(CodeLanguage.Cpp, "cpp", "c++", "cc", "cxx", "hpp")
        aliases(CodeLanguage.CSharp, "csharp", "c#", "cs")
        aliases(CodeLanguage.Php, "php")
        aliases(CodeLanguage.Ruby, "ruby", "rb")
        aliases(CodeLanguage.Lua, "lua")
        aliases(CodeLanguage.Perl, "perl")
        aliases(CodeLanguage.R, "r")
        aliases(CodeLanguage.Dart, "dart")
        aliases(CodeLanguage.Scala, "scala")
        aliases(CodeLanguage.Elixir, "elixir", "ex", "exs")
        aliases(CodeLanguage.Haskell, "haskell", "hs")
        aliases(CodeLanguage.Clojure, "clojure", "clj")
        aliases(CodeLanguage.Ocaml, "ocaml", "ml")
        aliases(CodeLanguage.FSharp, "fsharp", "fs")
        aliases(CodeLanguage.Erlang, "erlang", "erl")
        aliases(CodeLanguage.Zig, "zig")
        aliases(CodeLanguage.Nim, "nim")
        aliases(CodeLanguage.Html, "html", "svg")
        aliases(CodeLanguage.Css, "css")
        aliases(CodeLanguage.Scss, "scss", "sass")
        aliases(CodeLanguage.Less, "less")
        aliases(CodeLanguage.Xml, "xml")
        aliases(CodeLanguage.Vue, "vue")
        aliases(CodeLanguage.Svelte, "svelte")
        aliases(CodeLanguage.Astro, "astro")
        aliases(CodeLanguage.Json, "json")
        aliases(CodeLanguage.Jsonc, "jsonc")
        aliases(CodeLanguage.Yaml, "yaml", "yml")
        aliases(CodeLanguage.Toml, "toml")
        aliases(CodeLanguage.Ini, "ini")
        aliases(CodeLanguage.Shell, "bash", "sh", "zsh", "shell", "shellscript", "console")
        aliases(CodeLanguage.PowerShell, "powershell", "ps1", "pwsh")
        aliases(CodeLanguage.Fish, "fish")
        aliases(CodeLanguage.Sql, "sql")
        aliases(CodeLanguage.Graphql, "graphql", "gql")
        aliases(CodeLanguage.Prisma, "prisma")
        aliases(CodeLanguage.Docker, "docker", "dockerfile")
        aliases(CodeLanguage.Hcl, "hcl", "tf", "terraform")
        aliases(CodeLanguage.Nix, "nix")
        aliases(CodeLanguage.Markdown, "markdown", "md")
        aliases(CodeLanguage.Mdx, "mdx")
        aliases(CodeLanguage.Latex, "tex", "latex")
        aliases(CodeLanguage.Diff, "diff")
        aliases(CodeLanguage.Regex, "regex", "regexp")
        aliases(CodeLanguage.Vim, "vim", "viml")
        aliases(CodeLanguage.Make, "make", "makefile")
        aliases(CodeLanguage.Cmake, "cmake")
        aliases(CodeLanguage.Groovy, "groovy")
        aliases(CodeLanguage.Plain, "text", "plain", "plaintext", "txt")
    }
}

fun codeLanguageOf(label: String?): CodeLanguage =
    label?.trim()?.lowercase()?.let(LANGUAGE_BY_LABEL::get) ?: CodeLanguage.Plain

/** Resolves special filenames before their extension so extensionless files work. */
fun codeLanguageOfPath(path: String): CodeLanguage {
    val name = path.substringBefore('?').substringBefore('#').substringAfterLast('/').substringAfterLast('\\')
    return when (name.lowercase()) {
        "dockerfile" -> CodeLanguage.Docker
        "makefile", "gnumakefile" -> CodeLanguage.Make
        "cmakelists.txt" -> CodeLanguage.Cmake
        else -> codeLanguageOf(name.substringAfterLast('.', missingDelimiterValue = ""))
    }
}

/**
 * Tokenizes lines with persistent TextMate state, so multi-line comments and
 * strings keep their grammar context. Unknown languages fail closed to plain.
 */
fun highlightLines(lines: List<String>, language: CodeLanguage): List<List<CodeToken>> =
    TextMateHighlighter.highlightLines(lines, language.grammarScope)
        ?: lines.map { line ->
            if (line.isEmpty()) emptyList() else listOf(CodeToken(line, CodeTokenKind.Plain))
        }

/** Tokenizes one independent line. Prefer [highlightLines] for complete files or blocks. */
fun highlightLine(line: String, language: CodeLanguage): List<CodeToken> {
    if (line.isEmpty()) return emptyList()
    return TextMateHighlighter.highlightLine(line, language.grammarScope)
        ?: listOf(CodeToken(line, CodeTokenKind.Plain))
}

/** Word-level diff between two lines, used by the review word-diff view. */
fun wordDiff(old: String, new: String): Pair<List<IntRange>, List<IntRange>> {
    if (old.length > MAX_WORD_DIFF_LINE_LENGTH || new.length > MAX_WORD_DIFF_LINE_LENGTH) {
        return emptyList<IntRange>() to emptyList()
    }
    val oldWords = splitWords(old)
    val newWords = splitWords(new)
    var prefix = 0
    while (
        prefix < oldWords.size &&
        prefix < newWords.size &&
        old.substring(oldWords[prefix]) == new.substring(newWords[prefix])
    ) {
        prefix++
    }
    var suffix = 0
    while (
        suffix < oldWords.size - prefix &&
        suffix < newWords.size - prefix &&
        old.substring(oldWords[oldWords.size - 1 - suffix]) ==
            new.substring(newWords[newWords.size - 1 - suffix])
    ) {
        suffix++
    }
    val removed = oldWords.subList(prefix, oldWords.size - suffix)
    val added = newWords.subList(prefix, newWords.size - suffix)
    return removed to added
}

private const val MAX_WORD_DIFF_LINE_LENGTH = 1_000

private fun splitWords(line: String): List<IntRange> {
    val ranges = mutableListOf<IntRange>()
    var start = 0
    while (start < line.length) {
        val char = line[start]
        var end = start + 1
        if (char.isLetterOrDigit() || char == '_') {
            while (end < line.length && (line[end].isLetterOrDigit() || line[end] == '_')) end++
        }
        ranges += start until end
        start = end
    }
    return ranges
}
