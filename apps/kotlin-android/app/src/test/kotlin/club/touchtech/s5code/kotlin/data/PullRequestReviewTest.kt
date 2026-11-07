package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.DiffLine
import club.touchtech.s5code.kotlin.model.DiffLineKind
import club.touchtech.s5code.kotlin.transport.wire.PullRequestActorDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDiffResultDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestOmittedFileStatDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewThreadDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestThreadCommentDto
import java.nio.file.Files
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.job
import kotlinx.coroutines.joinAll
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Ports of the mobile `pullRequestFileFilters`, `pullRequestDiffComments`,
 * `pullRequestReviewDrafts` and `pullRequestDiffPages` tests, plus the review
 * sheet's submit rule.
 */
class PullRequestReviewTest {

    /* ── Test files ──────────────────────────────────────────────────── */

    @Test
    fun `recognizes test names, directories and snapshots across languages`() {
        val tests =
            listOf(
                "src/app.test.ts", "src/app.spec.tsx", "src/__tests__/app.ts", "tests/helpers/fixtures.json",
                "test/app.js", "e2e/login.ts", "src/__snapshots__/app.snap", "src/app.test.ts.snap", "test_app.py",
                "app_test.py", "internal/app_test.go", "app_test.cpp", "spec/app_spec.rb", "AppTests.swift",
                "AppTest.kt", "AppSpec.java", "AppTests.cs", "src\\__tests__\\app.ts",
            )
        assertEquals(tests, tests.filter(::isPullRequestTestFile))
        val sources =
            listOf(
                "src/testing.ts", "src/contest.ts", "src/testUtils.ts", "latest/app.ts", "tests.ts",
                "specification.md", "AppTest.ts", "pnpm-lock.yaml",
            )
        assertEquals(emptyList<String>(), sources.filter(::isPullRequestTestFile))
    }

    private fun patch(path: String) = "diff --git a/$path b/$path\n--- a/$path\n+++ b/$path\n@@ -1 +1 @@\n-old\n+new\n"

    @Test
    fun `hides whole test files in order, keeping a requested one`() {
        val files =
            parseUnifiedDiff(listOf("src/first.test.ts", "src/first.ts", "src/__tests__/second.ts", "src/second.ts").joinToString("\n", transform = ::patch))
        assertEquals(listOf("src/first.ts", "src/second.ts"), filterPullRequestTestFiles(files, true).map { it.path })
        assertEquals(
            listOf("src/first.test.ts", "src/first.ts", "src/second.ts"),
            filterPullRequestTestFiles(files, true, includePath = "src/first.test.ts").map { it.path },
        )
        assertEquals(files, filterPullRequestTestFiles(files, false))
    }

    @Test
    fun `parsed diffs keep a renamed file's old path`() {
        val renamed =
            parseUnifiedDiff(
                "diff --git a/src/old.ts b/src/new.ts\nsimilarity index 90%\nrename from src/old.ts\nrename to src/new.ts\n" +
                    "--- a/src/old.ts\n+++ b/src/new.ts\n@@ -1 +1 @@\n-old\n+new\n"
            ).single()
        assertEquals("src/new.ts", renamed.path)
        assertEquals("src/old.ts", renamed.oldPath)
        assertNull(parseUnifiedDiff(patch("src/same.ts")).single().oldPath)
    }

    /* ── Positions and annotations ───────────────────────────────────── */

    @Test
    fun `places line comments on the side the tapped row belongs to`() {
        assertEquals(PullRequestReviewPosition("added", newLine = 4), reviewPosition(DiffLine(DiffLineKind.Added, "+a", null, 4)))
        val deleted = reviewPosition(DiffLine(DiffLineKind.Removed, "-a", 3, null))!!
        assertEquals("left", deleted.diffSide)
        assertEquals("Old line 3", deleted.label)
        val context = reviewPosition(DiffLine(DiffLineKind.Context, " a", 7, 9))!!
        assertEquals(9, context.line)
        assertEquals("right", context.diffSide)
        assertEquals(7, context.copy(side = "left").line)
        assertNull(reviewPosition(DiffLine(DiffLineKind.Context, "\\ No newline at end of file", null, null)))
    }

    private val file =
        parseUnifiedDiff(
            "diff --git a/src/app.ts b/src/app.ts\n--- a/src/app.ts\n+++ b/src/app.ts\n@@ -1,2 +1,2 @@\n-old one\n+new one\n same\n"
        ).single()

    private fun thread(id: String, line: Int?, side: String = "right", outdated: Boolean = false, path: String = "src/app.ts") =
        PullRequestReviewThreadDto(
            id = id,
            path = path,
            line = line,
            side = side,
            isOutdated = outdated,
            comments = listOf(PullRequestThreadCommentDto(id = "$id-c", author = PullRequestActorDto(login = "ana"), body = "Look")),
        )

    private fun pending(id: String, position: PullRequestReviewPosition, headSha: String = "head") =
        PendingReviewComment(id, "src/app.ts", position = position, body = "Draft $id", headSha = headSha, code = "")

    @Test
    fun `anchors old and new lines separately despite identical numbers`() {
        // Flat lines: 0 = -old one (old 1), 1 = +new one (new 1), 2 = same (old 2, new 2).
        val annotations = annotatePullRequestFile(file, listOf(thread("left", 1, "left"), thread("right", 1)), emptyList(), "head")
        assertEquals(listOf("discussion:left"), annotations.lines[0]?.map { it.id })
        assertEquals(listOf("discussion:right"), annotations.lines[1]?.map { it.id })
        assertEquals("ana", annotations.lines[0]?.single()?.title)
    }

    @Test
    fun `keeps outdated, unloaded and other files' threads off the code`() {
        val annotations =
            annotatePullRequestFile(
                file,
                listOf(thread("old", 1, outdated = true), thread("far", 99), thread("other", 1, path = "src/other.ts")),
                emptyList(),
                "head",
            )
        assertTrue(annotations.isEmpty)
    }

    @Test
    fun `shows file-level threads and only drafts written on the loaded revision`() {
        val annotations =
            annotatePullRequestFile(
                file,
                listOf(thread("file", null)),
                listOf(
                    pending("current", PullRequestReviewPosition("context", oldLine = 2, newLine = 2, side = "right")),
                    pending("stale", PullRequestReviewPosition("added", newLine = 1), headSha = "older"),
                ),
                "head",
            )
        assertEquals(listOf("discussion:file"), annotations.file.map { it.id })
        assertEquals(mapOf(2 to listOf("draft:current")), annotations.lines.mapValues { (_, list) -> list.map { it.id } })
    }

    /* ── Drafts ──────────────────────────────────────────────────────── */

    @Test
    fun `a draft written on an older revision is stale`() {
        val draft = ReviewDraft(comments = listOf(pending("a", PullRequestReviewPosition("added", newLine = 1), headSha = "old")))
        assertTrue(draft.isStale("new"))
        assertFalse(draft.isStale("old"))
        assertFalse(ReviewDraft(summary = "Hi").isStale("new"))
    }

    @Test
    fun `clearing a submitted review keeps edits made while it was in flight`() {
        val first = pending("a", PullRequestReviewPosition("added", newLine = 1))
        val second = pending("b", PullRequestReviewPosition("added", newLine = 1))
        val submitted = ReviewDraft(summary = "Ship it", comments = listOf(first))
        // While submitting, the summary changed, a comment was added and the first one moved.
        val moved = first.copy(position = PullRequestReviewPosition("context", oldLine = 2, newLine = 2, side = "right"))
        val current = ReviewDraft(summary = "Ship it!", comments = listOf(moved, second))
        assertEquals(ReviewDraft(summary = "Ship it!", comments = listOf(moved, second)), clearSubmittedReview(current, submitted))
        assertTrue(clearSubmittedReview(submitted, submitted).isEmpty)
    }

    @Test
    fun `review keys keep identical numbers apart across repositories, hosts and environments`() {
        val reference = PullRequestReference("project", "acme/web", 1, "github.com")
        val key = pullRequestReviewKey("laptop", reference)
        assertNotEquals(key, pullRequestReviewKey("laptop", reference.copy(repository = "acme/api")))
        assertNotEquals(key, pullRequestReviewKey("laptop", reference.copy(host = "ghe.acme.test")))
        assertNotEquals(key, pullRequestReviewKey("server", reference))
    }

    @Test
    fun `the draft store round trips and sets a damaged document aside`() = runBlocking {
        val directory = Files.createTempDirectory("review-drafts").toFile()
        val file = directory.resolve("drafts.json")
        val comment =
            pending("a", PullRequestReviewPosition("deleted", oldLine = 3)).copy(path = "src/new.ts", oldPath = "src/old.ts")
        val draft = ReviewDraft(comment = "Separate PR comment", comments = listOf(comment))
        // Edits launch their own writes; join them so none lands after the damaged text below.
        val writes = Job(coroutineContext.job)
        val store = PullRequestReviewDraftStore(file, CoroutineScope(coroutineContext + writes))
        store.ready.first { it }
        store.update("key") { draft }
        store.update("empty") { ReviewDraft() }
        store.flush()
        val reread = PullRequestReviewDraftStore(file, this)
        reread.ready.first { it }
        assertEquals(mapOf("key" to draft), reread.drafts.value)

        writes.children.toList().joinAll()
        writes.complete()
        file.writeText("{not json")
        val damaged = PullRequestReviewDraftStore(file, this)
        damaged.ready.first { it }
        assertEquals(emptyMap<String, ReviewDraft>(), damaged.drafts.value)
        assertEquals("{not json", directory.resolve("drafts.json.damaged").readText())
        directory.deleteRecursively()
        Unit
    }

    /* ── Diff pages ──────────────────────────────────────────────────── */

    @Test
    fun `adds pages in order and drops later ones when a refreshed page changes`() {
        val first = PullRequestDiffResultDto(patch = patch("a.ts"), nextCursor = "2")
        val second = PullRequestDiffResultDto(patch = patch("b.ts"))
        val pages = retainPullRequestDiffPage(retainPullRequestDiffPage(emptyList(), null, first), "2", second)
        assertEquals(listOf("a.ts", "b.ts"), combinePullRequestDiffPages(pages).files.map { it.path })
        assertEquals(pages, retainPullRequestDiffPage(pages, null, first))
        val refreshed = first.copy(patch = patch("c.ts"))
        assertEquals(listOf(PullRequestDiffPage(null, refreshed)), retainPullRequestDiffPage(pages, null, refreshed))
    }

    @Test
    fun `notes truncated or unparsable pages`() {
        assertNull(combinePullRequestDiffPages(listOf(PullRequestDiffPage(null, PullRequestDiffResultDto(patch("a.ts"))))).notice)
        assertEquals(
            PULL_REQUEST_DIFF_NOTICE,
            combinePullRequestDiffPages(listOf(PullRequestDiffPage(null, PullRequestDiffResultDto(patch("a.ts"), truncated = true)))).notice,
        )
        assertEquals(
            PULL_REQUEST_DIFF_NOTICE,
            combinePullRequestDiffPages(listOf(PullRequestDiffPage(null, PullRequestDiffResultDto("Binary files differ")))).notice,
        )
    }

    private fun withheldDiff(additions: Int = 3000, deletions: Int = 1800) =
        PullRequestDiffResultDto(
            patch = "diff --git a/large.ts b/large.ts\n--- a/large.ts\n+++ b/large.ts\n",
            truncated = true,
            omittedFileStats = listOf(PullRequestOmittedFileStatDto("large.ts", additions, deletions)),
        )

    @Test
    fun `withheld hunks keep the provider's line counts`() {
        val combined = combinePullRequestDiffPages(listOf(PullRequestDiffPage(null, withheldDiff())))
        val file = combined.files.single()
        assertEquals("large.ts", file.path)
        assertEquals(3000, file.additions)
        assertEquals(1800, file.deletions)
        assertTrue(file.hunks.isEmpty())
        assertEquals(PULL_REQUEST_DIFF_NOTICE, combined.notice)
    }

    @Test
    fun `a stats-only refresh replaces the withheld page`() {
        val first = PullRequestDiffResultDto(patch = patch("first.ts"), nextCursor = "second")
        val pages = retainPullRequestDiffPage(retainPullRequestDiffPage(emptyList(), null, first), "second", withheldDiff())
        assertEquals(
            listOf(Triple("first.ts", 1, 1), Triple("large.ts", 3000, 1800)),
            combinePullRequestDiffPages(pages).files.map { Triple(it.path, it.additions, it.deletions) },
        )
        val refreshed = retainPullRequestDiffPage(pages, "second", withheldDiff(4000, 2000))
        assertNotEquals(pages, refreshed)
        assertEquals(
            listOf(Triple("first.ts", 1, 1), Triple("large.ts", 4000, 2000)),
            combinePullRequestDiffPages(refreshed).files.map { Triple(it.path, it.additions, it.deletions) },
        )
    }

    /* ── Review sheet ────────────────────────────────────────────────── */

    private val reviewer = PullRequestPermissions(connected = true, canWrite = true, canComment = true, canInlineComment = true)

    private fun canSubmit(
        verdict: String?,
        body: String = "",
        draft: ReviewDraft = ReviewDraft(),
        provider: String = "github",
        reviewing: Boolean = true,
        permissions: PullRequestPermissions = reviewer,
    ) = canSubmitPullRequestReview(reviewing, draft, body, verdict, "head", provider, permissions)

    @Test
    fun `a review needs a verdict and something to say unless it approves`() {
        assertFalse(canSubmit(null, "Hi"))
        assertTrue(canSubmit("approve"))
        assertFalse(canSubmit("comment"))
        assertTrue(canSubmit("comment", "Hi"))
        val withComment = ReviewDraft(comments = listOf(pending("a", PullRequestReviewPosition("added", newLine = 1))))
        assertTrue(canSubmit("request-changes", draft = withComment))
    }

    @Test
    fun `Forgejo change requests need a summary, and stale drafts block a review`() {
        val withComment = ReviewDraft(comments = listOf(pending("a", PullRequestReviewPosition("added", newLine = 1))))
        assertFalse(canSubmit("request-changes", draft = withComment, provider = "forgejo"))
        assertTrue(canSubmit("request-changes", body = "Fix it", provider = "forgejo"))
        val stale = ReviewDraft(comments = listOf(pending("a", PullRequestReviewPosition("added", newLine = 1), headSha = "old")))
        assertFalse(canSubmit("approve", draft = stale))
        assertFalse(canSubmit("approve", draft = withComment, permissions = reviewer.copy(canInlineComment = false)))
    }

    @Test
    fun `comment mode needs the comment permission and a body`() {
        assertTrue(canSubmit(null, "Hi", reviewing = false))
        assertFalse(canSubmit(null, " ", reviewing = false))
        assertFalse(canSubmit(null, "Hi", reviewing = false, permissions = reviewer.copy(canComment = false)))
    }
}
