package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.DiffLineKind
import club.touchtech.s5code.kotlin.transport.TransportJson
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewGuideDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewGuideNoteDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewGuideResultDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewGuideRunDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewGuideStopDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Ports of client-runtime `reviewGuideTour.test.ts` and RN `reviewGuideState.test.ts`. */
class PullRequestReviewGuideTest {

    private val patch =
        """
        diff --git a/src/tray.ts b/src/tray.ts
        --- a/src/tray.ts
        +++ b/src/tray.ts
        @@ -10,5 +10,6 @@ function tray()
         const a = 1;
        -const size = pick();
        +const size = "xs";
        +const variant = "tray";
         const b = 2;
         const c = 3;
         const d = 4;
        @@ -40,3 +41,2 @@ function footer()
         keep();
        -dropped();
         tail();
        """.trimIndent()

    private val file = parseUnifiedDiff(patch).single()
    private val rows = guideDiffRows(file)

    private fun numbers(rows: List<GuideDiffRow>) = rows.map { Triple(it.kind, it.oldLine, it.newLine) }

    @Test
    fun `holds the tour to its first and last stops`() {
        assertEquals(0, stepTourPosition(0, -1, 3))
        assertEquals(1, stepTourPosition(0, 1, 3))
        assertEquals(2, stepTourPosition(2, 1, 3))
    }

    @Test
    fun `numbers both sides through context, deletions and additions`() {
        val context = DiffLineKind.Context
        val removed = DiffLineKind.Removed
        val added = DiffLineKind.Added
        assertEquals(
            listOf(
                Triple(context, 10, 10),
                Triple(removed, 11, null),
                Triple(added, null, 11),
                Triple(added, null, 12),
                Triple(context, 12, 13),
                Triple(context, 13, 14),
                Triple(context, 14, 15),
                Triple(context, 40, 41),
                Triple(removed, 41, null),
                Triple(context, 42, 42),
            ),
            numbers(rows),
        )
    }

    @Test
    fun `brings the replaced lines along with an excerpt that starts on an addition`() {
        assertEquals(
            listOf(DiffLineKind.Removed, DiffLineKind.Added, DiffLineKind.Added),
            excerptRows(rows, "new", 11, 12).map { it.kind },
        )
    }

    @Test
    fun `selects old-side lines for a removal and stops at the hunk boundary`() {
        assertEquals(listOf(DiffLineKind.Removed to 41), excerptRows(rows, "old", 41, 41).map { it.kind to it.oldLine })
    }

    @Test
    fun `returns nothing for a range outside the diff`() {
        assertTrue(excerptRows(rows, "new", 100, 120).isEmpty())
        assertNull(excerptFile(file, excerptRows(rows, "new", 100, 120)))
    }

    @Test
    fun `cuts a slice that renumbers to the same lines`() {
        val shown = excerptRows(rows, "new", 10, 13)
        val slice = excerptFile(file, shown)!!
        assertEquals(numbers(shown), numbers(guideDiffRows(slice)))
        assertEquals(2, slice.additions)
        assertEquals(1, slice.deletions)
    }

    @Test
    fun `starts a hunk per source hunk so separate hunks stay separate`() {
        val slice = excerptFile(file, excerptRows(rows, "old", 14, 42))!!
        assertEquals(listOf("@@ -14,1 +15,1 @@", "@@ -40,3 +41,2 @@"), slice.hunks.map { it.header })
    }

    @Test
    fun `letters notes in guide order`() {
        val notes = (1..28).map { PullRequestReviewGuideNoteDto(path = "a.ts", line = it, title = "Note $it") }
        val markers = letterStopNotes(PullRequestReviewGuideStopDto(notes = notes)).map { it.marker }
        assertEquals(listOf("A", "B", "C"), markers.take(3))
        assertEquals("Z", markers[25])
        assertEquals(listOf("27", "28"), markers.takeLast(2))
    }

    @Test
    fun `reads a diff focus from its route form and finds its lines`() {
        assertEquals(GuideDiffFocus("new", 11, 12), GuideDiffFocus.parse("new:11:12"))
        assertNull(GuideDiffFocus.parse("both:1:2"))
        assertNull(GuideDiffFocus.parse("new:1"))
        assertNull(GuideDiffFocus.parse(null))
        assertEquals(2..3, guideFocusLines(file, GuideDiffFocus("new", 11, 12)))
        assertEquals(8..8, guideFocusLines(file, GuideDiffFocus("old", 41, 41)))
        assertNull(guideFocusLines(file, GuideDiffFocus("new", 100, 120)))
    }

    /* ── Guide state ─────────────────────────────────────────────────── */

    private val guide = PullRequestReviewGuideDto(headSha = "old-head", generatedAt = "2026-10-11T00:00:00.000Z")

    private fun failed(headSha: String) =
        PullRequestReviewGuideRunDto(tag = "Failed", headSha = headSha, failedAt = guide.generatedAt, detail = "Writer disconnected")

    @Test
    fun `keeps a previous guide readable during a rewrite`() {
        val state =
            reviewGuideState(
                PullRequestReviewGuideResultDto(guide, PullRequestReviewGuideRunDto(tag = "Running", headSha = "new-head"), "new-head"),
            )
        assertEquals(ReviewGuideState(guide, running = true, stale = true, failure = null), state)
    }

    @Test
    fun `does not label an unknown head stale`() {
        assertFalse(reviewGuideState(PullRequestReviewGuideResultDto(guide, null, null)).stale)
    }

    @Test
    fun `shows only failures for the current head`() {
        assertEquals("Writer disconnected", reviewGuideState(PullRequestReviewGuideResultDto(null, failed("old-head"), "old-head")).failure)
        assertNull(reviewGuideState(PullRequestReviewGuideResultDto(null, failed("old-head"), "new-head")).failure)
    }

    @Test
    fun `starts empty without claiming a job is running`() {
        assertEquals(ReviewGuideState(null, running = false, stale = false, failure = null), reviewGuideState(null))
    }

    private fun notice(data: PullRequestReviewGuideResultDto?, running: Boolean = false, error: String? = null) =
        guideNoticeState(data, running, startError = null, error = error).kind

    @Test
    fun `guide notice transitions`() {
        assertEquals(GuideNoticeKind.Loading, notice(null))
        assertEquals(GuideNoticeKind.Empty, notice(PullRequestReviewGuideResultDto(null, null, "head")))
        assertEquals(GuideNoticeKind.Ready, notice(PullRequestReviewGuideResultDto(guide, null, guide.headSha)))

        val failure = PullRequestReviewGuideResultDto(null, failed("head"), "head")
        assertEquals(GuideNoticeKind.Failed, notice(failure))
        assertEquals(GuideNoticeKind.Writing, notice(failure, running = true))

        val stale = PullRequestReviewGuideResultDto(guide, null, "head")
        assertEquals(GuideNoticeKind.Stale, notice(stale))
        assertEquals(GuideNoticeKind.Writing, notice(stale, running = true))
        assertEquals(GuideNoticeKind.Ready, notice(stale.copy(guide = guide.copy(headSha = "head"))))

        assertEquals(GuideNoticeKind.Unavailable, notice(null, error = "Connection lost"))
    }

    @Test
    fun `decodes the run union by its tag`() {
        val result =
            TransportJson.decodeFromString(
                PullRequestReviewGuideResultDto.serializer(),
                """
                {"guide":{"headSha":"abc","generatedAt":"2026-10-11T00:00:00.000Z",
                  "modelSelection":{"instanceId":"codex","model":"gpt-5"},
                  "stops":[{"title":"Tray","summary":"Why","attention":{"kind":"risk","note":"Check"},
                    "excerpts":[{"path":"a.ts","side":"old","start":1,"end":2,"caption":"c"}],
                    "files":[{"path":"a.ts","note":null}],"notes":[],"questions":["Is it safe?"]}]},
                 "run":{"_tag":"Running","headSha":"def","startedAt":"2026-10-12T00:00:00.000Z"},
                 "currentHeadSha":"def"}
                """.trimIndent(),
            )
        val state = reviewGuideState(result)
        assertTrue(state.running)
        assertTrue(state.stale)
        assertEquals("old", result.guide!!.stops.single().excerpts.single().side)
        assertEquals("risk", result.guide.stops.single().attention?.kind)
        assertEquals(listOf("Is it safe?"), result.guide.stops.single().questions)
    }
}
