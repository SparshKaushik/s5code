package club.touchtech.s5code.kotlin.feature.review

import club.touchtech.s5code.kotlin.model.DiffLine
import club.touchtech.s5code.kotlin.model.DiffLineKind

data class ReviewInlineComment(
    val id: String,
    val sectionId: String,
    val sectionTitle: String,
    val filePath: String,
    val startIndex: Int,
    val endIndex: Int,
    val rangeLabel: String,
    val text: String,
    val diff: String,
    val fenceLanguage: String,
)

sealed interface ReviewCommentMessageSegment {
    data class Text(val id: String, val text: String) : ReviewCommentMessageSegment

    data class Comment(val comment: ReviewInlineComment) : ReviewCommentMessageSegment
}

fun parseReviewCommentMessageSegments(value: String): List<ReviewCommentMessageSegment> {
    val segments = mutableListOf<ReviewCommentMessageSegment>()
    var cursor = 0
    var commentIndex = 0
    REVIEW_COMMENT_BLOCK.findAll(value).forEach { match ->
        if (match.range.first > cursor) {
            segments +=
                ReviewCommentMessageSegment.Text(
                    id = "review-comment-text:$cursor",
                    text = value.substring(cursor, match.range.first),
                )
        }
        val comment = parseReviewInlineComment(match, commentIndex)
        if (comment == null) {
            segments +=
                ReviewCommentMessageSegment.Text(
                    id = "review-comment-invalid:${match.range.first}",
                    text = match.value,
                )
        } else {
            segments += ReviewCommentMessageSegment.Comment(comment)
            commentIndex++
        }
        cursor = match.range.last + 1
    }
    if (cursor < value.length) {
        segments += ReviewCommentMessageSegment.Text("review-comment-text:$cursor", value.substring(cursor))
    }
    return segments
}

private fun parseReviewInlineComment(match: MatchResult, index: Int): ReviewInlineComment? {
    val attributes =
        REVIEW_COMMENT_ATTRIBUTE.findAll(match.groupValues[1]).associate { attribute ->
            attribute.groupValues[1] to unescapeReviewCommentAttribute(attribute.groupValues[2])
        }
    val filePath = attributes["filePath"]?.trim().takeUnless { it.isNullOrEmpty() } ?: return null
    val sectionId = attributes["sectionId"]?.trim().takeUnless { it.isNullOrEmpty() } ?: return null
    val startIndex = attributes["startIndex"]?.toIntOrNull()?.takeIf { it >= 0 } ?: return null
    val endIndex = attributes["endIndex"]?.toIntOrNull()?.takeIf { it >= 0 } ?: return null
    val rawBody = match.groupValues[2]
    val fence = REVIEW_COMMENT_FENCE.findAll(rawBody).lastOrNull()
    val text = rawBody.substring(0, fence?.range?.first ?: rawBody.length).trim()
    return ReviewInlineComment(
        id = "review-comment:$index:$sectionId:$filePath:$startIndex:$endIndex",
        sectionId = sectionId,
        sectionTitle = attributes["sectionTitle"]?.trim().takeUnless { it.isNullOrEmpty() } ?: "Review",
        filePath = filePath,
        startIndex = minOf(startIndex, endIndex),
        endIndex = maxOf(startIndex, endIndex),
        rangeLabel = attributes["rangeLabel"]?.trim().takeUnless { it.isNullOrEmpty() } ?: "line",
        text = text,
        diff = fence?.groupValues?.get(3).orEmpty(),
        fenceLanguage = fence?.groupValues?.get(2)?.trim().takeUnless { it.isNullOrEmpty() } ?: "diff",
    )
}

private fun unescapeReviewCommentAttribute(value: String): String =
    value.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&")

private val REVIEW_COMMENT_BLOCK =
    Regex("""<review_comment\b([^>]*)>\s*([\s\S]*?)</review_comment>""")
private val REVIEW_COMMENT_ATTRIBUTE = Regex("""([a-zA-Z][a-zA-Z0-9_-]*)="([^"]*)"""")
private val REVIEW_COMMENT_FENCE = Regex("""(`{3,})([^\s`]*)[^\n]*\n([\s\S]*?)\n\1""")

/** A line or contiguous range selected from one review file. */
data class ReviewCommentTarget(
    val sectionId: String,
    val sectionTitle: String,
    val filePath: String,
    val lines: List<DiffLine>,
    val startIndex: Int,
    val endIndex: Int,
) {
    val normalizedStart: Int = minOf(startIndex, endIndex).coerceIn(0, lines.lastIndex.coerceAtLeast(0))
    val normalizedEnd: Int = maxOf(startIndex, endIndex).coerceIn(0, lines.lastIndex.coerceAtLeast(0))
    val selectedLines: List<DiffLine>
        get() = if (lines.isEmpty()) emptyList() else lines.subList(normalizedStart, normalizedEnd + 1)
}

fun buildReviewCommentTarget(
    sectionId: String,
    sectionTitle: String,
    filePath: String,
    lines: List<DiffLine>,
    anchorIndex: Int,
    lineIndex: Int,
) =
    ReviewCommentTarget(
        sectionId = sectionId,
        sectionTitle = sectionTitle,
        filePath = filePath,
        lines = lines,
        startIndex = minOf(anchorIndex, lineIndex),
        endIndex = maxOf(anchorIndex, lineIndex),
    )

fun formatReviewSelectedRangeLabel(target: ReviewCommentTarget): String {
    val lines = target.selectedLines
    val first = lines.firstOrNull() ?: return "line"
    val last = lines.last()
    val firstNumber = first.newNo ?: first.oldNo
    val lastNumber = last.newNo ?: last.oldNo
    if (firstNumber == null || lastNumber == null) return if (lines.size == 1) "line" else "${lines.size} lines"
    val marker =
        when {
            lines.all { it.kind == DiffLineKind.Added } -> "+"
            lines.all { it.kind == DiffLineKind.Removed } -> "-"
            else -> ""
        }
    return if (firstNumber == lastNumber) "$marker$firstNumber" else "$marker$firstNumber to $marker$lastNumber"
}

fun formatReviewCommentContext(target: ReviewCommentTarget, comment: String): String {
    val diff = formatSelectedDiff(target)
    val longestBacktickRun = Regex("`+").findAll(diff).maxOfOrNull { it.value.length } ?: 0
    val fence = "`".repeat(maxOf(3, longestBacktickRun + 1))
    return buildString {
        append("<review_comment")
        append(" sectionId=\"").append(escapeReviewCommentAttribute(target.sectionId)).append('"')
        append(" sectionTitle=\"").append(escapeReviewCommentAttribute(target.sectionTitle)).append('"')
        append(" filePath=\"").append(escapeReviewCommentAttribute(target.filePath)).append('"')
        append(" startIndex=\"").append(target.normalizedStart).append('"')
        append(" endIndex=\"").append(target.normalizedEnd).append('"')
        append(" rangeLabel=\"").append(escapeReviewCommentAttribute(formatReviewSelectedRangeLabel(target))).append("\">\n")
        append(comment.trim()).append('\n')
        append(fence).append("diff\n").append(diff).append('\n').append(fence)
        append("\n</review_comment>")
    }
}

private fun formatSelectedDiff(target: ReviewCommentTarget): String {
    val lines = target.selectedLines
    val old = hunkRange(lines.mapNotNull(DiffLine::oldNo))
    val new = hunkRange(lines.mapNotNull(DiffLine::newNo))
    val body = lines.joinToString("\n") { line ->
        val marker =
            when (line.kind) {
                DiffLineKind.Added -> "+"
                DiffLineKind.Removed -> "-"
                DiffLineKind.Context -> " "
            }
        marker + line.text
    }
    return "@@ -${old.first},${old.second} +${new.first},${new.second} @@\n${body.ifEmpty { " " }}"
}

private fun hunkRange(numbers: List<Int>): Pair<Int, Int> =
    if (numbers.isEmpty()) 0 to 0 else numbers.first() to numbers.size

private fun escapeReviewCommentAttribute(value: String): String =
    value.replace("&", "&amp;").replace("\"", "&quot;").replace("<", "&lt;").replace(">", "&gt;")
