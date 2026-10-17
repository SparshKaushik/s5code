package club.touchtech.s5code.kotlin.feature.review

import club.touchtech.s5code.kotlin.model.DiffLine
import club.touchtech.s5code.kotlin.model.DiffLineKind
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ReviewCommentTest {
    private val lines =
        listOf(
            DiffLine(DiffLineKind.Context, "val before = 1", 11, 11),
            DiffLine(DiffLineKind.Removed, "val value = 1", 12, null),
            DiffLine(DiffLineKind.Added, "val value = 2", null, 12),
        )

    @Test
    fun `selection normalizes indexes and formats a structured comment`() {
        val target = buildReviewCommentTarget("working-tree", "Working tree", "src/Theme.kt", lines, 2, 1)
        val formatted = formatReviewCommentContext(target, "Please keep this configurable.")

        assertEquals(1, target.normalizedStart)
        assertEquals(2, target.normalizedEnd)
        assertEquals("12", formatReviewSelectedRangeLabel(target))
        assertTrue(formatted.contains("<review_comment sectionId=\"working-tree\""))
        assertTrue(formatted.contains("filePath=\"src/Theme.kt\""))
        assertTrue(formatted.contains("startIndex=\"1\" endIndex=\"2\""))
        assertTrue(formatted.contains("@@ -12,1 +12,1 @@\n-val value = 1\n+val value = 2"))
        assertTrue(formatted.endsWith("</review_comment>"))
        val segments = parseReviewCommentMessageSegments("Before\n$formatted\nAfter")
        assertEquals(3, segments.size)
        val parsed = (segments[1] as ReviewCommentMessageSegment.Comment).comment
        assertEquals("src/Theme.kt", parsed.filePath)
        assertEquals("Please keep this configurable.", parsed.text)
        assertEquals("@@ -12,1 +12,1 @@\n-val value = 1\n+val value = 2", parsed.diff)
    }

    @Test
    fun `attributes are escaped and one-kind ranges keep their marker`() {
        val target = buildReviewCommentTarget("turn&1", "A \"turn\"", "a/<b>.kt", lines, 1, 1)
        val formatted = formatReviewCommentContext(target, "change")

        assertEquals("-12", formatReviewSelectedRangeLabel(target))
        assertTrue(formatted.contains("sectionId=\"turn&amp;1\""))
        assertTrue(formatted.contains("sectionTitle=\"A &quot;turn&quot;\""))
        assertTrue(formatted.contains("filePath=\"a/&lt;b&gt;.kt\""))
    }

    @Test
    fun `invalid structured comment stays ordinary text`() {
        val raw = "<review_comment filePath=\"a.kt\">broken</review_comment>"
        val segment = parseReviewCommentMessageSegments(raw).single()
        assertEquals(raw, (segment as ReviewCommentMessageSegment.Text).text)
    }

    @Test
    fun `word ranges pair removal and addition and skip dense rewrites`() {
        val focused = reviewWordDiffRanges(lines)
        assertEquals("1", lines[1].text.substring(focused.getValue(1).single()))
        assertEquals("2", lines[2].text.substring(focused.getValue(2).single()))

        val dense =
            listOf(
                DiffLine(DiffLineKind.Removed, "old old", 1, null),
                DiffLine(DiffLineKind.Added, "new new", null, 1),
            )
        assertFalse(reviewWordDiffRanges(dense).containsKey(0))
        assertFalse(reviewWordDiffRanges(dense).containsKey(1))
    }
}
