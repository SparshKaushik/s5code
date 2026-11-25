package club.touchtech.s5code.kotlin.design.text

/*
 * HTML inside Markdown, as forges write it into pull request bodies and bot
 * comments, after mobile's `markdownWithHtml` / `markdownHtmlContent` in
 * `markdownDocument.ts`. Tags become the Markdown the parser already renders;
 * anything else is dropped so the text reads cleanly. Fenced code and code
 * spans keep their literal tags.
 */

// A tag name must end at whitespace, `/` or `>`, so `<https://…>` and
// `<user@host>` autolinks are not tags.
private val HTML_TAG = Regex("""<!--[\s\S]*?-->|</?[a-zA-Z][a-zA-Z0-9-]*(?=[\s/>])(?:[^>"']|"[^"]*"|'[^']*')*>""")
private val ATTRIBUTE = Regex("""([\w:-]+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))""")
private val ANCHOR_TAG = Regex("""^</?a[\s/>]""", RegexOption.IGNORE_CASE)
private val SAFE_HREF = Regex("^(?:https?|mailto):", RegexOption.IGNORE_CASE)
private val FENCE = Regex("^\\s*(```|~~~)")
private val IMG_TAG = Regex("^<img[\\s/>]", RegexOption.IGNORE_CASE)
private val SUMMARY_OPEN = Regex("^<summary\\b", RegexOption.IGNORE_CASE)
private val SUMMARY_CLOSE = Regex("^</summary\\s*>", RegexOption.IGNORE_CASE)
private val BREAK_TAG = Regex("^<br\\b", RegexOption.IGNORE_CASE)
private val BLOCK_TAG = Regex("^</?(?:p|div|details)\\b", RegexOption.IGNORE_CASE)
private val ALT_ESCAPE = Regex("[\\\\\\[\\]]")
private val ENTITY = Regex("&(#[0-9]+|#[xX][0-9a-fA-F]+|amp|lt|gt|quot|apos|nbsp);")

private fun decodeEntities(value: String): String =
    ENTITY.replace(value) { match ->
        when (val name = match.groupValues[1]) {
            "amp" -> "&"
            "lt" -> "<"
            "gt" -> ">"
            "quot" -> "\""
            "apos" -> "'"
            "nbsp" -> " "
            else -> {
                val code = if (name[1] == 'x' || name[1] == 'X') name.drop(2).toIntOrNull(16) else name.drop(1).toIntOrNull()
                code?.takeIf(Character::isValidCodePoint)?.let { String(Character.toChars(it)) } ?: match.value
            }
        }
    }

private fun attribute(tag: String, name: String): String? =
    ATTRIBUTE.findAll(tag).firstOrNull { it.groupValues[1].equals(name, ignoreCase = true) }?.let { match ->
        decodeEntities(match.groups[2]?.value ?: match.groups[3]?.value ?: match.groups[4]?.value.orEmpty())
    }

// `<…>` destinations run verbatim to `>`; a pipe would split a table row, and a
// space would end the paragraph parser's image source.
private fun destination(url: String): String =
    "<" + url.replace("<", "%3C").replace(">", "%3E").replace("|", "%7C").replace(" ", "%20").replace("(", "%28").replace(")", "%29").replace("\n", "") + ">"

/**
 * `anchorMarkdown`: pairs `<a>` tags in order. Only a closed anchor with an
 * http(s) or mailto href becomes a link; any other keeps its text unlinked.
 */
private fun anchorReplacements(tags: List<String>): ArrayDeque<String> {
    val replacements = MutableList(tags.size) { "" }
    val open = ArrayDeque<Pair<Int, String?>>()
    tags.forEachIndexed { index, tag ->
        if (tag[1] != '/') {
            open.addLast(index to attribute(tag, "href")?.trim()?.takeIf(SAFE_HREF::containsMatchIn))
            return@forEachIndexed
        }
        val (openingIndex, href) = open.removeLastOrNull() ?: return@forEachIndexed
        href ?: return@forEachIndexed
        replacements[openingIndex] = "["
        replacements[index] = "](${destination(href)})"
    }
    return ArrayDeque(replacements)
}

/** Code span ranges of [text], whose tags stay literal. */
private fun codeSpans(text: String): List<IntRange> {
    val ranges = mutableListOf<IntRange>()
    var index = 0
    while (index < text.length) {
        if (text[index] != '`') {
            index++
            continue
        }
        var run = 0
        while (index + run < text.length && text[index + run] == '`') run++
        val marker = "`".repeat(run)
        val close = text.indexOf(marker, index + run)
        if (close < 0) {
            index += run
            continue
        }
        ranges += index until close + run
        index = close + run
    }
    return ranges
}

fun markdownWithHtml(markdown: String): String {
    if (!markdown.contains('<')) return markdown
    val output = StringBuilder()
    val prose = StringBuilder()
    var fence: String? = null
    fun flushProse() {
        if (prose.isNotEmpty()) output.append(convertHtml(prose.toString()))
        prose.clear()
    }
    markdown.split('\n').forEachIndexed { index, line ->
        val separator = if (index == 0) "" else "\n"
        val marker = FENCE.find(line)?.groupValues?.get(1)
        if (fence != null || marker != null) {
            flushProse()
            output.append(separator).append(line)
            fence = if (fence == null) marker else if (marker == fence) null else fence
        } else {
            prose.append(separator).append(line)
        }
    }
    flushProse()
    return output.toString()
}

/**
 * Whether the line starting at [lineStart] is a table row, list item or quote,
 * where a paragraph break would split the row or end the item.
 */
private fun inContainerLine(content: String, lineStart: Int): Boolean {
    var index = lineStart
    while (index < content.length && (content[index] == ' ' || content[index] == '\t')) index++
    val first = content.getOrNull(index) ?: return false
    if (first == '|' || first == '>') return true
    if (first in "-*+") return content.getOrNull(index + 1).let { it == ' ' || it == '\t' }
    var digits = index
    while (digits < content.length && content[digits].isDigit()) digits++
    return digits > index && content.getOrNull(digits).let { it == '.' || it == ')' }
}

private fun convertHtml(content: String): String {
    val code = codeSpans(content)
    val tags =
        HTML_TAG.findAll(content)
            .filter { tag -> code.none { tag.range.first in it } }
            // `[x](<url>)` is a link destination, not a tag.
            .filterNot { tag -> tag.range.first >= 2 && content.startsWith("](", tag.range.first - 2) }
            .toList()
    if (tags.isEmpty()) return content
    val anchors = anchorReplacements(tags.map { it.value }.filter(ANCHOR_TAG::containsMatchIn))
    var openLinks = 0
    val result = StringBuilder()
    var end = 0
    tags.forEach { match ->
        result.append(content, end, match.range.first)
        val tag = match.value
        val lineStart = content.lastIndexOf('\n', match.range.first - 1) + 1
        // A paragraph break inside an open link label would end it before its `]`.
        val blockBreak = if (openLinks == 0 && !inContainerLine(content, lineStart)) "\n\n" else " "
        val replacement =
            when {
                tag.startsWith("<!--") -> ""
                ANCHOR_TAG.containsMatchIn(tag) ->
                    anchors.removeFirstOrNull().orEmpty().also {
                        if (it == "[") openLinks++ else if (it.isNotEmpty()) openLinks--
                    }
                IMG_TAG.containsMatchIn(tag) ->
                    attribute(tag, "src")?.takeIf(String::isNotBlank)?.let { src ->
                        val alt = attribute(tag, "alt").orEmpty().replace(ALT_ESCAPE, "\\\\$0")
                        "![$alt](${destination(src)})"
                    }.orEmpty()
                SUMMARY_OPEN.containsMatchIn(tag) -> "$blockBreak**"
                SUMMARY_CLOSE.containsMatchIn(tag) -> "**$blockBreak"
                // This parser joins a paragraph's lines, so a break is a new paragraph.
                BREAK_TAG.containsMatchIn(tag) -> blockBreak
                BLOCK_TAG.containsMatchIn(tag) -> blockBreak
                else -> ""
            }
        result.append(replacement)
        end = match.range.last + 1
    }
    result.append(content, end, content.length)
    return result.toString()
}
