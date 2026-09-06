package club.touchtech.s5code.kotlin.design.text

import dev.textmate.grammar.Grammar
import dev.textmate.grammar.raw.GrammarReader
import dev.textmate.grammar.tokenize.StateStack
import dev.textmate.regex.JoniOnigLib
import java.io.InputStream
import java.util.LinkedHashMap

/**
 * Process-wide TextMate grammar registry. The engine is installed once by the
 * application and backed only by APK assets; highlighting never downloads a
 * grammar or executes JavaScript.
 *
 * KotlinTextMate grammars are mutable while compiling and are not thread-safe,
 * so tokenization is serialized behind [lock]. Only eight compiled grammars are
 * retained at once; source and review screens can traverse many file types
 * without growing the process cache without bound.
 */
object TextMateHighlighter {
    private val lock = Any()
    private val onigLib = JoniOnigLib()
    private var assetReader: ((String) -> InputStream)? = null
    private val rawGrammarCache =
        object : LinkedHashMap<String, dev.textmate.grammar.raw.RawGrammar>(MAX_RAW_GRAMMARS, 0.75f, true) {
            override fun removeEldestEntry(
                eldest: MutableMap.MutableEntry<String, dev.textmate.grammar.raw.RawGrammar>?,
            ): Boolean = size > MAX_RAW_GRAMMARS
        }
    private val grammarCache =
        object : LinkedHashMap<String, Grammar>(MAX_COMPILED_GRAMMARS, 0.75f, true) {
            override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, Grammar>?): Boolean =
                size > MAX_COMPILED_GRAMMARS
        }

    fun install(openAsset: (String) -> InputStream) {
        synchronized(lock) {
            assetReader = openAsset
            rawGrammarCache.clear()
            grammarCache.clear()
        }
    }

    fun highlightLine(line: String, grammarScope: String?): List<CodeToken>? =
        highlightLines(listOf(line), grammarScope)?.single()

    fun highlightLines(lines: List<String>, grammarScope: String?): List<List<CodeToken>>? {
        val scope = grammarScope ?: return null
        val reader = assetReader ?: return null
        return synchronized(lock) {
            try {
                val grammar = grammarCache[scope] ?: loadGrammar(scope, reader)?.also { grammarCache[scope] = it }
                grammar?.let { loaded ->
                    var state: StateStack? = null
                    lines.map { line ->
                        val result = loaded.tokenizeLine(line, state)
                        state = result.ruleStack
                        if (line.isEmpty()) emptyList() else result.tokens.toCodeTokens(line)
                    }
                }
            } catch (e: Exception) {
                // A malformed or engine-incompatible grammar must not break a
                // transcript/file render or poison later attempts.
                try {
                    android.util.Log.w("TextMateHighlighter", "Failed to tokenize for scope $scope", e)
                } catch (_: Throwable) {
                    // Running in pure JVM unit test environment
                }
                grammarCache.remove(scope)
                null
            }
        }
    }

    internal fun resetForTest() {
        synchronized(lock) {
            grammarCache.clear()
            rawGrammarCache.clear()
            assetReader = null
        }
    }

    internal fun installClasspathForTest() {
        install { path ->
            checkNotNull(TextMateHighlighter::class.java.classLoader?.getResourceAsStream(path)) {
                "Missing test TextMate asset: $path"
            }
        }
    }

    private fun loadGrammar(scope: String, reader: (String) -> InputStream): Grammar? {
        val raw = loadRawGrammar(scope, reader) ?: return null
        return Grammar(
            rootScopeName = raw.scopeName,
            rawGrammar = raw,
            onigLib = onigLib,
            grammarLookup = { includedScope -> loadRawGrammar(includedScope, reader) },
        )
    }

    private fun loadRawGrammar(scope: String, reader: (String) -> InputStream) =
        rawGrammarCache[scope] ?: GRAMMAR_ASSETS[scope]?.let { path ->
            reader(path).use(GrammarReader::readGrammar).also { rawGrammarCache[scope] = it }
        }

    private fun List<dev.textmate.grammar.Token>.toCodeTokens(line: String): List<CodeToken> {
        val result = mutableListOf<CodeToken>()
        var cursor = 0
        for (token in this) {
            val start = token.startIndex.coerceIn(cursor, line.length)
            val end = token.endIndex.coerceIn(start, line.length)
            if (start > cursor) result.addToken(line.substring(cursor, start), CodeTokenKind.Plain)
            if (end > start) result.addToken(line.substring(start, end), token.scopes.toCodeTokenKind())
            cursor = end
        }
        if (cursor < line.length) result.addToken(line.substring(cursor), CodeTokenKind.Plain)
        return result.ifEmpty { listOf(CodeToken(line, CodeTokenKind.Plain)) }
    }

    private fun MutableList<CodeToken>.addToken(text: String, kind: CodeTokenKind) {
        if (text.isEmpty()) return
        val last = lastOrNull()
        if (last?.kind == kind) {
            this[lastIndex] = last.copy(text = last.text + text)
        } else {
            add(CodeToken(text, kind))
        }
    }

    private fun List<String>.toCodeTokenKind(): CodeTokenKind {
        val scopes = asReversed()
        return when {
            scopes.any { scope -> scope.isCommentScope() } -> CodeTokenKind.Comment
            scopes.any { scope -> scope.isStringScope() } -> CodeTokenKind.StringLiteral
            scopes.any { scope -> scope.isAnnotationScope() } -> CodeTokenKind.Annotation
            scopes.any { scope -> scope.isNumberScope() } -> CodeTokenKind.Number
            scopes.any { scope -> scope.isFunctionScope() } -> CodeTokenKind.Function
            scopes.any { scope -> scope.isTypeScope() } -> CodeTokenKind.Type
            scopes.any { scope -> scope.isVariableScope() } -> CodeTokenKind.Variable
            scopes.any { scope -> scope.isKeywordScope() } -> CodeTokenKind.Keyword
            scopes.any { scope -> scope.isPunctuationScope() } -> CodeTokenKind.Punctuation
            else -> CodeTokenKind.Plain
        }
    }

    private fun String.isCommentScope() =
        startsWith("comment") || contains(".comment")

    private fun String.isStringScope() =
        startsWith("string") ||
            startsWith("constant.character") ||
            contains(".string")

    private fun String.isAnnotationScope() =
        contains("annotation") ||
            startsWith("meta.decorator") ||
            startsWith("entity.name.function.decorator")

    private fun String.isNumberScope() =
        startsWith("constant.numeric") ||
            startsWith("constant.language") ||
            startsWith("constant.character")

    private fun String.isFunctionScope() =
        startsWith("entity.name.function") ||
            startsWith("support.function") ||
            contains(".function-call")

    private fun String.isTypeScope() =
        startsWith("entity.name.type") ||
            startsWith("entity.name.class") ||
            startsWith("support.type") ||
            startsWith("support.class")

    private fun String.isVariableScope() =
        startsWith("variable.parameter") ||
            startsWith("variable.other.readwrite") ||
            startsWith("variable.other.property") ||
            contains("object-literal.key")

    private fun String.isKeywordScope() =
        startsWith("keyword") ||
            startsWith("storage") ||
            startsWith("entity.name.tag") ||
            startsWith("support.class.component") ||
            startsWith("variable.language")

    private fun String.isPunctuationScope() =
        startsWith("punctuation") ||
            startsWith("meta.brace") ||
            startsWith("meta.delimiter") ||
            startsWith("keyword.operator")

    private const val MAX_COMPILED_GRAMMARS = 8
    private const val MAX_RAW_GRAMMARS = 24
}

private val GRAMMAR_ASSETS =
    mapOf(
        "source.asp.vb.net" to "textmate/grammars/vb.json",
        "source.astro" to "textmate/grammars/astro.json",
        "source.batchfile" to "textmate/grammars/bat.json",
        "source.c" to "textmate/grammars/c.json",
        "source.clojure" to "textmate/grammars/clojure.json",
        "source.cmake" to "textmate/grammars/cmake.json",
        "source.coffee" to "textmate/grammars/coffee.json",
        "source.cpp" to "textmate/grammars/cpp.json",
        "source.cpp.embedded.macro" to "textmate/grammars/cpp-macro.json",
        "source.cs" to "textmate/grammars/csharp.json",
        "source.css" to "textmate/grammars/css.json",
        "source.css.less" to "textmate/grammars/less.json",
        "source.css.postcss" to "textmate/grammars/postcss.json",
        "source.css.scss" to "textmate/grammars/scss.json",
        "source.dart" to "textmate/grammars/dart.json",
        "source.diff" to "textmate/grammars/diff.json",
        "source.dockerfile" to "textmate/grammars/docker.json",
        "source.elixir" to "textmate/grammars/elixir.json",
        "source.elm" to "textmate/grammars/elm.json",
        "source.erlang" to "textmate/grammars/erlang.json",
        "source.fish" to "textmate/grammars/fish.json",
        "source.fsharp" to "textmate/grammars/fsharp.json",
        "source.go" to "textmate/grammars/go.json",
        "source.graphql" to "textmate/grammars/graphql.json",
        "source.groovy" to "textmate/grammars/groovy.json",
        "source.haskell" to "textmate/grammars/haskell.json",
        "source.hcl" to "textmate/grammars/hcl.json",
        "source.ini" to "textmate/grammars/ini.json",
        "source.java" to "textmate/grammars/java.json",
        "source.js" to "textmate/grammars/javascript.json",
        "source.js.jsx" to "textmate/grammars/jsx.json",
        "source.json" to "textmate/grammars/json.json",
        "source.json.comments" to "textmate/grammars/jsonc.json",
        "source.json.lines" to "textmate/grammars/jsonl.json",
        "source.json5" to "textmate/grammars/json5.json",
        "source.julia" to "textmate/grammars/julia.json",
        "source.kotlin" to "textmate/grammars/kotlin.json",
        "source.lua" to "textmate/grammars/lua.json",
        "source.makefile" to "textmate/grammars/make.json",
        "source.mdx" to "textmate/grammars/mdx.json",
        "source.nim" to "textmate/grammars/nim.json",
        "source.nix" to "textmate/grammars/nix.json",
        "source.objc" to "textmate/grammars/objective-c.json",
        "source.ocaml" to "textmate/grammars/ocaml.json",
        "source.perl" to "textmate/grammars/perl.json",
        "source.perl.6" to "textmate/grammars/raku.json",
        "source.php" to "textmate/grammars/php.json",
        "source.powershell" to "textmate/grammars/powershell.json",
        "source.prisma" to "textmate/grammars/prisma.json",
        "source.python" to "textmate/grammars/python.json",
        "source.r" to "textmate/grammars/r.json",
        "source.regexp.python" to "textmate/grammars/regexp.json",
        "source.rst" to "textmate/grammars/rst.json",
        "source.ruby" to "textmate/grammars/ruby.json",
        "source.rust" to "textmate/grammars/rust.json",
        "source.sass" to "textmate/grammars/sass.json",
        "source.scala" to "textmate/grammars/scala.json",
        "source.shell" to "textmate/grammars/shellscript.json",
        "source.sql" to "textmate/grammars/sql.json",
        "source.stylus" to "textmate/grammars/stylus.json",
        "source.svelte" to "textmate/grammars/svelte.json",
        "source.swift" to "textmate/grammars/swift.json",
        "source.toml" to "textmate/grammars/toml.json",
        "source.ts" to "textmate/grammars/typescript.json",
        "source.tsx" to "textmate/grammars/tsx.json",
        "source.viml" to "textmate/grammars/viml.json",
        "source.yaml" to "textmate/grammars/yaml.json",
        "source.zig" to "textmate/grammars/zig.json",
        "text.bibtex" to "textmate/grammars/bibtex.json",
        "text.git-commit" to "textmate/grammars/git-commit.json",
        "text.git-rebase" to "textmate/grammars/git-rebase.json",
        "text.haml" to "textmate/grammars/haml.json",
        "text.html.basic" to "textmate/grammars/html.json",
        "text.html.derivative" to "textmate/grammars/html-derivative.json",
        "text.html.handlebars" to "textmate/grammars/handlebars.json",
        "text.html.markdown" to "textmate/grammars/markdown.json",
        "text.html.vue" to "textmate/grammars/vue.json",
        "text.log" to "textmate/grammars/log.json",
        "text.pug" to "textmate/grammars/pug.json",
        "text.shell-session" to "textmate/grammars/shellsession.json",
        "text.tex" to "textmate/grammars/tex.json",
        "text.tex.latex" to "textmate/grammars/latex.json",
        "text.xml" to "textmate/grammars/xml.json",
        "text.xml.xsl" to "textmate/grammars/xsl.json",
    )
