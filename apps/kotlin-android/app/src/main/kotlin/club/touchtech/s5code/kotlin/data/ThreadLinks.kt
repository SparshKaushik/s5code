package club.touchtech.s5code.kotlin.data

import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction

/** A thread in one environment; thread ids are only unique within an environment. */
data class ThreadLinkRef(val environmentId: String, val threadId: String)

private const val V1_PREFIX = "t3-thread://v1/"
private const val V2_PREFIX = "t3-thread://v2/"

/** A parsed thread link. A v1 link names no environment; the message's own, or a unique match, stands in. */
data class ThreadLink(val environmentId: String?, val threadId: String)

/**
 * `parseThreadLinkHref` in `packages/shared/src/threadLinks.ts`: the ids as
 * written. Thread ids can hold percent escapes of their own, so they are not
 * decoded; like the branded id schemas, surrounding whitespace is trimmed and a
 * blank id is no id.
 */
fun parseThreadLink(href: String): ThreadLink? {
    if (href.startsWith(V1_PREFIX)) {
        return href.substring(V1_PREFIX.length).trim().takeIf(String::isNotEmpty)?.let { ThreadLink(null, it) }
    }
    if (!href.startsWith(V2_PREFIX)) return null
    val path = href.substring(V2_PREFIX.length)
    val slash = path.indexOf('/').takeIf { it >= 0 } ?: return null
    val environmentId = path.substring(0, slash).trim().takeIf(String::isNotEmpty) ?: return null
    val threadId = path.substring(slash + 1).trim().takeIf(String::isNotEmpty) ?: return null
    return ThreadLink(environmentId, threadId)
}

/** The thread id a v1 or v2 link names, as written. */
fun parseThreadLinkHref(href: String): String? = parseThreadLink(href)?.threadId

/** The percent-decoded id when decoding changes it and still yields a valid id. */
fun percentDecodedThreadLinkId(threadId: String): String? {
    val decoded = decodeUriComponent(threadId) ?: return null
    return if (decoded == threadId) null else decoded.trim().takeIf(String::isNotEmpty)
}

/** `threadLinkCandidates`: in the link's own environment or else the message's, as written then decoded. */
fun threadLinkCandidates(link: ThreadLink, messageEnvironmentId: String): List<ThreadLinkRef> {
    val environmentId = link.environmentId ?: messageEnvironmentId
    return listOfNotNull(link.threadId, percentDecodedThreadLinkId(link.threadId))
        .map { ThreadLinkRef(environmentId, it) }
}

/**
 * `guessThreadLinkRef`: a v1 link its message's environment lacks opens a
 * unique match elsewhere, and two or more is ambiguous rather than a guess. A
 * v2 link names its environment, so it never guesses.
 */
fun guessThreadLinkRef(link: ThreadLink, refs: List<ThreadLinkRef>): ThreadLinkRef? {
    if (link.environmentId != null) return null
    for (threadId in listOfNotNull(link.threadId, percentDecodedThreadLinkId(link.threadId))) {
        val matches = refs.filter { it.threadId == threadId }
        if (matches.size > 1) return null
        matches.firstOrNull()?.let { return it }
    }
    return null
}

/** The thread a tapped link opens: a known candidate, else a v1 guess, else null. */
fun resolveThreadLink(link: ThreadLink, messageEnvironmentId: String, refs: List<ThreadLinkRef>): ThreadLinkRef? =
    threadLinkCandidates(link, messageEnvironmentId).firstOrNull { it in refs } ?: guessThreadLinkRef(link, refs)

/**
 * `resolveThreadLinkRef` for a link known only by its thread id: the linking
 * environment wins (literal id first, then the decoded one); otherwise a unique
 * match in another connected environment does.
 */
fun resolveThreadLinkRef(source: ThreadLinkRef, refs: List<ThreadLinkRef>): ThreadLinkRef? =
    resolveThreadLink(ThreadLink(null, source.threadId), source.environmentId, refs)

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
