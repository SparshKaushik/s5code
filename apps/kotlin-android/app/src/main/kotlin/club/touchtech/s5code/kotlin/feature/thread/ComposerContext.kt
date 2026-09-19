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
