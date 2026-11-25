package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.DiffLine
import club.touchtech.s5code.kotlin.model.DiffLineKind
import club.touchtech.s5code.kotlin.model.ReviewFile
import club.touchtech.s5code.kotlin.transport.wire.PullRequestChangeBreakdownDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDiffResultDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewThreadDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestThreadCommentDto
import java.text.NumberFormat
import java.util.Locale
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/*
 * Reviewing a pull request's files, ported from
 * `packages/shared/src/changedFileKind.ts` and
 * `apps/mobile/src/features/pull-requests/` (`pullRequestFileFilters.ts`,
 * `pullRequestDiffComments.ts`, `pullRequestReviewDrafts.ts`,
 * `pullRequestDiffPages.ts`, `PullRequestReviewSheet.tsx`).
 */

/* ── Changed file kinds ──────────────────────────────────────────────── */

private val GENERATED_FILES =
    setOf(
        "bun.lock", "bun.lockb", "cargo.lock", "composer.lock", "deno.lock", "flake.lock", "gemfile.lock", "go.sum",
        "gradle.lockfile", "mix.lock", "npm-shrinkwrap.json", "package-lock.json", "packages.lock.json", "pipfile.lock",
        "pnpm-lock.yaml", "podfile.lock", "poetry.lock", "pubspec.lock", "uv.lock", "yarn.lock",
    )
private val GENERATED_NAME =
    Regex("(\\.generated\\.|\\.gen\\.|\\.g\\.dart$|\\.pb\\.go$|_pb2(_grpc)?\\.py$|\\.pb\\.(cc|h)$|\\.min\\.(js|css)$|\\.(js|css)\\.map$)")
private val GENERATED_DIRS = setOf("__generated__", "generated")

private val MOCK_NAME =
    Regex("(\\.snap$|\\.(mocks?|fixtures?|stubs?|fakes?)\\.[^.]+$|^(mocks?|fixtures?|stubs?|fakes?)([-_.A-Z]|$))")
private val MOCK_DIRS =
    setOf("__fixtures__", "__mocks__", "__snapshots__", "fakes", "fixture", "fixtures", "mock", "mocks", "stubs", "testdata")

private val TEST_NAME =
    Regex(
        "(\\.(test|spec|e2e)\\.[^.]+$|_(test|spec)\\.[^.]+$|^test_.+\\.py$|^conftest\\.py$|^(vitest|jest)\\.setup\\.[^.]+$)",
        RegexOption.IGNORE_CASE,
    )
// Case-sensitive: `FooTest.java` is a test class, `Latest.java` is not.
private val CLASS_TEST_NAME = Regex("(Tests?|Spec)\\.(java|kt|kts|scala|groovy|cs|php|swift|m)$")
private val TEST_DIRS = setOf("__tests__", "__test__", "cypress", "e2e", "spec", "specs", "test", "tests")

private val DOCS_NAME = Regex("\\.(md|mdx|markdown|rst|adoc|txt)$", RegexOption.IGNORE_CASE)
// Plain-text files that tools read as config or input, not prose.
private val NOT_DOCS_NAME = Regex("^(requirements.*|constraints.*|CMakeLists|robots|llms)\\.txt$", RegexOption.IGNORE_CASE)

/** `ChangedFileKind`, in the order the change breakdown lists them. */
enum class ChangedFileKind(val label: String) {
    Code("Code"),
    Tests("Tests"),
    Mocks("Mocks"),
    Generated("Generated"),
    Docs("Docs"),
}

/**
 * `classifyChangedFile`: sorts a changed file by its path alone. Lockfiles and
 * build output count as generated, test support is kept apart from the tests
 * themselves, and prose is its own kind.
 */
fun classifyChangedFile(path: String): ChangedFileKind {
    val segments = path.replace('\\', '/').split('/')
    val name = segments.last()
    val dirs = segments.dropLast(1).map { it.lowercase() }
    fun inDir(set: Set<String>) = dirs.any { it in set }
    return when {
        name.lowercase() in GENERATED_FILES || GENERATED_NAME.containsMatchIn(name) || inDir(GENERATED_DIRS) ->
            ChangedFileKind.Generated
        MOCK_NAME.containsMatchIn(name) || inDir(MOCK_DIRS) -> ChangedFileKind.Mocks
        TEST_NAME.containsMatchIn(name) || CLASS_TEST_NAME.containsMatchIn(name) || inDir(TEST_DIRS) -> ChangedFileKind.Tests
        DOCS_NAME.containsMatchIn(name) && !NOT_DOCS_NAME.containsMatchIn(name) -> ChangedFileKind.Docs
        else -> ChangedFileKind.Code
    }
}

/** `isPullRequestTestFile`: test support such as fixtures and snapshots is hidden along with the tests. */
fun isPullRequestTestFile(path: String): Boolean =
    classifyChangedFile(path).let { it == ChangedFileKind.Tests || it == ChangedFileKind.Mocks }

/**
 * `PullRequestChangeBreakdownLine`: "Code +1,234 −56 · Tests +8 −0", skipping
 * kinds with no changed lines.
 */
fun pullRequestChangeBreakdownLabel(breakdown: PullRequestChangeBreakdownDto, locale: Locale = Locale.getDefault()): String {
    val format = NumberFormat.getIntegerInstance(locale)
    return ChangedFileKind.entries
        .mapNotNull { kind ->
            val count =
                when (kind) {
                    ChangedFileKind.Code -> breakdown.code
                    ChangedFileKind.Tests -> breakdown.tests
                    ChangedFileKind.Mocks -> breakdown.mocks
                    ChangedFileKind.Generated -> breakdown.generated
                    ChangedFileKind.Docs -> breakdown.docs
                }
            if (count.additions == 0 && count.deletions == 0) null
            else "${kind.label} +${format.format(count.additions)} \u2212${format.format(count.deletions)}"
        }
        .joinToString(" · ")
}

/** `filterPullRequestTestFiles`: [includePath] stays even when it is a test. */
fun filterPullRequestTestFiles(files: List<ReviewFile>, hideTestFiles: Boolean, includePath: String? = null): List<ReviewFile> =
    if (!hideTestFiles) files else files.filter { it.path == includePath || !isPullRequestTestFile(it.path) }

/* ── Review positions ────────────────────────────────────────────────── */

/**
 * `PullRequestReviewPosition`, flattened: added carries [newLine], deleted
 * [oldLine], context both plus a side.
 */
@Serializable
data class PullRequestReviewPosition(
    /** added | deleted | context */
    val kind: String,
    val oldLine: Int? = null,
    val newLine: Int? = null,
    /** left | right, context only. */
    val side: String? = null,
) {
    fun toJson(): JsonObject = buildJsonObject {
        put("kind", kind)
        when (kind) {
            "added" -> put("newLine", newLine ?: 0)
            "deleted" -> put("oldLine", oldLine ?: 0)
            else -> {
                put("oldLine", oldLine ?: 0)
                put("newLine", newLine ?: 0)
                put("side", side ?: "right")
            }
        }
    }

    private val oldSide: Boolean get() = kind == "deleted" || (kind == "context" && side == "left")

    /** The line number on the side the comment sits. */
    val line: Int get() = (if (oldSide) oldLine else newLine) ?: 0

    /** The diff side the comment anchors to. */
    val diffSide: String get() = if (oldSide) "left" else "right"

    /** `reviewPositionLabel`. */
    val label: String get() = "${if (oldSide) "Old line" else "Line"} $line"
}

/** `reviewPosition`: where a tapped diff row would place a line comment. */
fun reviewPosition(line: DiffLine): PullRequestReviewPosition? =
    when {
        line.kind == DiffLineKind.Added && line.newNo != null -> PullRequestReviewPosition("added", newLine = line.newNo)
        line.kind == DiffLineKind.Removed && line.oldNo != null -> PullRequestReviewPosition("deleted", oldLine = line.oldNo)
        line.oldNo != null && line.newNo != null ->
            PullRequestReviewPosition("context", oldLine = line.oldNo, newLine = line.newNo, side = "right")
        else -> null
    }

/* ── Review drafts ───────────────────────────────────────────────────── */

/** `PendingReviewComment`: a line comment saved for the next review, pinned to the revision it was written on. */
@Serializable
data class PendingReviewComment(
    val id: String,
    val path: String,
    val oldPath: String? = null,
    val position: PullRequestReviewPosition,
    val body: String,
    val headSha: String,
    val code: String,
) {
    /** `PullRequestReviewCommentDraft` on the wire. */
    fun toJson(): JsonObject = buildJsonObject {
        put("path", path)
        oldPath?.let { put("oldPath", it) }
        put("position", position.toJson())
        put("body", body)
    }
}

@Serializable
data class ReviewDraft(
    val summary: String = "",
    val comment: String = "",
    val comments: List<PendingReviewComment> = emptyList(),
) {
    val isEmpty: Boolean get() = summary.isEmpty() && comment.isEmpty() && comments.isEmpty()

    /** A comment written on an older revision blocks submitting until it moves. */
    fun isStale(headSha: String?): Boolean = comments.any { it.headSha != headSha }
}

/** `pullRequestReviewKey`. */
fun pullRequestReviewKey(environmentId: String, reference: PullRequestReference): String =
    listOf(
        environmentId,
        reference.projectId,
        reference.host.orEmpty(),
        reference.repository,
        reference.number.toString(),
    ).joinToString("\u0000")

/**
 * `clearSubmittedReview`: removes what a review sent, keeping anything edited
 * while it was in flight.
 */
fun clearSubmittedReview(current: ReviewDraft, submitted: ReviewDraft): ReviewDraft =
    current.copy(
        summary = if (current.summary == submitted.summary) "" else current.summary,
        comments = current.comments.filter { comment -> submitted.comments.none { it == comment } },
    )

/* ── Diff annotations ────────────────────────────────────────────────── */

/** A discussion or pending comment drawn under its diff line. */
data class PullRequestDiffAnnotation(
    /** `discussion:<threadId>` or `draft:<draftId>`. */
    val id: String,
    val title: String,
    val body: String,
    val footer: String,
)

/** One file's annotations: file-level ones under the header, the rest by flat line index. */
data class PullRequestFileAnnotations(
    val file: List<PullRequestDiffAnnotation> = emptyList(),
    val lines: Map<Int, List<PullRequestDiffAnnotation>> = emptyMap(),
) {
    val isEmpty: Boolean get() = file.isEmpty() && lines.isEmpty()
}

/**
 * `annotatePullRequestDiff` for one file: current discussions anchor on their
 * side and line (or the file when lineless), drafts written on [headSha] on
 * theirs. Outdated discussions and stale drafts are left out.
 */
fun annotatePullRequestFile(
    file: ReviewFile,
    threads: List<PullRequestReviewThreadDto>,
    drafts: List<PendingReviewComment>,
    headSha: String?,
): PullRequestFileAnnotations {
    val anchors = HashMap<Pair<String, Int>, Int>()
    file.hunks.flatMap { it.lines }.forEachIndexed { index, line ->
        line.oldNo?.let { anchors["left" to it] = index }
        line.newNo?.let { anchors["right" to it] = index }
    }
    val fileLevel = mutableListOf<PullRequestDiffAnnotation>()
    val byLine = LinkedHashMap<Int, MutableList<PullRequestDiffAnnotation>>()
    for (thread in threads) {
        if (thread.isOutdated || thread.path != file.path) continue
        val first = thread.comments.firstOrNull()
        val annotation =
            PullRequestDiffAnnotation(
                id = "discussion:${thread.id}",
                title = (first?.author?.login ?: "Discussion") + if (thread.isResolved) " · Resolved" else "",
                body = first?.body.orEmpty(),
                footer = "${thread.commentCount ?: thread.comments.size} comments · Tap to open",
            )
        val line = thread.line
        if (line == null) {
            fileLevel += annotation
        } else {
            anchors[thread.side to line]?.let { byLine.getOrPut(it) { mutableListOf() } += annotation }
        }
    }
    for (draft in drafts) {
        if (draft.headSha != headSha || draft.path != file.path) continue
        val index = anchors[draft.position.diffSide to draft.position.line] ?: continue
        byLine.getOrPut(index) { mutableListOf() } +=
            PullRequestDiffAnnotation(
                id = "draft:${draft.id}",
                title = "Pending review",
                body = draft.body,
                footer = "Tap to edit",
            )
    }
    return PullRequestFileAnnotations(fileLevel, byLine)
}

/** A discussion's comments merged with the pages loaded since, oldest first, one per id. */
fun mergeDiscussionComments(
    loaded: List<PullRequestThreadCommentDto>,
    extra: List<PullRequestThreadCommentDto>,
): List<PullRequestThreadCommentDto> =
    (loaded + extra).distinctBy { it.id }.sortedBy { parseInstant(it.createdAt) ?: 0L }

/* ── Diff pages ──────────────────────────────────────────────────────── */

data class PullRequestDiffPage(val cursor: String?, val result: PullRequestDiffResultDto)

/** `retainPullRequestDiffPage`: a changed page drops the later pages, whose cursors belong to the old patch. */
fun retainPullRequestDiffPage(
    pages: List<PullRequestDiffPage>,
    cursor: String?,
    result: PullRequestDiffResultDto,
): List<PullRequestDiffPage> {
    val index = pages.indexOfFirst { it.cursor == cursor }
    if (index == -1) return pages + PullRequestDiffPage(cursor, result)
    val previous = pages[index].result
    if (previous == result) return pages
    return pages.take(index) + PullRequestDiffPage(cursor, result)
}

const val PULL_REQUEST_DIFF_NOTICE =
    "Some file contents are unavailable. Open the pull request on its provider to see the full changes."

data class PullRequestCombinedDiff(val files: List<ReviewFile>, val notice: String?)

/**
 * `combinePullRequestDiffPages`, parsing each page's patch. A withheld file's
 * patch is only its header, so its line counts come from the page's
 * `omittedFileStats` rather than reading as +0 −0.
 */
fun combinePullRequestDiffPages(
    pages: List<PullRequestDiffPage>,
    parse: (String) -> List<ReviewFile> = ::parseUnifiedDiff,
): PullRequestCombinedDiff {
    val parsed = pages.map { page ->
        val files = parse(page.result.patch)
        val stats = page.result.omittedFileStats.orEmpty().associateBy { it.path }
        if (stats.isEmpty()) files
        else files.map { file -> stats[file.path]?.let { file.copy(additions = it.additions, deletions = it.deletions) } ?: file }
    }
    val files = parsed.flatten()
    val unparsed = pages.indices.any { parsed[it].isEmpty() && pages[it].result.patch.isNotBlank() }
    return PullRequestCombinedDiff(
        files = files,
        notice = if (pages.any { it.result.truncated } || unparsed) PULL_REQUEST_DIFF_NOTICE else null,
    )
}

/* ── Review sheet ────────────────────────────────────────────────────── */

/** Forgejo refuses a change request review without a summary, even with inline comments. */
fun reviewSummaryRequired(provider: String?, verdict: String?): Boolean =
    provider == "forgejo" && verdict == "request-changes"

/** The verdicts a review may offer, in the sheet's order. */
val PULL_REQUEST_REVIEW_VERDICTS: List<Pair<String, String>> =
    listOf("comment" to "Comment", "approve" to "Approve", "request-changes" to "Request changes")

/** Whether the review sheet's submit button may run (`PullRequestReviewSheet`). */
fun canSubmitPullRequestReview(
    reviewing: Boolean,
    draft: ReviewDraft,
    body: String,
    verdict: String?,
    headSha: String?,
    provider: String?,
    permissions: PullRequestPermissions,
): Boolean {
    val canSubmitComments = draft.comments.isEmpty() || permissions.canInlineComment
    if (reviewing && (draft.isStale(headSha) || !canSubmitComments)) return false
    if (!reviewing) return permissions.canComment && body.isNotBlank()
    if (verdict == null) return false
    return if (reviewSummaryRequired(provider, verdict)) {
        body.isNotBlank()
    } else {
        verdict == "approve" || body.isNotBlank() || draft.comments.isNotEmpty()
    }
}
