package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.DiffHunk
import club.touchtech.s5code.kotlin.model.DiffLine
import club.touchtech.s5code.kotlin.model.DiffLineKind
import club.touchtech.s5code.kotlin.model.ReviewFile
import club.touchtech.s5code.kotlin.model.ReviewFileStatus

/**
 * Parses a unified diff into per-file hunks.
 *
 * The server sends `review.getDiffPreview` as raw diff text with a content hash
 * rather than a parsed tree, so the parsing happens here. That is the right split:
 * the diff is the thing git produces, and re-encoding it as JSON would make it
 * several times larger over a connection that may be a phone on cellular.
 *
 * Not cached: the review screen parses once per fetch and holds the result in
 * composition, so a map keyed by content hash would only ever hold the diff that is
 * already on screen.
 *
 * Only what the review screen renders is extracted. Binary files, mode changes,
 * and rename detection are recognised enough to label the row correctly; the
 * rest of git's diff vocabulary is skipped rather than half-supported.
 */
fun parseUnifiedDiff(diff: String): List<ReviewFile> {
    if (diff.isBlank()) return emptyList()

    val files = mutableListOf<ReviewFile>()
    var path: String? = null
    var oldPath: String? = null
    var status = ReviewFileStatus.Modified
    var binary = false
    val hunks = mutableListOf<DiffHunk>()
    var hunkHeader: String? = null
    var hunkLines = mutableListOf<DiffLine>()
    var oldLine = 0
    var newLine = 0

    fun flushHunk() {
        val header = hunkHeader ?: return
        hunks += DiffHunk(header, hunkLines.toList())
        hunkHeader = null
        hunkLines = mutableListOf()
    }

    fun flushFile() {
        flushHunk()
        val current = path ?: return
        val additions = hunks.sumOf { hunk -> hunk.lines.count { it.kind == DiffLineKind.Added } }
        val deletions = hunks.sumOf { hunk -> hunk.lines.count { it.kind == DiffLineKind.Removed } }
        files +=
            ReviewFile(
                path = current,
                additions = additions,
                deletions = deletions,
                status = if (binary) ReviewFileStatus.Binary else status,
                hunks = hunks.toList(),
            )
        path = null
        oldPath = null
        status = ReviewFileStatus.Modified
        binary = false
        hunks.clear()
    }

    diff.lineSequence().forEach { line ->
        when {
            line.startsWith("diff --git ") -> {
                flushFile()
                // `diff --git a/<old> b/<new>`. The b-side is the path to show;
                // the a-side only matters for detecting a rename.
                val paths = line.removePrefix("diff --git ").split(" b/", limit = 2)
                oldPath = paths.getOrNull(0)?.removePrefix("a/")
                path = paths.getOrNull(1) ?: oldPath
            }
            line.startsWith("new file mode") -> status = ReviewFileStatus.Added
            line.startsWith("deleted file mode") -> status = ReviewFileStatus.Deleted
            line.startsWith("rename from ") -> {
                status = ReviewFileStatus.Renamed
                oldPath = line.removePrefix("rename from ")
            }
            line.startsWith("rename to ") -> {
                status = ReviewFileStatus.Renamed
                path = line.removePrefix("rename to ")
            }
            line.startsWith("Binary files") || line.startsWith("GIT binary patch") -> binary = true
            line.startsWith("--- ") || line.startsWith("+++ ") -> Unit
            line.startsWith("@@") -> {
                flushHunk()
                hunkHeader = line
                val ranges = parseHunkRanges(line)
                oldLine = ranges.first
                newLine = ranges.second
            }
            hunkHeader != null ->
                when {
                    line.startsWith("+") -> {
                        hunkLines += DiffLine(DiffLineKind.Added, line.drop(1), null, newLine)
                        newLine += 1
                    }
                    line.startsWith("-") -> {
                        hunkLines += DiffLine(DiffLineKind.Removed, line.drop(1), oldLine, null)
                        oldLine += 1
                    }
                    line.startsWith("\\") -> Unit // "\ No newline at end of file"
                    else -> {
                        hunkLines += DiffLine(DiffLineKind.Context, line.drop(1), oldLine, newLine)
                        oldLine += 1
                        newLine += 1
                    }
                }
        }
    }
    flushFile()
    return files
}

/** Reads the old and new starting line numbers out of an `@@ -a,b +c,d @@` header. */
private fun parseHunkRanges(header: String): Pair<Int, Int> {
    val match = HUNK_HEADER.find(header) ?: return 1 to 1
    val old = match.groupValues.getOrNull(1)?.toIntOrNull() ?: 1
    val new = match.groupValues.getOrNull(2)?.toIntOrNull() ?: 1
    return old to new
}

private val HUNK_HEADER = Regex("""^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@""")
