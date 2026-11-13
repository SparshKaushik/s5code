package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.DiffHunk
import club.touchtech.s5code.kotlin.model.DiffLine
import club.touchtech.s5code.kotlin.model.DiffLineKind
import club.touchtech.s5code.kotlin.model.ReviewFile
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewGuideDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewGuideExcerptDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewGuideNoteDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewGuideResultDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewGuideStopDto

/* Guided review, ported from client-runtime `reviewGuideTour.ts` and RN's
 * `guide/reviewGuideState.ts` and `guide/guideDiffFocus.ts`. */

fun stepTourPosition(current: Int, delta: Int, stopCount: Int): Int = (current + delta).coerceAtMost(stopCount - 1).coerceAtLeast(0)

/** Every file a stop shows or names, excerpts first. */
fun stopFilePaths(stop: PullRequestReviewGuideStopDto): List<String> =
    (stop.excerpts.map { it.path } + stop.files.map { it.path }).distinct()

data class LetteredNote(val marker: String, val note: PullRequestReviewGuideNoteDto)

/** A stop's notes lettered A–Z in guide order, then numbered. */
fun letterStopNotes(stop: PullRequestReviewGuideStopDto): List<LetteredNote> =
    stop.notes.mapIndexed { index, note -> LetteredNote(if (index < 26) ('A' + index).toString() else (index + 1).toString(), note) }

/* ── Excerpts ────────────────────────────────────────────────────────── */

/**
 * `DiffRow`: one diff line with both sides' positions. [oldAt]/[newAt] are
 * where the line sits on each side even when it exists on only one.
 */
data class GuideDiffRow(
    val kind: DiffLineKind,
    val oldLine: Int?,
    val newLine: Int?,
    val oldAt: Int,
    val newAt: Int,
    val text: String,
    val hunk: Int,
)

private val HUNK_START = Regex("""^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@""")

fun guideDiffRows(file: ReviewFile): List<GuideDiffRow> =
    buildList {
        file.hunks.forEachIndexed { hunkIndex, hunk ->
            val match = HUNK_START.find(hunk.header)
            var oldAt = match?.groupValues?.get(1)?.toIntOrNull() ?: 1
            var newAt = match?.groupValues?.get(2)?.toIntOrNull() ?: 1
            hunk.lines.forEach { line ->
                val onOld = line.kind != DiffLineKind.Added
                val onNew = line.kind != DiffLineKind.Removed
                add(GuideDiffRow(line.kind, oldAt.takeIf { onOld }, newAt.takeIf { onNew }, oldAt, newAt, line.text, hunkIndex))
                if (onOld) oldAt += 1
                if (onNew) newAt += 1
            }
        }
    }

/**
 * The rows an excerpt covers on its side. An excerpt starting on an addition
 * brings the lines it replaced; one ending on a removal brings its replacement.
 */
fun excerptRows(rows: List<GuideDiffRow>, side: String, start: Int, end: Int): List<GuideDiffRow> {
    fun inRange(row: GuideDiffRow): Boolean {
        val line = if (side == "new") row.newLine else row.oldLine
        return line != null && line in start..end
    }
    var first = rows.indexOfFirst(::inRange)
    if (first == -1) return emptyList()
    var last = rows.indexOfLast(::inRange)
    if (side == "new" && rows[first].kind == DiffLineKind.Added) {
        while (first > 0 && rows[first - 1].kind == DiffLineKind.Removed && rows[first - 1].hunk == rows[first].hunk) first -= 1
    }
    if (side == "old" && rows[last].kind == DiffLineKind.Removed) {
        while (last < rows.size - 1 && rows[last + 1].kind == DiffLineKind.Added && rows[last + 1].hunk == rows[last].hunk) last += 1
    }
    return rows.subList(first, last + 1)
}

/**
 * `rowsToPatch`, as a parsed file: [rows] cut from [file], one hunk per source
 * hunk, with headers numbered as git would. Null when there is nothing to show.
 */
fun excerptFile(file: ReviewFile, rows: List<GuideDiffRow>): ReviewFile? {
    if (rows.isEmpty()) return null
    val hunks =
        rows.fold(mutableListOf<MutableList<GuideDiffRow>>()) { groups, row ->
            if (groups.lastOrNull()?.last()?.hunk == row.hunk) groups.last() += row else groups += mutableListOf(row)
            groups
        }.map { group ->
            val head = group.first()
            val oldCount = group.count { it.oldLine != null }
            val newCount = group.count { it.newLine != null }
            // Git names the line before an empty side, so a pure insertion after line 9 is `-9,0`.
            val oldStart = if (oldCount == 0) (head.oldAt - 1).coerceAtLeast(0) else head.oldAt
            val newStart = if (newCount == 0) (head.newAt - 1).coerceAtLeast(0) else head.newAt
            DiffHunk(
                "@@ -$oldStart,$oldCount +$newStart,$newCount @@",
                group.map { DiffLine(it.kind, it.text, it.oldLine, it.newLine) },
            )
        }
    return file.copy(
        additions = rows.count { it.kind == DiffLineKind.Added },
        deletions = rows.count { it.kind == DiffLineKind.Removed },
        hunks = hunks,
    )
}

/** The slice of [file] an excerpt shows, or null when its lines are not in the loaded diff. */
fun excerptFile(file: ReviewFile, excerpt: PullRequestReviewGuideExcerptDto): ReviewFile? =
    excerptFile(file, excerptRows(guideDiffRows(file), excerpt.side, excerpt.start, excerpt.end))

/** `GuideDiffFocus`: the lines on [side] (old | new) a guide excerpt opens the diff at. */
data class GuideDiffFocus(val side: String, val start: Int, val end: Int) {
    companion object {
        /** Reads the route form `side:start:end`. */
        fun parse(raw: String?): GuideDiffFocus? {
            val parts = raw?.split(':') ?: return null
            if (parts.size != 3 || parts[0] !in setOf("old", "new")) return null
            val start = parts[1].toIntOrNull() ?: return null
            val end = parts[2].toIntOrNull() ?: return null
            return GuideDiffFocus(parts[0], start, end)
        }
    }
}

/**
 * `guideFocusedRows`: indices into [file]'s flattened lines that a guide
 * excerpt points at, as the range the diff screen highlights.
 */
fun guideFocusLines(file: ReviewFile, focus: GuideDiffFocus): IntRange? {
    val lines = file.hunks.flatMap { it.lines }
    val hits = lines.indices.filter { index ->
        val line = if (focus.side == "old") lines[index].oldNo else lines[index].newNo
        line != null && line in focus.start..focus.end
    }
    return if (hits.isEmpty()) null else hits.first()..hits.last()
}

/* ── Guide state ─────────────────────────────────────────────────────── */

data class ReviewGuideState(
    val guide: PullRequestReviewGuideDto?,
    val running: Boolean,
    /** The guide was written for an older head than the PR's current one. */
    val stale: Boolean,
    /** The current head's failed run, if any. */
    val failure: String?,
)

fun reviewGuideState(data: PullRequestReviewGuideResultDto?): ReviewGuideState {
    val guide = data?.guide
    val run = data?.run
    return ReviewGuideState(
        guide = guide,
        running = run?.tag == "Running",
        stale = guide != null && data.currentHeadSha != null && guide.headSha != data.currentHeadSha,
        failure = run?.takeIf { it.tag == "Failed" && it.headSha == data.currentHeadSha }?.detail,
    )
}

enum class GuideNoticeKind { Writing, Failed, Unavailable, Loading, Stale, Ready, Empty }

data class GuideNotice(val kind: GuideNoticeKind, val title: String, val description: String, val action: String)

/** `guideNoticeState`: the card above (or in place of) the guide outline. */
fun guideNoticeState(
    data: PullRequestReviewGuideResultDto?,
    running: Boolean,
    startError: String?,
    error: String?,
): GuideNotice {
    val state = reviewGuideState(data)
    if (running) {
        return GuideNotice(
            GuideNoticeKind.Writing,
            "Rewriting guide",
            if (state.guide != null) {
                "You can keep reading this guide while the agent writes the update."
            } else {
                "An agent is reading the change and writing the tour. You can leave and come back when it’s ready."
            },
            if (state.guide != null) "Rewriting…" else "Writing guide…",
        )
    }
    (startError ?: state.failure)?.let { return GuideNotice(GuideNoticeKind.Failed, "Could not write guide", it, "Try again") }
    if (error != null) return GuideNotice(GuideNoticeKind.Unavailable, "Could not refresh guide", error, "Try again")
    if (data == null) return GuideNotice(GuideNoticeKind.Loading, "Review guide", "Checking for a saved guide…", "Loading guide…")
    if (state.stale) {
        return GuideNotice(
            GuideNoticeKind.Stale,
            "New changes since this guide",
            "You can read the previous guide, or rewrite it for the latest changes.",
            "Rewrite guide",
        )
    }
    return GuideNotice(
        if (state.guide != null) GuideNoticeKind.Ready else GuideNoticeKind.Empty,
        "Review guide",
        "Get a tour of the change, with code excerpts and notes on what needs a closer look.",
        "Write guide",
    )
}
