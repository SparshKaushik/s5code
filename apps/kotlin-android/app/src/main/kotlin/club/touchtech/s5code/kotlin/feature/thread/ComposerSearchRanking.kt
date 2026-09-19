package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.model.SlashCommand

/** One bounded fuzzy-search candidate, ordered by score then stable label. */
private data class Ranked<T>(val item: T, val score: Int, val tieBreaker: String)

/**
 * Kotlin port of `packages/shared/src/searchRanking.ts`.
 *
 * Lower scores are better. Exact, prefix, token-boundary, substring, and finally
 * subsequence matches form separate tiers, so a loose fuzzy match can never jump
 * ahead of a direct prefix merely because its source string is shorter.
 */
internal fun scoreQueryMatch(
    value: String,
    query: String,
    exactBase: Int,
    prefixBase: Int? = null,
    boundaryBase: Int? = null,
    includesBase: Int? = null,
    fuzzyBase: Int? = null,
    boundaryMarkers: List<String> = listOf(" ", "-", "_", "/"),
): Int? {
    if (value.isEmpty() || query.isEmpty()) return null
    if (value == query) return exactBase
    if (prefixBase != null && value.startsWith(query)) {
        return prefixBase + lengthPenalty(value, query)
    }
    if (boundaryBase != null) {
        val boundaryIndex =
            boundaryMarkers.mapNotNull { marker ->
                value.indexOf("$marker$query").takeIf { it >= 0 }?.plus(marker.length)
            }.minOrNull()
        if (boundaryIndex != null) {
            return boundaryBase + boundaryIndex * 2 + lengthPenalty(value, query)
        }
    }
    if (includesBase != null) {
        val index = value.indexOf(query)
        if (index >= 0) return includesBase + index * 2 + lengthPenalty(value, query)
    }
    if (fuzzyBase != null) {
        return scoreSubsequenceMatch(value, query)?.plus(fuzzyBase)
    }
    return null
}

internal fun rankSlashCommands(
    commands: List<SlashCommand>,
    rawQuery: String,
    limit: Int = 20,
): List<SlashCommand> {
    val query = rawQuery.trim().removePrefix("/").lowercase()
    if (query.isEmpty()) return commands.take(limit)
    return commands
        .mapNotNull { command ->
            val name = command.name.removePrefix("/").lowercase()
            val description = command.description.lowercase()
            val score =
                listOfNotNull(
                    scoreQueryMatch(
                        value = name,
                        query = query,
                        exactBase = 0,
                        prefixBase = 2,
                        boundaryBase = 4,
                        includesBase = 6,
                        fuzzyBase = 100,
                        boundaryMarkers = listOf("-", "_", "/"),
                    ),
                    scoreQueryMatch(
                        value = description,
                        query = query,
                        exactBase = 20,
                        prefixBase = 22,
                        boundaryBase = 24,
                        includesBase = 26,
                        fuzzyBase = 120,
                    ),
                ).minOrNull() ?: return@mapNotNull null
            Ranked(command, score, "$name\u0000${command.description.lowercase()}")
        }
        .sortedWith(compareBy<Ranked<SlashCommand>> { it.score }.thenBy { it.tieBreaker })
        .take(limit)
        .map { it.item }
}

internal fun rankComposerPaths(
    paths: List<String>,
    rawQuery: String,
    limit: Int = 20,
): List<String> {
    val query = rawQuery.trim().removePrefix("@").lowercase()
    if (query.isEmpty()) return paths.take(limit)
    return paths
        .distinct()
        .mapNotNull { path ->
            val normalized = path.lowercase()
            val leaf = normalized.substringAfterLast('/')
            val score =
                listOfNotNull(
                    scoreQueryMatch(
                        value = leaf,
                        query = query,
                        exactBase = 0,
                        prefixBase = 2,
                        boundaryBase = 4,
                        includesBase = 6,
                        fuzzyBase = 100,
                        boundaryMarkers = listOf("-", "_", "."),
                    ),
                    scoreQueryMatch(
                        value = normalized,
                        query = query,
                        exactBase = 1,
                        prefixBase = 3,
                        boundaryBase = 5,
                        includesBase = 7,
                        fuzzyBase = 110,
                        boundaryMarkers = listOf("/", "-", "_", "."),
                    ),
                ).minOrNull() ?: return@mapNotNull null
            Ranked(path, score, normalized)
        }
        .sortedWith(compareBy<Ranked<String>> { it.score }.thenBy { it.tieBreaker })
        .take(limit)
        .map { it.item }
}

/**
 * Serializes a picked `@` path as a Markdown link, ported from
 * `serializeComposerFileLink` in `packages/shared/src/composerTrigger.ts`.
 *
 * The basename becomes the label and the full path the destination. Providers
 * resolve file references from the link syntax — a bare `@path` token is a
 * client-side convention only, so sending it raw loses the mention.
 */
internal fun serializeComposerFileLink(path: String): String {
    val basename = path.substringAfterLast('/').substringAfterLast('\\')
    val label =
        basename
            .replace("\\", "\\\\")
            .replace("[", "\\[")
            .replace("]", "\\]")
    // `encodeURI` rather than `Uri.encode`, so the function stays testable on
    // the plain JVM. The allow set is encodeURI's: everything else percent-
    // encodes per UTF-8 byte, then the four Markdown-sensitive survivors go.
    val destination =
        encodeUri(path)
            .replace("(", "%28")
            .replace(")", "%29")
            .replace("#", "%23")
            .replace("?", "%3F")
            .replace("\\", "%5C")
    return "[$label]($destination)"
}

/** JS `encodeURI`: these characters survive verbatim; all others percent-encode. */
private val ENCODE_URI_UNESCAPED =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789;,/?:@&=+\$-_.!~*'()#"
        .toSet()

private fun encodeUri(value: String): String = buildString {
    for (character in value) {
        if (character in ENCODE_URI_UNESCAPED) {
            append(character)
        } else {
            for (byte in character.toString().toByteArray(Charsets.UTF_8)) {
                append('%')
                append(String.format("%02X", byte))
            }
        }
    }
}

private fun lengthPenalty(value: String, query: String): Int =
    (value.length - query.length).coerceIn(0, 64)

private fun scoreSubsequenceMatch(value: String, query: String): Int? {
    if (query.isEmpty()) return 0
    var queryIndex = 0
    var firstMatchIndex = -1
    var previousMatchIndex = -1
    var gapPenalty = 0
    value.forEachIndexed { valueIndex, character ->
        if (character != query[queryIndex]) return@forEachIndexed
        if (firstMatchIndex == -1) firstMatchIndex = valueIndex
        if (previousMatchIndex != -1) gapPenalty += valueIndex - previousMatchIndex - 1
        previousMatchIndex = valueIndex
        queryIndex += 1
        if (queryIndex == query.length) {
            val spanPenalty = valueIndex - firstMatchIndex + 1 - query.length
            val lengthPenalty = minOf(64, value.length - query.length)
            return firstMatchIndex * 2 + gapPenalty * 3 + spanPenalty + lengthPenalty
        }
    }
    return null
}
