package club.touchtech.s5code.kotlin.data

import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction

/** A thread in one environment; thread ids are only unique within an environment. */
data class ThreadLinkRef(val environmentId: String, val threadId: String)

private const val THREAD_LINK_HREF_PREFIX = "t3-thread://v1/"

/**
 * The id as written, following `parseThreadLinkHref` in
 * `packages/shared/src/threadLinks.ts`. Thread ids can hold percent escapes of
 * their own, so it is not decoded; like the `ThreadId` schema, surrounding
 * whitespace is trimmed and a blank id is no id.
 */
fun parseThreadLinkHref(href: String): String? {
    if (!href.startsWith(THREAD_LINK_HREF_PREFIX)) return null
    return href.substring(THREAD_LINK_HREF_PREFIX.length).trim().takeIf(String::isNotEmpty)
}

/** The percent-decoded id when decoding changes it and still yields a valid id. */
fun percentDecodedThreadLinkId(threadId: String): String? {
    val decoded = decodeUriComponent(threadId) ?: return null
    return if (decoded == threadId) null else decoded.trim().takeIf(String::isNotEmpty)
}

/**
 * `resolveThreadLinkRef`: a link names a thread id, not an environment. The
 * linking environment wins (literal id first, then the decoded one); otherwise a
 * unique match in any other connected environment does, and two or more is
 * ambiguous rather than a guess.
 */
fun resolveThreadLinkRef(source: ThreadLinkRef, refs: List<ThreadLinkRef>): ThreadLinkRef? {
    val ids = listOfNotNull(source.threadId, percentDecodedThreadLinkId(source.threadId))
    for (threadId in ids) {
        refs.firstOrNull { it.environmentId == source.environmentId && it.threadId == threadId }
            ?.let { return it }
    }
    for (threadId in ids) {
        val matches = refs.filter { it.threadId == threadId }
        if (matches.size > 1) return null
        matches.firstOrNull()?.let { return it }
    }
    return null
}

/**
 * JavaScript's `decodeURIComponent`, which [java.net.URLDecoder] is not: that
 * one turns `+` into a space. Null where JS would throw (a malformed escape or
 * invalid UTF-8).
 */
private fun decodeUriComponent(value: String): String? {
    if ('%' !in value) return value
    val out = StringBuilder()
    val bytes = ByteArrayOutputStream()
    val decoder =
        Charsets.UTF_8.newDecoder()
            .onMalformedInput(CodingErrorAction.REPORT)
            .onUnmappableCharacter(CodingErrorAction.REPORT)
    fun flush(): Boolean {
        if (bytes.size() == 0) return true
        val chunk =
            try {
                decoder.decode(ByteBuffer.wrap(bytes.toByteArray())).toString()
            } catch (_: CharacterCodingException) {
                return false
            }
        out.append(chunk)
        bytes.reset()
        return true
    }
    var index = 0
    while (index < value.length) {
        val char = value[index]
        if (char == '%') {
            val byte = value.substring(index + 1, minOf(index + 3, value.length))
                .takeIf { hex -> hex.length == 2 && hex.all { it.digitToIntOrNull(16) != null } }
                ?.toInt(16)
                ?: return null
            bytes.write(byte)
            index += 3
        } else {
            if (!flush()) return null
            out.append(char)
            index += 1
        }
    }
    return if (flush()) out.toString() else null
}
