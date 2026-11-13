package club.touchtech.s5code.kotlin.design.text

private const val CODEX_FILE_CITATION_NAME = "codex-file-citation"
private const val CODEX_FOLLOW_UP_NAME = "codex-followup"

/**
 * `renderCodexInlineDirectivesAsMarkdown` from
 * `packages/shared/src/codexMarkdownDirectives.ts`: Codex's inline directives
 * become plain markdown before parsing. `:codex-followup[Label]{prompt="…"}`
 * shows its label; `:codex-file-citation{path="…" line_range_start="…"}`
 * becomes a file link. A directive missing what it needs stays as written.
 *
 * Upstream parses with micromark's directive extension; this scanner covers the
 * same text-directive grammar (`:name`, optional `[label]`, optional
 * `{attributes}`) and skips fenced code and code spans, where directives are text.
 */
fun renderCodexInlineDirectivesAsMarkdown(markdown: String): String {
    if (!markdown.contains(":$CODEX_FILE_CITATION_NAME") && !markdown.contains(":$CODEX_FOLLOW_UP_NAME")) {
        return markdown
    }
    val out = StringBuilder(markdown.length)
    var fence: String? = null
    val lines = markdown.split('\n')
    lines.forEachIndexed { index, line ->
        val trimmed = line.trimStart()
        val marker = trimmed.takeWhile { it == '`' || it == '~' }
        val open = fence
        when {
            open != null -> {
                if (marker.length >= open.length && marker.all { it == open[0] } && trimmed.trim() == marker) {
                    fence = null
                }
                out.append(line)
            }
            marker.length >= 3 && marker.all { it == marker[0] } -> {
                fence = marker
                out.append(line)
            }
            else -> out.append(renderLine(line))
        }
        if (index < lines.lastIndex) out.append('\n')
    }
    return out.toString()
}

private fun renderLine(line: String): String {
    val out = StringBuilder(line.length)
    var index = 0
    while (index < line.length) {
        val char = line[index]
        if (char == '`') {
            // A code span runs to the next backtick run of the same length.
            val run = line.substring(index).takeWhile { it == '`' }
            val close = line.indexOf(run, index + run.length)
            val end = if (close < 0) index + run.length else close + run.length
            out.append(line, index, end)
            index = end
            continue
        }
        if (char == ':' && (index == 0 || line[index - 1] != ':')) {
            val directive = parseTextDirective(line, index)
            if (directive != null) {
                val replacement = directive.replacement()
                if (replacement != null) {
                    out.append(replacement)
                    index = directive.end
                    continue
                }
            }
        }
        out.append(char)
        index++
    }
    return out.toString()
}

private class TextDirective(
    val name: String,
    val label: String?,
    val attributes: Map<String, String>,
    val end: Int,
) {
    fun replacement(): String? =
        when (name) {
            CODEX_FOLLOW_UP_NAME ->
                label?.takeIf {
                    it.isNotBlank() && !attributes["prompt"].isNullOrBlank()
                }
            CODEX_FILE_CITATION_NAME -> codexFileCitationMarkdown(attributes)
            else -> null
        }
}

private fun parseTextDirective(line: String, start: Int): TextDirective? {
    val name =
        listOf(CODEX_FILE_CITATION_NAME, CODEX_FOLLOW_UP_NAME).firstOrNull { line.startsWith(it, start + 1) }
            ?: return null
    var index = start + 1 + name.length
    // The name ends at whitespace or punctuation other than `-` and `_`.
    line.getOrNull(index)?.let { next ->
        if (next.isLetterOrDigit() || next == '-' || next == '_') return null
    }
    var label: String? = null
    if (line.getOrNull(index) == '[') {
        var depth = 0
        var cursor = index
        while (cursor < line.length) {
            when (line[cursor]) {
                '\\' -> cursor++
                '[' -> depth++
                ']' -> if (--depth == 0) break
            }
            cursor++
        }
        if (cursor >= line.length) return null
        label = line.substring(index + 1, cursor)
        index = cursor + 1
    }
    val attributes = mutableMapOf<String, String>()
    if (line.getOrNull(index) == '{') {
        val parsed = parseAttributes(line, index + 1, attributes) ?: return null
        index = parsed
    }
    return TextDirective(name, label, attributes, index)
}

/** Reads `key="v"`, `key='v'`, `key=v`, `#id`, `.class` and bare keys up to `}`; returns the index after it. */
private fun parseAttributes(line: String, from: Int, into: MutableMap<String, String>): Int? {
    var index = from
    while (index < line.length) {
        val char = line[index]
        when {
            char == '}' -> return index + 1
            char.isWhitespace() -> index++
            else -> {
                val keyStart = index
                while (index < line.length && line[index] !in " \t=}\"'<>`") index++
                if (index == keyStart) return null
                val key = line.substring(keyStart, index)
                if (line.getOrNull(index) != '=') {
                    when {
                        key.startsWith("#") -> into["id"] = key.drop(1)
                        key.startsWith(".") -> into["class"] = key.drop(1)
                        else -> into[key] = ""
                    }
                    continue
                }
                index++
                val quote = line.getOrNull(index)
                if (quote == '"' || quote == '\'') {
                    val close = line.indexOf(quote, index + 1)
                    if (close < 0) return null
                    into[key] = line.substring(index + 1, close)
                    index = close + 1
                } else {
                    val valueStart = index
                    while (index < line.length && line[index] !in " \t}\"'=<>`") index++
                    if (index == valueStart) return null
                    into[key] = line.substring(valueStart, index)
                }
            }
        }
    }
    return null
}

/** `resolveCodexFileCitationLink` + `codexFileCitationMarkdown`; null without a path. */
private fun codexFileCitationMarkdown(attributes: Map<String, String>): String? {
    val path = attributes["path"]?.trim()?.takeIf { it.isNotEmpty() } ?: return null
    val line = attributes["line_range_start"]?.trim()?.toLongOrNull()?.takeIf { it > 0 }
    val destination = path.replace("%", "%25").replace("#", "%23").replace("?", "%3F")
    val href = if (line == null) destination else "$destination#L$line"
    val normalized = path.replace('\\', '/').trimEnd('/')
    val label = normalized.substring(normalized.lastIndexOf('/') + 1).ifEmpty { normalized }.ifEmpty { "File" }
    val escapedLabel = label.replace(Regex("""[\\\[\]*_`<&]"""), """\\$0""")
    val escapedHref =
        href.replace("\\", "%5C").replace("<", "%3C").replace(">", "%3E").replace("\r", "%0D").replace("\n", "%0A")
    return "[$escapedLabel](<$escapedHref>)"
}
