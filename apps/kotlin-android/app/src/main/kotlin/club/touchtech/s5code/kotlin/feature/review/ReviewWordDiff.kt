package club.touchtech.s5code.kotlin.feature.review

import club.touchtech.s5code.kotlin.design.text.wordDiff
import club.touchtech.s5code.kotlin.model.DiffLine
import club.touchtech.s5code.kotlin.model.DiffLineKind

private const val MAX_WORD_DIFF_RANGES = 4
private const val MAX_WORD_DIFF_COVERAGE = 0.45

/**
 * Pairs adjacent removed/added runs and computes restrained intra-line ranges.
 * Dense rewrites keep the line-level tint; small edits get the stronger word tint.
 */
fun reviewWordDiffRanges(lines: List<DiffLine>): Map<Int, List<IntRange>> {
    val rangesByLine = mutableMapOf<Int, List<IntRange>>()
    var index = 0
    while (index < lines.size) {
        val removed = mutableListOf<Int>()
        val added = mutableListOf<Int>()
        while (lines.getOrNull(index)?.kind == DiffLineKind.Removed) removed += index++
        while (lines.getOrNull(index)?.kind == DiffLineKind.Added) added += index++
        repeat(minOf(removed.size, added.size)) { pairIndex ->
            val removedIndex = removed[pairIndex]
            val addedIndex = added[pairIndex]
            val (removedRanges, addedRanges) =
                wordDiff(lines[removedIndex].text, lines[addedIndex].text)
            restrainedRanges(lines[removedIndex].text, removedRanges)?.let {
                rangesByLine[removedIndex] = it
            }
            restrainedRanges(lines[addedIndex].text, addedRanges)?.let {
                rangesByLine[addedIndex] = it
            }
        }
        if (removed.isEmpty() && added.isEmpty()) index++
    }
    return rangesByLine
}

private fun restrainedRanges(text: String, ranges: List<IntRange>): List<IntRange>? {
    val trimmed =
        ranges.mapNotNull { range ->
            var start = range.first.coerceAtLeast(0)
            var endExclusive = (range.last + 1).coerceAtMost(text.length)
            while (start < endExclusive && text[start].isWhitespace()) start++
            while (endExclusive > start && text[endExclusive - 1].isWhitespace()) endExclusive--
            if (endExclusive > start) start until endExclusive else null
        }
    if (trimmed.isEmpty() || trimmed.size > MAX_WORD_DIFF_RANGES) return null
    val meaningful = text.count { !it.isWhitespace() }
    if (meaningful == 0) return null
    val highlighted = trimmed.sumOf { range -> text.substring(range).count { !it.isWhitespace() } }
    return trimmed.takeIf { highlighted.toDouble() / meaningful <= MAX_WORD_DIFF_COVERAGE }
}
