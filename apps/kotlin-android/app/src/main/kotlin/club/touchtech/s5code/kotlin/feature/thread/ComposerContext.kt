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

/**
 * `replaceComposerContextReferences` for the plain-text bubble: each
 * reference becomes its label, suffixed "(unavailable)" when the message's
 * context records do not carry the payload — RN renders the same marker on
 * the markdown link it emits.
 */
fun replaceComposerContextReferences(
    text: String,
    contextRecords: Map<String, FeedEntry.ContextRecordLabel>,
): String {
    val occurrences = collectComposerContextReferences(text)
    if (occurrences.isEmpty()) return text
    val result = StringBuilder()
    var cursor = 0
    for (occurrence in occurrences) {
        result.append(text, cursor, occurrence.range.first)
        val available = occurrence.contextId in contextRecords
        result.append(occurrence.label)
        if (!available) result.append(" (unavailable)")
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

/* ── Commands that own their turn ─────────────────────────────────────── */

private val GOAL_COMMAND = Regex("^/goal(?:\\s|$)")

/**
 * The server's `isGoalCommand`: a native `/goal` changes the provider's goal,
 * so it is never a steer of the running turn and never a message carrying
 * files — the orchestrator rejects `steer_active` for it outright. Callers
 * force a queued (separate-turn) dispatch instead of failing the send.
 */
internal fun isGoalCommand(text: String, hasAttachments: Boolean): Boolean =
    !hasAttachments && GOAL_COMMAND.containsMatchIn(text.trim())
