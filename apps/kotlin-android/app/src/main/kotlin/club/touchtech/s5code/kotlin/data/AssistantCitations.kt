package club.touchtech.s5code.kotlin.data

import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException

/*
 * Assistant quotes, ported from `packages/shared/src/assistantCitations.ts`.
 * Web and desktop insert `[Assistant quote](t3-citation://v1/<env>/<thread>/<message>?text=…)`
 * links into prompts; this client only reads them back as plain text.
 */

private const val CITATION_HREF_PREFIX = "t3-citation://v1/"
private const val ASSISTANT_CITATION_MAX_TEXT_LENGTH = 8_000
private const val ASSISTANT_CITATION_MAX_COMMENT_LENGTH = 8_000
private const val ASSISTANT_CITATION_CONTEXT_LENGTH = 32
private const val ASSISTANT_CITATION_MAX_ID_LENGTH = 512
private const val MAX_SAFE_INTEGER = 9_007_199_254_740_991L

// Percent encoding needs up to nine characters per UTF-16 code unit; 16k covers selectors.
private const val MAX_CITATION_HREF_LENGTH =
    9 * (ASSISTANT_CITATION_MAX_TEXT_LENGTH + ASSISTANT_CITATION_MAX_COMMENT_LENGTH) + 16_000

// JS `\s` covers Unicode spaces that Java's ASCII-only `\s` does not.
private const val JS_WHITESPACE = "\\s\\u00A0\\u1680\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000\\uFEFF"

private val CITATION_LINK =
    Regex(
        "\\[Assistant quote]\\((" + Regex.escape(CITATION_HREF_PREFIX) +
            "[^$JS_WHITESPACE)]{1,${MAX_CITATION_HREF_LENGTH - CITATION_HREF_PREFIX.length}})\\)"
    )

private val CITATION_REQUIRED_KEYS = listOf("text", "start", "end", "prefix", "suffix")
private val CITATION_OFFSET = Regex("^\\d{1,16}$")

/** The quoted text and optional comment of a valid citation href. */
internal data class AssistantCitationText(val text: String, val comment: String?)

/** `assistantCitationsToPlainText`: each valid quote link becomes its text; invalid links stay. */
fun assistantCitationsToPlainText(prompt: String): String =
    CITATION_LINK.replace(prompt) { match ->
        val citation = parseAssistantCitationHref(match.groupValues[1]) ?: return@replace match.value
        if (citation.comment == null) citation.text else "${citation.text}\nComment: ${citation.comment}"
    }

/** `parseAssistantCitationHref`, keeping only what the plain-text form needs. */
internal fun parseAssistantCitationHref(href: String): AssistantCitationText? {
    if (!href.startsWith(CITATION_HREF_PREFIX) || href.length > MAX_CITATION_HREF_LENGTH) return null
    val afterPrefix = href.substring(CITATION_HREF_PREFIX.length)
    val hashIndex = afterPrefix.indexOf('#')
    if (hashIndex >= 0 && hashIndex < afterPrefix.lastIndex) return null
    val withoutHash = if (hashIndex >= 0) afterPrefix.substring(0, hashIndex) else afterPrefix
    val path = withoutHash.substringBefore('?')
    val query = if ('?' in withoutHash) withoutHash.substringAfter('?') else ""

    val parts = path.split('/')
    if (parts.size != 3) return null
    val ids = parts.map { decodeUriComponent(it) ?: return null }
    if (ids.any { it.isBlank() || it != it.trim() || it.length > ASSISTANT_CITATION_MAX_ID_LENGTH }) return null

    // URLSearchParams: `&`-separated, empty segments skipped, `+` is a space.
    val params =
        query.split('&').filter { it.isNotEmpty() }.map { pair ->
            val name = pair.substringBefore('=')
            val value = if ('=' in pair) pair.substringAfter('=') else ""
            decodeQueryComponent(name) to decodeQueryComponent(value)
        }
    fun values(key: String) = params.filter { it.first == key }.map { it.second }
    val comment = values("comment").firstOrNull()
    if (params.size != CITATION_REQUIRED_KEYS.size + (if (comment == null) 0 else 1)) return null
    if (CITATION_REQUIRED_KEYS.any { values(it).size != 1 }) return null

    val text = values("text").single()
    val start = values("start").single()
    val end = values("end").single()
    if (!CITATION_OFFSET.matches(start) || !CITATION_OFFSET.matches(end)) return null
    val startValue = start.toLong()
    val endValue = end.toLong()
    if (startValue > MAX_SAFE_INTEGER || endValue > MAX_SAFE_INTEGER || endValue <= startValue) return null
    if (text.isBlank() || text.length > ASSISTANT_CITATION_MAX_TEXT_LENGTH) return null
    if (comment != null && comment.length > ASSISTANT_CITATION_MAX_COMMENT_LENGTH) return null
    if (values("prefix").single().length > ASSISTANT_CITATION_CONTEXT_LENGTH) return null
    if (values("suffix").single().length > ASSISTANT_CITATION_CONTEXT_LENGTH) return null
    return AssistantCitationText(text, comment)
}

/** `decodeURIComponent`: null on a malformed escape or invalid UTF-8. */
private fun decodeUriComponent(value: String): String? {
    if ('%' !in value) return value
    val bytes = percentDecodedBytes(value) ?: return null
    return try {
        Charsets.UTF_8.newDecoder().decode(ByteBuffer.wrap(bytes)).toString()
    } catch (_: CharacterCodingException) {
        null
    }
}

/** `URLSearchParams` decoding: `+` is a space, a bad escape stays literal, bad bytes become U+FFFD. */
private fun decodeQueryComponent(value: String): String {
    val spaced = value.replace('+', ' ')
    if ('%' !in spaced) return spaced
    return String(percentDecodedBytes(spaced, lenient = true) ?: return spaced, Charsets.UTF_8)
}

private fun percentDecodedBytes(value: String, lenient: Boolean = false): ByteArray? {
    val bytes = ByteArrayOutputStream()
    var index = 0
    while (index < value.length) {
        val char = value[index]
        if (char == '%') {
            val high = value.getOrNull(index + 1)?.let { Character.digit(it, 16) } ?: -1
            val low = value.getOrNull(index + 2)?.let { Character.digit(it, 16) } ?: -1
            if (high >= 0 && low >= 0) {
                bytes.write(high * 16 + low)
                index += 3
                continue
            }
            if (!lenient) return null
        }
        val next = if (Character.isHighSurrogate(char) && index + 1 < value.length) index + 2 else index + 1
        bytes.write(value.substring(index, next).toByteArray(Charsets.UTF_8))
        index = next
    }
    return bytes.toByteArray()
}
