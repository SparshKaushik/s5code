package club.touchtech.s5code.kotlin.design.text

/**
 * Pure-Kotlin Markdown model. Parsing is deliberately separate from rendering so
 * completed transcript entries can be parsed once, memoized by content
 * fingerprint, and rendered without re-tokenizing on every recomposition.
 *
 * The supported subset matches what the other S5 Code clients render: headings,
 * paragraphs, emphasis, inline/fenced code, links, bullet/ordered lists, block
 * quotes, task list items, thematic rules, and pipe tables.
 */
sealed interface MdBlock {
    data class Heading(val level: Int, val text: List<MdSpan>) : MdBlock

    data class Paragraph(
        val text: List<MdSpan>,
        val images: List<MdImage> = emptyList(),
    ) : MdBlock

    data class Quote(val blocks: List<MdBlock>) : MdBlock

    data class Code(val language: String?, val lines: List<String>) : MdBlock

    data class Bullets(val items: List<MdListItem>, val ordered: Boolean) : MdBlock

    data class Table(
        val header: List<List<MdSpan>>,
        val rows: List<List<List<MdSpan>>>,
        val alignments: List<MdTableAlignment> = List(header.size) { MdTableAlignment.Start },
    ) : MdBlock

    data object Rule : MdBlock
}

/**
 * One list entry. [checked] is non-null only for task list syntax
 * (`- [ ]` / `- [x]`), which the plan/checklist output uses heavily.
 */
data class MdListItem(val text: List<MdSpan>, val checked: Boolean? = null, val marker: String? = null)

enum class MdTableAlignment {
    Start,
    Center,
    End,
}

data class MdImage(val alt: String, val source: String, val title: String? = null)

/** Inline run. Styles compose, so bold links and code inside emphasis both work. */
data class MdSpan(
    val text: String,
    val bold: Boolean = false,
    val italic: Boolean = false,
    val code: Boolean = false,
    val strike: Boolean = false,
    val link: String? = null,
)

/** Parses [source] into blocks. Never throws: unknown syntax degrades to text. */
fun parseMarkdown(source: String): List<MdBlock> {
    val lines = source.replace("\r\n", "\n").split('\n')
    val blocks = mutableListOf<MdBlock>()
    var index = 0

    while (index < lines.size) {
        val raw = lines[index]
        val line = raw.trimEnd()
        val trimmed = line.trimStart()

        when {
            trimmed.isEmpty() -> index++

            trimmed.startsWith("```") || trimmed.startsWith("~~~") -> {
                val fence = trimmed.take(3)
                val language = trimmed.removePrefix(fence).trim().takeIf(String::isNotEmpty)
                val body = mutableListOf<String>()
                index++
                while (index < lines.size && !lines[index].trimStart().startsWith(fence)) {
                    body += lines[index]
                    index++
                }
                if (index < lines.size) index++
                blocks += MdBlock.Code(language, body.trimBlankEdges())
            }

            isRule(trimmed) -> {
                blocks += MdBlock.Rule
                index++
            }

            headingLevel(trimmed) > 0 -> {
                val level = headingLevel(trimmed)
                blocks += MdBlock.Heading(level, parseInline(trimmed.drop(level).trim()))
                index++
            }

            trimmed.startsWith("> ") || trimmed == ">" -> {
                val body = mutableListOf<String>()
                while (index < lines.size) {
                    val candidate = lines[index].trimStart()
                    if (!candidate.startsWith(">")) break
                    body += candidate.removePrefix(">").removePrefix(" ")
                    index++
                }
                blocks += MdBlock.Quote(parseMarkdown(body.joinToString("\n")))
            }

            tableDelimiter(lines.getOrNull(index + 1)) != null && hasUnescapedPipe(trimmed) -> {
                val delimiter = checkNotNull(tableDelimiter(lines.getOrNull(index + 1)))
                val header = splitTableRow(trimmed)
                if (header.size == delimiter.size && header.size >= 2) {
                    index += 2
                    val rows = mutableListOf<List<List<MdSpan>>>()
                    while (index < lines.size) {
                        val candidate = lines[index].trim()
                        if (candidate.isEmpty() || !hasUnescapedPipe(candidate)) break
                        val cells = splitTableRow(candidate)
                        rows +=
                            List(header.size) { cellIndex ->
                                parseInline(cells.getOrElse(cellIndex) { "" })
                            }
                        index++
                    }
                    blocks +=
                        MdBlock.Table(
                            header = header.map(::parseInline),
                            rows = rows,
                            alignments = delimiter,
                        )
                } else {
                    blocks += MdBlock.Paragraph(parseInline(trimmed))
                    index++
                }
            }

            bulletMarker(trimmed) != null || orderedMarker(trimmed) != null -> {
                val ordered = bulletMarker(trimmed) == null
                val items = mutableListOf<MdListItem>()
                while (index < lines.size) {
                    val candidate = lines[index].trimStart()
                    val marker =
                        if (ordered) orderedMarker(candidate) else bulletMarker(candidate)?.let { "•" }
                    if (marker == null) break
                    val content =
                        if (ordered) {
                            candidate.substringAfter(' ', "").trim()
                        } else {
                            candidate.drop(2).trim()
                        }
                    val (checked, text) = splitTaskMarker(content)
                    items += MdListItem(parseInline(text), checked, marker)
                    index++
                }
                blocks += MdBlock.Bullets(items, ordered)
            }

            else -> {
                val body = mutableListOf(trimmed)
                index++
                while (index < lines.size) {
                    val candidate = lines[index].trimEnd()
                    val next = candidate.trimStart()
                    if (
                        next.isEmpty() ||
                            isRule(next) ||
                            headingLevel(next) > 0 ||
                            next.startsWith(">") ||
                            next.startsWith("```") ||
                            next.startsWith("~~~") ||
                            bulletMarker(next) != null ||
                            orderedMarker(next) != null ||
                            (tableDelimiter(lines.getOrNull(index + 1)) != null && hasUnescapedPipe(next))
                    ) {
                        break
                    }
                    body += next
                    index++
                }
                val paragraph = parseParagraph(body.joinToString(" "))
                blocks += MdBlock.Paragraph(paragraph.spans, paragraph.images)
            }
        }
    }
    return blocks
}

private data class ParsedParagraph(val spans: List<MdSpan>, val images: List<MdImage>)

private fun parseParagraph(source: String): ParsedParagraph {
    val images = mutableListOf<MdImage>()
    val text = StringBuilder()
    var cursor = 0
    val pattern = Regex("!\\[([^]]*)]\\((\\S+?)(?:\\s+[\"']([^\"']*)[\"'])?\\)")
    pattern.findAll(source).forEach { match ->
        text.append(source.substring(cursor, match.range.first))
        images +=
            MdImage(
                alt = match.groupValues[1],
                source = match.groupValues[2].removeSurrounding("<", ">"),
                title = match.groupValues[3].takeIf(String::isNotEmpty),
            )
        cursor = match.range.last + 1
    }
    text.append(source.substring(cursor))
    return ParsedParagraph(parseInline(text.toString().trim()), images)
}

/** Parses inline runs. Unclosed markers stay literal rather than eating the rest. */
fun parseInline(source: String): List<MdSpan> {
    val spans = mutableListOf<MdSpan>()
    val buffer = StringBuilder()
    var bold = false
    var italic = false
    var strike = false
    var index = 0

    fun flush() {
        if (buffer.isNotEmpty()) {
            spans += MdSpan(buffer.toString(), bold = bold, italic = italic, strike = strike)
            buffer.clear()
        }
    }

    while (index < source.length) {
        val rest = source.substring(index)
        when {
            rest.startsWith("`") -> {
                val end = rest.indexOf('`', startIndex = 1)
                if (end <= 0) {
                    buffer.append('`')
                    index++
                } else {
                    flush()
                    spans += MdSpan(rest.substring(1, end), code = true, bold = bold, italic = italic)
                    index += end + 1
                }
            }

            rest.startsWith("[") -> {
                val closeText = rest.indexOf(']')
                val openHref = if (closeText >= 0) rest.indexOf('(', startIndex = closeText) else -1
                val closeHref = if (openHref >= 0) rest.indexOf(')', startIndex = openHref) else -1
                if (closeText <= 0 || openHref != closeText + 1 || closeHref < 0) {
                    buffer.append('[')
                    index++
                } else {
                    flush()
                    val label = rest.substring(1, closeText)
                    val href = rest.substring(openHref + 1, closeHref)
                    parseInline(label).forEach { span -> spans += span.copy(link = href) }
                    index += closeHref + 1
                }
            }

            rest.startsWith("**") || rest.startsWith("__") -> {
                val marker = rest.take(2)
                if (!bold && !rest.drop(2).contains(marker)) {
                    buffer.append(marker)
                    index += 2
                } else {
                    flush()
                    bold = !bold
                    index += 2
                }
            }

            rest.startsWith("~~") -> {
                if (!strike && !rest.drop(2).contains("~~")) {
                    buffer.append("~~")
                    index += 2
                } else {
                    flush()
                    strike = !strike
                    index += 2
                }
            }

            (rest.startsWith("*") || rest.startsWith("_")) && !rest.startsWith("**") -> {
                val marker = rest.first()
                if (!italic && !rest.drop(1).contains(marker)) {
                    buffer.append(marker)
                    index++
                } else {
                    flush()
                    italic = !italic
                    index++
                }
            }

            else -> {
                buffer.append(source[index])
                index++
            }
        }
    }
    flush()
    return spans
}

private fun headingLevel(line: String): Int {
    val hashes = line.takeWhile { it == '#' }.length
    return if (hashes in 1..6 && line.length > hashes && line[hashes] == ' ') hashes else 0
}

private fun isRule(line: String): Boolean =
    line.length >= 3 && (line.all { it == '-' } || line.all { it == '*' } || line.all { it == '_' })

private fun bulletMarker(line: String): Char? =
    line.firstOrNull()?.takeIf { (it == '-' || it == '*' || it == '+') && line.length > 1 && line[1] == ' ' }

private fun orderedMarker(line: String): String? {
    val digits = line.takeWhile(Char::isDigit)
    if (digits.isEmpty() || digits.length > 3) return null
    val after = line.drop(digits.length)
    return if ((after.startsWith(". ") || after.startsWith(") "))) "$digits." else null
}

private fun splitTaskMarker(content: String): Pair<Boolean?, String> =
    when {
        content.startsWith("[ ] ") -> false to content.removePrefix("[ ] ")
        content.startsWith("[x] ", ignoreCase = true) -> true to content.drop(4)
        else -> null to content
    }

private fun tableDelimiter(line: String?): List<MdTableAlignment>? {
    val cells = splitTableRow(line?.trim().orEmpty())
    if (cells.size < 2) return null
    return cells.map { cell ->
        if (!Regex("^:?-{3,}:?$").matches(cell.replace(" ", ""))) return null
        when {
            cell.trim().startsWith(':') && cell.trim().endsWith(':') -> MdTableAlignment.Center
            cell.trim().endsWith(':') -> MdTableAlignment.End
            else -> MdTableAlignment.Start
        }
    }
}

/** Splits only pipes outside code spans; `\\|` stays literal inside a cell. */
private fun splitTableRow(line: String): List<String> {
    val normalized = line.trim().removePrefix("|").removeSuffix("|")
    val cells = mutableListOf<String>()
    val cell = StringBuilder()
    var escaped = false
    var inCode = false
    normalized.forEach { char ->
        when {
            escaped -> {
                cell.append(char)
                escaped = false
            }
            char == '\\' -> escaped = true
            char == '`' -> {
                inCode = !inCode
                cell.append(char)
            }
            char == '|' && !inCode -> {
                cells += cell.toString().trim()
                cell.clear()
            }
            else -> cell.append(char)
        }
    }
    if (escaped) cell.append('\\')
    cells += cell.toString().trim()
    return cells
}

private fun hasUnescapedPipe(line: String): Boolean {
    var escaped = false
    var inCode = false
    line.forEach { char ->
        when {
            escaped -> escaped = false
            char == '\\' -> escaped = true
            char == '`' -> inCode = !inCode
            char == '|' && !inCode -> return true
        }
    }
    return false
}

private fun List<String>.trimBlankEdges(): List<String> {
    var start = 0
    var end = size
    while (start < end && this[start].isBlank()) start++
    while (end > start && this[end - 1].isBlank()) end--
    return subList(start, end).toList()
}
