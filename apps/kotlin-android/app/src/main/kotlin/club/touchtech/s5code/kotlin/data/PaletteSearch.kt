package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ThreadOpenPick
import club.touchtech.s5code.kotlin.model.ThreadOpenStats
import java.text.Normalizer
import java.util.Locale
import kotlin.math.abs
import kotlin.math.ln
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow

/**
 * Port of `packages/shared/src/paletteSearch.ts`: the one ranking the web and
 * RN palettes, the RN thread list, and settings search share, so a query finds
 * the same thread first on every surface. Whole words and word prefixes rank
 * above typos and mid-word hits; recent use and opens reorder matches of one
 * quality. Offsets are UTF-16 units, as in the TypeScript original, so ranges
 * index straight into the source text.
 */

data class SearchRange(val start: Int, val end: Int)

private class SearchField(
    /** Case- and accent-folded text, one UTF-16 unit per source unit. */
    val text: String,
    /** True where a word starts in [text]. */
    val wordStarts: BooleanArray,
)

private fun isNumberCodePoint(codePoint: Int): Boolean =
    when (Character.getType(codePoint)) {
        Character.DECIMAL_DIGIT_NUMBER.toInt(),
        Character.LETTER_NUMBER.toInt(),
        Character.OTHER_NUMBER.toInt() -> true
        else -> false
    }

private fun isAlphanumeric(codePoint: Int): Boolean =
    Character.isLetter(codePoint) || isNumberCodePoint(codePoint)

private fun isAlphanumeric(character: Char): Boolean = isAlphanumeric(character.code)

/** `\p{So}`: emoji and other symbols can be searched for, so they start words too. */
private fun isSymbol(codePoint: Int): Boolean =
    Character.getType(codePoint) == Character.OTHER_SYMBOL.toInt()

private fun isLowercase(codePoint: Int): Boolean =
    Character.getType(codePoint) == Character.LOWERCASE_LETTER.toInt()

private fun isUppercase(codePoint: Int): Boolean =
    Character.getType(codePoint) == Character.UPPERCASE_LETTER.toInt()

private fun isMark(codePoint: Int): Boolean =
    when (Character.getType(codePoint)) {
        Character.NON_SPACING_MARK.toInt(),
        Character.ENCLOSING_MARK.toInt(),
        Character.COMBINING_SPACING_MARK.toInt() -> true
        else -> false
    }

private fun isApostrophe(codePoint: Int): Boolean = codePoint == '\''.code || codePoint == '’'.code

private fun containsNumber(text: String): Boolean {
    var index = 0
    while (index < text.length) {
        val codePoint = text.codePointAt(index)
        if (isNumberCodePoint(codePoint)) return true
        index += Character.charCount(codePoint)
    }
    return false
}

private fun containsLetter(text: String): Boolean {
    var index = 0
    while (index < text.length) {
        val codePoint = text.codePointAt(index)
        if (Character.isLetter(codePoint)) return true
        index += Character.charCount(codePoint)
    }
    return false
}

// Thread titles, branches, and project names repeat across keystrokes; folding
// them once keeps a keystroke to plain string scans. Oldest first out, as the
// TypeScript cache does: a read does not re-insert.
private const val FIELD_CACHE_LIMIT = 100_000
private val fieldCache = LinkedHashMap<String, SearchField>()

private fun foldCharacter(character: String): String {
    // A typed straight apostrophe finds a curly one: phone keyboards type "’".
    val decomposed = Normalizer.normalize(character, Normalizer.Form.NFKD)
    val stripped = buildString {
        var index = 0
        while (index < decomposed.length) {
            val codePoint = decomposed.codePointAt(index)
            if (!isMark(codePoint)) appendCodePoint(codePoint)
            index += Character.charCount(codePoint)
        }
    }
    return stripped.lowercase(Locale.ROOT).replaceFirst("ς", "σ").replaceFirst(Regex("[‘’]"), "'")
}

/**
 * [text] with case and accents folded one UTF-16 unit at a time, so offsets and
 * word starts still read from [text]. A letter that folds to more or less than
 * one unit ("ß", "ﬁ") stays as it is.
 */
private fun foldInPlace(text: String): String =
    buildString(text.length) {
        for (character in text) {
            if (character.code < 0x80 || character.isSurrogate()) {
                append(character)
            } else {
                val folded = foldCharacter(character.toString())
                append(if (folded.length == 1) folded else character.toString())
            }
        }
    }.lowercase(Locale.ROOT)

/** Chinese and Japanese text has no spaces, so each ideograph or kana starts a word. */
private fun isCjk(codePoint: Int): Boolean =
    codePoint in 0x3040..0x30ff ||
        codePoint in 0x3400..0x4dbf ||
        codePoint in 0x4e00..0x9fff ||
        codePoint in 0xf900..0xfaff

/** Two Chinese, Japanese, or Korean characters already make a common word. */
private fun hasCjkWord(token: String): Boolean =
    token.codePoints().anyMatch { isCjk(it) || it in 0xac00..0xd7af }

private const val NONE = -1

/**
 * Whether [character] starts a word after [previous] and [beforePrevious]
 * ([NONE] at the start): after a non-letter, at a camelCase or digit boundary,
 * or at any Chinese or Japanese character.
 */
private fun isWordStart(beforePrevious: Int, previous: Int, character: Int, next: Int): Boolean {
    // "Julius’s": a possessive or contraction does not start a word.
    val afterApostrophe =
        previous != NONE && isApostrophe(previous) && beforePrevious != NONE && isAlphanumeric(beforePrevious)
    if (!(isAlphanumeric(character) || isSymbol(character)) || afterApostrophe) return false
    if (previous == NONE || !isAlphanumeric(previous)) return true
    if (isLowercase(previous) && isUppercase(character)) return true
    // The last capital of an acronym starts the next word: "JSONSchema".
    if (isUppercase(previous) && isUppercase(character) && next != NONE && isLowercase(next)) return true
    if (isNumberCodePoint(previous) != isNumberCodePoint(character)) return true
    return isCjk(character)
}

private fun foldSearchField(source: String): SearchField {
    val wordStarts = BooleanArray(source.length)
    val codePoints = source.codePoints().toArray()
    var index = 0
    for (position in codePoints.indices) {
        val character = codePoints[position]
        if (
            isWordStart(
                codePoints.getOrElse(position - 2) { NONE },
                codePoints.getOrElse(position - 1) { NONE },
                character,
                codePoints.getOrElse(position + 1) { NONE },
            )
        ) {
            wordStarts[index] = true
        }
        index += Character.charCount(character)
    }
    val folded = foldInPlace(source)
    // A fold that changed the length (rare lowercase expansions) keeps offsets
    // for the part that still lines up; the TypeScript original has the same edge.
    return SearchField(folded, if (folded.length == source.length) wordStarts else wordStarts.copyOf(folded.length))
}

private fun prepareSearchField(source: String): SearchField =
    synchronized(fieldCache) {
        fieldCache[source]?.let { return it }
        if (fieldCache.size >= FIELD_CACHE_LIMIT) {
            fieldCache.remove(fieldCache.keys.first())
        }
        foldSearchField(source).also { fieldCache[source] = it }
    }

// Quotes around a pasted query: "fix sidebar".
private val WRAPPING_QUOTES = Regex("^\\s*[\"'`“”‘’]+|[\"'`“”‘’]+\\s*$")
private val WHITESPACE = Regex("\\s+")

/** Folded, whitespace-collapsed query; empty when there is nothing to match. */
fun foldSearchQuery(query: String): String =
    foldInPlace(Normalizer.normalize(query.replace(WRAPPING_QUOTES, ""), Normalizer.Form.NFC))
        .split(WHITESPACE)
        .filter(String::isNotEmpty)
        .joinToString(" ")

private fun wordEnd(field: SearchField, index: Int): Int {
    var end = index + 1
    while (end < field.text.length && !field.wordStarts[end] && isAlphanumeric(field.text[end])) end++
    return end
}

/** One substitution, insertion, deletion, or swap of neighbours; with [swapOnly], only a swap. */
private fun withinOneEdit(left: String, right: String, swapOnly: Boolean): Boolean {
    if (abs(left.length - right.length) > (if (swapOnly) 0 else 1)) return false
    var index = 0
    while (index < left.length && index < right.length && left[index] == right[index]) index++
    val restLeft = left.substring(index)
    val restRight = right.substring(index)
    val swapped =
        restLeft.length > 1 &&
            restRight.length > 1 &&
            restLeft[0] == restRight[1] &&
            restLeft[1] == restRight[0] &&
            restLeft.substring(2) == restRight.substring(2)
    if (swapOnly) return swapped
    return swapped ||
        restLeft == restRight ||
        restLeft.drop(1) == restRight.drop(1) ||
        restLeft.drop(1) == restRight ||
        restLeft == restRight.drop(1)
}

private const val WHOLE_WORD = 1.0
// The bonus for a field equal to the whole query lifts a whole word to this.
private const val WHOLE_NAME = 1.5
private const val MID_WORD = 0.3
private const val TYPO = 0.45
// Whole words typed together are exact, so they rank with typos and above
// mid-word hits, though below a match at a word start.
private const val JOINED_WORDS = 0.6

private class TokenMatch(
    val score: Double,
    val start: Int,
    val end: Int,
    /** Separate ranges, for words typed together. */
    val letters: List<SearchRange>? = null,
)

/**
 * `matchToken`: mid-word hits need four or more characters and [midWord], so
 * "del" does not find "model". Typos keep the first letter except for swaps,
 * and numbers never match with a typo so pull request numbers stay distinct.
 */
private fun matchToken(
    field: SearchField,
    token: String,
    typos: Boolean,
    isPrimary: Boolean,
    midWord: Boolean,
): TokenMatch? {
    if (token.isEmpty()) return null
    var best: TokenMatch? = null
    val ownBoundary = !isAlphanumeric(token[0]) && !isSymbol(token[0].code)
    var index = field.text.indexOf(token)
    while (index != -1) {
        // A token that opens with punctuation (".ts") brings its own boundary.
        if (field.wordStarts[index] || ownBoundary) {
            // Whole when the match ends where a word ends: "t3c" is a prefix of
            // "t3code", ".ts" whole in "index.ts" but a prefix in "parser.tsx".
            val end = index + token.length
            val whole = end >= field.text.length || field.wordStarts[end] || !isAlphanumeric(field.text[end])
            var runEnd = end
            while (runEnd < field.text.length && isAlphanumeric(field.text[runEnd])) runEnd++
            val score = if (whole) WHOLE_WORD else 0.7 + (0.2 * token.length) / (runEnd - index)
            if (best == null || score > best.score) best = TokenMatch(score, index, index + token.length)
            if (score == WHOLE_WORD) return best
        } else if (midWord && token.length >= 4 && best == null) {
            best = TokenMatch(MID_WORD, index, index + token.length)
        }
        index = field.text.indexOf(token, index + 1)
    }
    // Joined words are exact, so they count without the typo pass: a strong
    // match elsewhere must not hide "VS Code" for "vscode".
    if ((best == null || best.score <= MID_WORD) && isPrimary) {
        matchJoinedWords(field, token)?.let { return it }
    }
    if ((best != null && best.score > MID_WORD) || !typos || token.length < 3) return best
    // Numbers skip typos to keep pull requests distinct. Words with digits ("t3code") do not.
    if (containsLetter(token)) {
        for (start in field.text.indices) {
            if (!field.wordStarts[start]) continue
            // People rarely mistype the first letter, so a typo keeps it, unless it
            // swaps the first two of a longer word ("ing" is not "nig…").
            val first = field.text[start]
            val firstLetterKept =
                first == token[0] ||
                    (token.length > 3 && first == token[1] && field.text.getOrNull(start + 1) == token[0])
            if (!firstLetterKept) continue
            // The whole run, across camelCase joins: "githbu" is "GitHub".
            var runEnd = start + 1
            while (runEnd < field.text.length && isAlphanumeric(field.text[runEnd])) runEnd++
            val word = field.text.substring(start, runEnd)
            // Short tokens forgive a swapped pair or one missing letter ("srv" for
            // "serv…"); any other edit of a three- or four-letter word matches too much.
            val short = token.length <= 4
            val lengths =
                if (short) listOf(token.length, token.length + 1)
                else listOf(token.length, token.length - 1, token.length + 1)
            for (length in lengths) {
                if (length > word.length) continue
                if (withinOneEdit(token, word.substring(0, length), short && length == token.length)) {
                    return TokenMatch(TYPO, start, start + length)
                }
            }
        }
    }
    return best
}

/**
 * `matchJoinedWords`: short words followed by a short prefix are noise inside a
 * title ("inth" in "in thread"). Allow them at its start ("t3co"), before three
 * letters ("gomod"), or as a whole word ("vscode").
 */
private fun matchJoinedWords(field: SearchField, token: String): TokenMatch? {
    val firstWord = field.wordStarts.indexOfFirst { it }
    for (start in field.text.indices) {
        if (!field.wordStarts[start] || field.text[start] != token[0]) continue
        val runs = mutableListOf<SearchRange>()
        var at = start
        var rest = token
        while (rest.isNotEmpty() && at < field.text.length) {
            val end = wordEnd(field, at)
            if (rest.length <= end - at && field.text.startsWith(rest, at)) {
                runs += SearchRange(at, at + rest.length)
                rest = ""
            } else if (rest.startsWith(field.text.substring(at, end))) {
                runs += SearchRange(at, end)
                rest = rest.substring(end - at)
                at = end
                while (at < field.text.length && !field.wordStarts[at]) at++
            } else {
                break
            }
        }
        val last = runs.lastOrNull()
        if (rest.isNotEmpty() || last == null || runs.size < 2) continue
        if (
            last.end == wordEnd(field, last.start) ||
            last.end - last.start >= 3 ||
            start == firstWord ||
            runs.dropLast(1).all { it.end - it.start >= 3 }
        ) {
            return TokenMatch(JOINED_WORDS, start, last.end, runs)
        }
    }
    return null
}

private class WeightedSearchField(
    val value: String,
    val weight: Double,
    /** Only a whole-word match counts, as for a short query against keywords. */
    val wholeWords: Boolean = false,
    /** Only a match at a word start counts, without typos. */
    val keyword: Boolean = false,
    /** Where the thread lives: helps a match, but never makes it strong. */
    val context: Boolean = false,
    /** A token of four or more letters may match inside a word here (titles only). */
    val midWord: Boolean = false,
)

private class SearchFieldsMatch(
    /** Roughly 0.3 (inside a word) to 1.5 (exact title). */
    val quality: Double,
    /** Every token matched at a word start, without a typo. */
    val strong: Boolean,
    /** Source ranges matched in the first field, for highlighting. */
    val primaryRanges: List<SearchRange>,
)

private val FILLER_WORDS = setOf("a", "an", "and", "for", "in", "of", "on", "the", "to", "with")

private fun mergeRanges(ranges: List<SearchRange>): List<SearchRange> {
    val merged = mutableListOf<SearchRange>()
    for (range in ranges.sortedBy { it.start }) {
        val last = merged.lastOrNull()
        if (last != null && range.start <= last.end) {
            merged[merged.lastIndex] = SearchRange(last.start, max(last.end, range.end))
        } else {
            merged += range
        }
    }
    return merged
}

/**
 * `matchSearchFields`: every query token must match some field. Quality is the
 * mean of each token's best weighted match, plus a bonus when the first field
 * holds the whole query as a phrase or equals it.
 */
private fun matchSearchFields(
    normalizedQuery: String,
    fields: List<WeightedSearchField>,
    typos: Boolean = false,
): SearchFieldsMatch? {
    if (normalizedQuery.isEmpty()) return null
    class Prepared(val field: SearchField, val source: WeightedSearchField)
    val prepared = fields.filter { it.value.isNotEmpty() }.map { Prepared(prepareSearchField(it.value), it) }
    // Identity, as in the original: the cache hands equal values one object, so a
    // branch equal to the title counts as the title.
    val primary = if (fields.firstOrNull()?.value?.isNotEmpty() == true) prepared.firstOrNull()?.field else null
    val tokens = normalizedQuery.split(' ').filter(String::isNotEmpty)
    if (tokens.isEmpty()) return null
    // A phrase-style query ("fix the sidebar") carries filler words a name may
    // leave out; they count where they match but are not required.
    val fillerOptional = tokens.any { it !in FILLER_WORDS }
    val primaryRanges = mutableListOf<SearchRange>()
    var total = 0.0
    var counted = 0
    var strong = true
    for (token in tokens) {
        var best = 0.0
        var bestPrimary: TokenMatch? = null
        var atWordStart = false
        for (entry in prepared) {
            val field = entry.field
            val weight = entry.source.weight
            if (best >= weight && field !== primary && atWordStart) continue
            val match =
                matchToken(field, token, typos && !entry.source.keyword, field === primary, entry.source.midWord)
                    ?: continue
            val floor = if (entry.source.wholeWords) WHOLE_WORD else if (entry.source.keyword) 0.7 else 0.0
            if (match.score < floor) continue
            if (field === primary) bestPrimary = match
            if (match.score >= 0.7 && !entry.source.context) atWordStart = true
            if (match.score * weight > best) best = match.score * weight
        }
        if (best == 0.0) {
            if (fillerOptional && token in FILLER_WORDS) continue
            return null
        }
        strong = strong && atWordStart
        bestPrimary?.let { primaryRanges += it.letters ?: listOf(SearchRange(it.start, it.end)) }
        total += best
        counted += 1
    }
    var quality = total / counted
    if (primary != null) {
        if (primary.text == normalizedQuery) quality += WHOLE_NAME - WHOLE_WORD
        else if (tokens.size > 1 && primary.text.contains(normalizedQuery)) quality += 0.25
    }
    return SearchFieldsMatch(quality, strong, mergeRanges(primaryRanges))
}

// A two-letter Latin substring of a message is noise rather than a match.
private const val MESSAGE_TOKEN_MIN_LENGTH = 3

data class MessageSearchTokens(val long: List<String>, val short: List<String>, val words: List<String>)

/**
 * `messageSearchTokens`: every long token must match a message; short ones only
 * narrow. No long tokens means the server has nothing to search, so the client
 * skips the request. Two CJK characters count as long.
 */
fun messageSearchTokens(query: String): MessageSearchTokens {
    val tokens =
        Normalizer.normalize(query.replace(WRAPPING_QUOTES, "").replace(Regex("[‘’]"), "'"), Normalizer.Form.NFC)
            .lowercase(Locale.ROOT)
            .split(WHITESPACE)
            .filter(String::isNotEmpty)
    fun searchable(token: String): Boolean {
        val length = token.codePointCount(0, token.length)
        return length >= MESSAGE_TOKEN_MIN_LENGTH || (length == 2 && hasCjkWord(token))
    }
    return MessageSearchTokens(
        long = tokens.filter(::searchable),
        short = tokens.filterNot(::searchable),
        words = tokens,
    )
}

/** Server message qualities at or above this matched a whole word or word start. */
private const val MESSAGE_PREFIX = 0.8

private const val DAY_MS = 24L * 60 * 60 * 1000
/** Both activity recency and open counts halve every two weeks. */
const val SEARCH_RECENCY_HALF_LIFE_MS = 14 * DAY_MS
/**
 * Opens closer together than this count once, so flipping between threads is
 * not frequency. Clients skip sending them.
 */
const val THREAD_OPEN_DEDUPE_MS = 10 * 60 * 1000L
/** Picked queries are stored this long; clients send no more. */
private const val THREAD_SEARCH_PICK_MAX_LENGTH = 32

/**
 * `pickedQuery`: [query] as a pick is stored and compared, folded and cut to the
 * stored length without splitting a surrogate pair.
 */
fun pickedQuery(query: String): String {
    val cut = foldSearchQuery(query).take(THREAD_SEARCH_PICK_MAX_LENGTH)
    return (if (cut.isNotEmpty() && cut.last().isHighSurrogate()) cut.dropLast(1) else cut).trim()
}

private fun decay(ageMs: Long): Double = 2.0.pow(-max(0L, ageMs).toDouble() / SEARCH_RECENCY_HALF_LIFE_MS)

/** `ThreadSearchPullRequest`: matched by number, title, and whole-query references. */
data class ThreadSearchPullRequest(
    val number: Int,
    val title: String?,
    /** `owner/repo#123` and the pull request URL, matched only by a whole query. */
    val references: List<String>,
)

/** `ThreadSearchSubject`: what a thread is ranked by. */
data class ThreadSearchSubject(
    /** Matched only by the whole query, like references. */
    val id: String? = null,
    val title: String,
    val branch: String? = null,
    val projectTitle: String? = null,
    /** Only set with two or more environments; it tells same-named threads apart. */
    val environmentLabel: String? = null,
    val pullRequests: List<ThreadSearchPullRequest> = emptyList(),
    /** Latest user activity, epoch millis. */
    val activityAtMillis: Long? = null,
    val opens: ThreadOpenStats? = null,
)

// Use reorders matches of about the same quality but never turns a mid-word
// hit on a busy thread into a better result than a whole word on a quiet one.
private const val USE_BOOST_LIMIT = 2.5

/** 1 for a stale, never-opened thread, about 2 for one active today, up to 2.5 for threads opened often. */
private fun threadUsageBoost(thread: ThreadSearchSubject, now: Long): Double {
    val recency = thread.activityAtMillis?.let { decay(now - it) } ?: 0.0
    val opens = thread.opens?.let { it.score * decay(now - it.openedAtMillis) } ?: 0.0
    return min(USE_BOOST_LIMIT, 1 + recency + 0.5 * (ln(1 + opens) / ln(2.0)))
}

private const val REFERENCE_PREFIX_MIN = 8

/**
 * `matchReference`: IDs and URLs have many word starts, so they need a long
 * prefix, or any long part containing a digit.
 */
private fun matchReference(normalizedQuery: String, thread: ThreadSearchSubject): SearchFieldsMatch? {
    if (normalizedQuery.length < REFERENCE_PREFIX_MIN) return null
    val references = listOfNotNull(thread.id) + thread.pullRequests.flatMap { it.references }
    val anywhere = containsNumber(normalizedQuery)
    // A number is never cut short: ".../pull/2288" is not pull request 22889.
    fun notCut(text: String, end: Int) = text.getOrNull(end)?.let { !isNumberCodePoint(it.code) } ?: true
    val matched =
        references.any { reference ->
            val folded = reference.lowercase(Locale.ROOT)
            val at = folded.indexOf(normalizedQuery)
            // A pasted link may go on past the reference: ".../pull/22889/files".
            if (anywhere) {
                (at != -1 && notCut(folded, at + normalizedQuery.length)) ||
                    (normalizedQuery.startsWith(folded) && notCut(normalizedQuery, folded.length))
            } else {
                at == 0
            }
        }
    return if (matched) SearchFieldsMatch(0.6, true, emptyList()) else null
}

// Picks expire from the pick time; they reorder word matches within a group.
private const val PICK_WINDOW_MS = 21 * DAY_MS
private const val PICK_PREFIX_MIN_LENGTH = 3

/** The latest pick of the thread for this query, while it lasts. */
private fun activePick(thread: ThreadSearchSubject, normalizedQuery: String, now: Long): ThreadOpenPick? {
    val query = pickedQuery(normalizedQuery)
    val prefix = query.length >= PICK_PREFIX_MIN_LENGTH
    var exact: ThreadOpenPick? = null
    var latest: ThreadOpenPick? = null
    for (pick in thread.opens?.picks.orEmpty()) {
        if (now - pick.atMillis > PICK_WINDOW_MS) continue
        if (pick.query == query && (exact == null || pick.atMillis > exact.atMillis)) exact = pick
        val matches = pick.query == query || (prefix && pick.query.startsWith(query))
        if (matches && (latest == null || pick.atMillis > latest.atMillis)) latest = pick
    }
    return exact ?: latest
}

/** `ThreadSearchMatch` in paletteSearch: how a thread matched. */
data class ThreadSearchScore(
    /** Match quality times use; what results sort by. */
    override val score: Double,
    override val quality: Double,
    override val strong: Boolean,
    /** Matched ranges in the title. */
    val ranges: List<SearchRange>,
    override val pickedAtMillis: Long? = null,
    override val pickedExactly: Boolean = false,
    override val groupQuality: Double = quality,
) : RankableMatch

/** What [rankSearchItems] orders by. */
interface RankableMatch {
    val score: Double
    val quality: Double
    val strong: Boolean
    val pickedAtMillis: Long? get() = null
    val pickedExactly: Boolean get() = false
    val groupQuality: Double get() = quality
}

// Most threads' branches start with the app's worktree prefix ("t3/", and
// "t3code/" before it), so "t3 connect" would match nearly every thread. Only
// before more text, so typing "t3/" does not empty the list for a keystroke.
private val WORKTREE_BRANCH_PREFIX_PATTERN = Regex("^(?:t3|t3code)/(?=\\S)")

private fun searchableBranch(branch: String?): String? = branch?.replace(WORKTREE_BRANCH_PREFIX_PATTERN, "")

/**
 * `matchHint`: whether a field other than the title explains the match: the
 * first that holds the whole query, else, past the words the project or
 * environment holds, the first that holds the rest.
 */
private fun hasMatchHint(normalizedQuery: String, thread: ThreadSearchSubject, typos: Boolean): Boolean {
    val candidates =
        thread.pullRequests.map { pullRequest ->
            "#${pullRequest.number}" + (pullRequest.title?.takeIf(String::isNotEmpty)?.let { " $it" } ?: "")
        } + listOfNotNull(searchableBranch(thread.branch), thread.projectTitle, thread.environmentLabel)
    fun holds(text: String, query: String) =
        matchSearchFields(query, listOf(WeightedSearchField(text, 1.0)), typos) != null
    fun hintFor(query: String) = candidates.any { it.isNotEmpty() && holds(it, query) }
    if (hintFor(normalizedQuery)) return true
    val contexts = listOfNotNull(thread.projectTitle, thread.environmentLabel).filter(String::isNotEmpty)
    val rest = normalizedQuery.split(' ').filter { token -> contexts.none { holds(it, token) } }.joinToString(" ")
    return rest.isNotEmpty() && rest != normalizedQuery && hintFor(rest)
}

/**
 * `scoreThreadSearch`: scores a thread for a normalized query, or null when it
 * does not match. Titles and pull request numbers count fully, pull request
 * titles and branches less, project and environment names least.
 */
fun scoreThreadSearch(
    normalizedQuery: String,
    thread: ThreadSearchSubject,
    now: Long,
    typos: Boolean = false,
    /** Palette picks; off where a pick would show up unexplained, such as a list filter. */
    picks: Boolean = true,
): ThreadSearchScore? {
    val fields = mutableListOf(WeightedSearchField(thread.title, 1.0, midWord = true))
    for (pullRequest in thread.pullRequests) {
        fields += WeightedSearchField("#${pullRequest.number}", 1.0)
        pullRequest.title?.takeIf(String::isNotEmpty)?.let { fields += WeightedSearchField(it, 0.5) }
    }
    searchableBranch(thread.branch)?.takeIf(String::isNotEmpty)?.let { fields += WeightedSearchField(it, 0.6) }
    thread.projectTitle?.takeIf(String::isNotEmpty)?.let { fields += WeightedSearchField(it, 0.25, context = true) }
    thread.environmentLabel?.takeIf(String::isNotEmpty)?.let {
        fields += WeightedSearchField(it, 0.25, context = true)
    }
    // Strip pull request prefixes and the worktree prefix so queries match the stored fields.
    val fieldQuery =
        normalizedQuery
            .replace(Regex("^(?:pr|pull request|pull)\\s*(?=#?\\d)"), "")
            .replace(Regex("^#\\s*"), "")
            .replace(WORKTREE_BRANCH_PREFIX_PATTERN, "")
    val fieldMatch = matchSearchFields(fieldQuery, fields, typos)
    val referenceMatch = if (fieldMatch == null) matchReference(normalizedQuery, thread) else null
    val match = fieldMatch ?: referenceMatch ?: return null
    // A row whose title shows no match must say why. With several words, one
    // that cannot (its words spread over unrelated fields) is a coincidence.
    val titleShowsMatch = match.primaryRanges.isNotEmpty()
    if (!titleShowsMatch && referenceMatch == null && fieldQuery.contains(' ') &&
        !hasMatchHint(fieldQuery, thread, typos)
    ) {
        return null
    }
    val pick = if (picks) activePick(thread, normalizedQuery, now) else null
    return ThreadSearchScore(
        score = match.quality * threadUsageBoost(thread, now),
        quality = match.quality,
        // A thread's whole title never lifts its group over a command or project
        // of that name: every new thread is titled "New thread" for a while.
        groupQuality = min(match.quality, WHOLE_NAME - 0.01),
        strong = match.strong,
        ranges = match.primaryRanges,
        pickedAtMillis = pick?.atMillis,
        pickedExactly = pick != null && pick.query == pickedQuery(normalizedQuery),
    )
}

/** How strongly a thread's messages match: 21 hits about 5.5, one hit 2. */
private fun messageMatchWeight(hits: Int): Double = 1 + ln(1.0 + hits) / ln(2.0)

/**
 * `rankMessageMatches`: server message hits across environments, word matches
 * first, then (with [picks]) a pick, then the closer match, then use.
 */
fun <T> rankMessageMatches(
    matches: List<T>,
    quality: (T) -> Double?,
    hits: (T) -> Int?,
    messageCreatedAt: (T) -> String?,
    subjectOf: (T) -> ThreadSearchSubject?,
    normalizedQuery: String,
    now: Long,
    picks: Boolean = true,
): List<T> {
    class Ranked(val match: T, val quality: Double, val pickedAt: Long, val score: Double)
    return matches
        .mapNotNull { match ->
            val subject = subjectOf(match) ?: return@mapNotNull null
            Ranked(
                match = match,
                // Older servers send no quality; their matches count as words.
                quality = quality(match) ?: 1.0,
                pickedAt = if (picks) activePick(subject, normalizedQuery, now)?.atMillis ?: 0 else 0,
                score = messageMatchWeight(hits(match) ?: 1) * threadUsageBoost(subject, now),
            )
        }
        .sortedWith(
            compareByDescending<Ranked> { it.quality >= MESSAGE_PREFIX }
                .thenByDescending { it.pickedAt }
                .thenByDescending { it.quality }
                .thenByDescending { it.score }
                .thenByDescending { messageCreatedAt(it.match).orEmpty() }
        )
        .map { it.match }
}

/** Whole words and prefixes share a class so "Compact" does not lift a result over "Compaction". */
private fun matchClass(quality: Double): Int =
    when {
        quality >= WHOLE_NAME -> 3
        quality >= 0.7 -> 2
        quality >= 0.4 -> 1
        else -> 0
    }

data class RankedSearchItem<T, M>(val item: T, val match: M)

/**
 * `rankSearchItems` (`rankSearchGroups` for one list): typos are tried only
 * when nothing matched at a word start, since they add noise beside real
 * matches. Use cannot cross match classes; picks reorder word matches.
 */
fun <T, M : RankableMatch> rankSearchItems(
    items: List<T>,
    score: (item: T, typos: Boolean) -> M?,
): List<RankedSearchItem<T, M>> {
    fun rank(typos: Boolean) = items.mapNotNull { item -> score(item, typos)?.let { RankedSearchItem(item, it) } }
    var ranked = rank(false)
    if (ranked.none { it.match.strong }) ranked = rank(true)
    fun wordMatch(match: M) = min(matchClass(match.quality), 2)
    fun exactPickAt(match: M) = if (match.pickedExactly) match.pickedAtMillis ?: 0 else 0
    return ranked.sortedWith(
        compareByDescending<RankedSearchItem<T, M>> { wordMatch(it.match) }
            .thenByDescending { exactPickAt(it.match) }
            .thenByDescending { matchClass(it.match.quality) }
            .thenByDescending { it.match.pickedAtMillis ?: 0 }
            .thenByDescending { it.match.score }
    )
}

/** `SearchTermsMatch`: a settings row's match against its label and keywords. */
data class SearchTermsMatch(
    override val score: Double,
    override val quality: Double,
    override val strong: Boolean,
    val ranges: List<SearchRange>,
    override val groupQuality: Double = quality,
) : RankableMatch

private const val KEYWORD_QUALITY = 0.65
// Below this, keyword prefixes match too much: "co" or "pro" would list most
// settings. A short query still matches a keyword it equals ("pr").
private const val KEYWORD_QUERY_MIN_LENGTH = 4

/**
 * `scoreSearchTerms`: the first term is the visible name; the rest are hidden
 * keywords that count less, so they cannot lift a row above a matching name.
 */
fun scoreSearchTerms(normalizedQuery: String, terms: List<String>, typos: Boolean = false): SearchTermsMatch? {
    // Filler words ("the ci") do not make a short query long.
    val meaningful = normalizedQuery.split(' ').filter { it !in FILLER_WORDS }.joinToString(" ")
    val short = meaningful.ifEmpty { normalizedQuery }.length < KEYWORD_QUERY_MIN_LENGTH
    val match =
        matchSearchFields(
            normalizedQuery,
            terms.mapIndexed { index, value ->
                if (index == 0) WeightedSearchField(value, 1.0)
                // An alias typed in full ("pr") is deliberate, above a mere name prefix.
                else WeightedSearchField(value, if (short) 0.8 else 0.6, wholeWords = short, keyword = true)
            },
            typos,
        ) ?: return null
    return SearchTermsMatch(
        score = match.quality,
        quality = match.quality,
        groupQuality = if (match.primaryRanges.isNotEmpty()) match.quality else min(match.quality, KEYWORD_QUALITY),
        strong = match.strong,
        ranges = match.primaryRanges,
    )
}
