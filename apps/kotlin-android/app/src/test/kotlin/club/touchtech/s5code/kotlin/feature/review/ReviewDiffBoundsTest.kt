package club.touchtech.s5code.kotlin.feature.review

import club.touchtech.s5code.kotlin.model.DiffHunk
import club.touchtech.s5code.kotlin.model.DiffLine
import club.touchtech.s5code.kotlin.model.DiffLineKind
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

/** The mobile `nativeReviewDiffAdapter` row cap, applied to the review file card. */
class ReviewDiffBoundsTest {
    private fun hunk(header: String, lines: Int) =
        DiffHunk(header, List(lines) { DiffLine(DiffLineKind.Added, "line $it", null, it + 1) })

    @Test
    fun `keeps a diff within the cap untouched`() {
        val hunks = listOf(hunk("@@ a", 3), hunk("@@ b", 5))
        val bounded = boundReviewHunks(hunks, maxRows = 10)
        assertFalse(bounded.truncated)
        assertSame(hunks, bounded.hunks)
    }

    @Test
    fun `counts headers as rows and cuts mid-hunk`() {
        val bounded = boundReviewHunks(listOf(hunk("@@ a", 3), hunk("@@ b", 5)), maxRows = 6)
        assertTrue(bounded.truncated)
        assertEquals(listOf("@@ a" to 3, "@@ b" to 1), bounded.hunks.map { it.header to it.lines.size })
    }

    @Test
    fun `drops hunks that start after the cap`() {
        val bounded = boundReviewHunks(listOf(hunk("@@ a", 3), hunk("@@ b", 5)), maxRows = 4)
        assertTrue(bounded.truncated)
        assertEquals(listOf("@@ a" to 3), bounded.hunks.map { it.header to it.lines.size })
    }

    @Test
    fun `caps a huge file at two thousand rows`() {
        val bounded = boundReviewHunks(listOf(hunk("@@ a", 5_000)))
        assertTrue(bounded.truncated)
        assertEquals(REVIEW_MAX_FILE_ROWS - 1, bounded.hunks.single().lines.size)
    }
}
