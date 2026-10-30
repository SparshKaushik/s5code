package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.model.FeedEntry

/**
 * `t3-context://v1/<kind>/<contextId>` inline references, ported from
 * `packages/shared/src/composerContextReferences.ts`. The link carries
 * position and identity only; the payload lives in the message's `context`
 * records, so a reference without a matching record renders "(unavailable)"
 * rather than promising a payload the message does not carry.
 */

private val CONTEXT_LINK =
    Regex("""(!?)\[([^\]\n]{0,512})\]\((t3-context://v1/[^\s)]{1,200})\)""")

private val CONTEXT_HREF =
    Regex("""^t3-context://v1/([a-z][a-z0-9-]{0,39})/([a-z0-9_-]{1,128})$""", RegexOption.IGNORE_CASE)

data class ComposerContextReference(
    /** Leading `!` marks an image form. */
    val image: Boolean,
    val label: String,
    val kind: String,
    val contextId: String,
    val range: IntRange,
)

/** `collectComposerContextReferences`: syntactically valid references in order. */
fun collectComposerContextReferences(text: String): List<ComposerContextReference> =
    CONTEXT_LINK.findAll(text).mapNotNull { match ->
        val href = match.groups[3]?.value ?: return@mapNotNull null
        val parsed = CONTEXT_HREF.find(href) ?: return@mapNotNull null
        ComposerContextReference(
            image = match.groups[1]?.value == "!",
            label = match.groups[2]?.value.orEmpty(),
            kind = parsed.groupValues[1],
            contextId = parsed.groupValues[2],
            range = match.range,
        )
    }.toList()

/** Match the other clients' explicit request to respond to attachment-only messages. */
internal fun composerMessageText(text: String, hasAttachments: Boolean): String {
    val references = collectComposerContextReferences(text)
    val imageOnly = references.isNotEmpty() && references.all { it.kind == "image" } &&
        references.asReversed().fold(text) { remainder, reference ->
            remainder.removeRange(reference.range)
        }.isBlank()
    return if ((text.isBlank() && hasAttachments) || imageOnly) {
        "[User attached one or more files without additional text. Respond using the conversation context and the attached files.]"
    } else text
}

/**
 * `replaceComposerContextReferences` for the plain-text bubble: each
 * reference becomes its label, suffixed "(unavailable)" when the message's
 * context records do not carry the payload — RN renders the same marker on
 * the markdown link it emits. A prompt link passes [markUnavailable] false:
 * it carries labels only, never payloads.
 */
fun replaceComposerContextReferences(
    text: String,
    contextRecords: Map<String, FeedEntry.ContextRecordLabel>,
    markUnavailable: Boolean = true,
): String {
    val occurrences = collectComposerContextReferences(text)
    if (occurrences.isEmpty()) return text
    val result = StringBuilder()
    var cursor = 0
    for (occurrence in occurrences) {
        result.append(text, cursor, occurrence.range.first)
        val available = occurrence.contextId in contextRecords
        result.append(occurrence.label)
        if (!available && markUnavailable) result.append(" (unavailable)")
        cursor = occurrence.range.last + 1
    }
    result.append(text, cursor, text.length)
    return result.toString()
}


/* ── Trigger detection ────────────────────────────────────────────────── */

/**
 * `detectComposerTrigger` from `packages/shared/src/composerTrigger.ts`, with
 * the cursor pinned to the end of the text — the Kotlin field owns caret
 * handling, so the menu only ever asks about the tail.
 */
enum class ComposerTriggerKind { Path, PullRequest, SlashCommand, SlashModel, Skill }

data class ComposerTrigger(
    val kind: ComposerTriggerKind,
    val query: String,
    /** Range the pick replaces: the token plus its trigger character. */
    val rangeStart: Int,
    val rangeEnd: Int,
)

private val PULL_REQUEST_TOKEN =
    Regex("""^#([\p{L}\p{N}][\p{L}\p{N}_-]*)?$""")

private val SLASH_TOKEN = Regex("""^/(\S*)$""")

private val SLASH_MODEL_PREFIX = Regex("""^/model(?:\s+(.*))?$""")

/**
 * The trigger under the end of [text], or null when no token is live. A `/`
 * counts only at the start of its line — mid-word it is a path character — and
 * a `$`-family currency prefix starts a skill token whether or not a query
 * follows it.
 */
internal fun detectComposerTrigger(text: String): ComposerTrigger? {
    val cursor = text.length
    val lineStart = text.lastIndexOf('\n', maxOf(0, cursor - 1)) + 1
    val linePrefix = text.substring(lineStart, cursor)

    if (linePrefix.startsWith("/")) {
        val commandMatch = SLASH_TOKEN.find(linePrefix)
        if (commandMatch != null) {
            val commandQuery = commandMatch.groupValues[1]
            // A second slash makes the word a path such as `/tmp/build.sh`, not a command.
            if (commandQuery.contains('/')) return null
            if (commandQuery.equals("model", ignoreCase = true)) {
                return ComposerTrigger(
                    ComposerTriggerKind.SlashModel, "", lineStart, cursor,
                )
            }
            return ComposerTrigger(
                ComposerTriggerKind.SlashCommand, commandQuery, lineStart, cursor,
            )
        }
        if (SLASH_MODEL_PREFIX.matches(linePrefix)) {
            val query = SLASH_MODEL_PREFIX.find(linePrefix)?.groupValues?.getOrNull(1).orEmpty().trim()
            return ComposerTrigger(ComposerTriggerKind.SlashModel, query, lineStart, cursor)
        }
    }

    val tokenStart = (cursor - 1 downTo 0).firstOrNull { text[it].isWhitespace() }
        ?.plus(1) ?: 0
    val token = text.substring(tokenStart, cursor)

    PULL_REQUEST_TOKEN.matchEntire(token)?.let { match ->
        return ComposerTrigger(
            ComposerTriggerKind.PullRequest,
            match.groupValues[1],
            tokenStart,
            cursor,
        )
    }
    val first = token.firstOrNull()
    if (first != null && first.category == CharCategory.CURRENCY_SYMBOL) {
        if (isCurrencyAmountQuery(token.substring(1))) return null
        return ComposerTrigger(
            ComposerTriggerKind.Skill,
            token.substring(1),
            tokenStart,
            cursor,
        )
    }
    if (!token.startsWith("@")) return null
    return ComposerTrigger(ComposerTriggerKind.Path, token.substring(1), tokenStart, cursor)
}

private val CURRENCY_AMOUNT = Regex("""^\.?[0-9][0-9,._]*(?:[kKmMbBtT]|[eE][0-9]+)?[.,;:!?)]*$""")

/**
 * `isCurrencyAmountQuery` in `packages/shared/src/composerTrigger.ts`: `$5`,
 * `€9.99` and `$10k` are money, not skills, including the punctuation prose
 * puts after one (`$10k.`, `($5)`).
 */
internal fun isCurrencyAmountQuery(query: String): Boolean = CURRENCY_AMOUNT.matches(query)

/** `replaceTextRange`: splice [replacement] over the trigger's range. */
internal fun replaceComposerTextRange(
    text: String,
    rangeStart: Int,
    rangeEnd: Int,
    replacement: String,
): String {
    val safeStart = rangeStart.coerceIn(0, text.length)
    val safeEnd = rangeEnd.coerceIn(safeStart, text.length)
    return text.substring(0, safeStart) + replacement + text.substring(safeEnd)
}
