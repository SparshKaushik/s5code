package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.PullRequestActivityDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestActorDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestCheckDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestCommentDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestCommitDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewThreadDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestThreadCommentDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Ports of the mobile `pullRequestEnvironmentFallback`, `pullRequestActivityFilter`,
 * `botBadgeImages` and `pullRequestStatus` tests, plus the cache freshness
 * rule and timeline ordering the Kotlin gateway and Activity segment rely on.
 */
class PullRequestRulesTest {

    /* ── Environment fallback ────────────────────────────────────────── */

    private val projects =
        listOf(
            PullRequestProjectIdentity("laptop", "t3", "github.com/pingdotgg/t3code"),
            PullRequestProjectIdentity("desktop", "other", "github.com/pingdotgg/other"),
            PullRequestProjectIdentity("server", "t3-copy", "github.com/pingdotgg/t3code"),
        )

    private fun fallback(connected: Set<String>, host: String? = "github.com") =
        pullRequestFallbackEnvironment("laptop", "t3", "pingdotgg/t3code", host, projects, connected)

    @Test
    fun `moves to a connected copy of the repository when the original is down`() {
        assertEquals(PullRequestEnvironmentTarget("server", "t3-copy"), fallback(setOf("desktop", "server")))
    }

    @Test
    fun `stays put while the original is connected or nothing else holds the repository`() {
        assertNull(fallback(setOf("laptop", "server")))
        assertNull(fallback(setOf("desktop")))
    }

    @Test
    fun `keeps the requested environment while it is still connecting`() {
        assertNull(
            pullRequestFallbackEnvironment(
                "laptop", "t3", "pingdotgg/t3code", "github.com", projects,
                connectedEnvironmentIds = setOf("server"),
                pendingEnvironmentIds = setOf("laptop"),
            )
        )
    }

    @Test
    fun `derives the host from the original project when the route has none`() {
        assertEquals(PullRequestEnvironmentTarget("server", "t3-copy"), fallback(setOf("server"), host = null))
    }

    /* ── Activity filters ────────────────────────────────────────────── */

    private val date = "2026-10-11T00:00:00.000Z"
    private val human = PullRequestActorDto(login = "reader")
    private val bot = human.copy(login = "reviewer[bot]")

    private fun comment(id: String, author: PullRequestActorDto = human, reviewState: String? = null) =
        PullRequestCommentDto(id = id, author = author, reviewState = reviewState, body = "A comment", createdAt = date)

    private val thread =
        PullRequestReviewThreadDto(
            id = "thread",
            path = "file.ts",
            line = 1,
            isResolved = true,
            comments = listOf(PullRequestThreadCommentDto(id = "reply", author = human, body = "A comment", createdAt = date)),
        )

    private val activity =
        PullRequestActivityDto(
            comments = listOf(comment("human"), comment("bot", bot), comment("dismissed", human, "DISMISSED"), comment("reply")),
            commentCount = 4,
            commits = listOf(PullRequestCommitDto(oid = "abcdef", messageHeadline = "A change", committedDate = date)),
            reviewThreads = listOf(thread),
        )

    @Test
    fun `hides bots and finished conversations by default, counting each thread once`() {
        val result = filterPullRequestActivity(activity, PullRequestActivityFilter())
        assertEquals(listOf("human"), result.activity.comments.map { it.id })
        assertEquals(emptyList<PullRequestReviewThreadDto>(), result.activity.reviewThreads)
        assertEquals(1, result.activity.commits.size)
        assertEquals(3, result.hiddenCount)
    }

    @Test
    fun `restores bot comments and finished conversations without duplicate thread replies`() {
        val result = filterPullRequestActivity(activity, PullRequestActivityFilter(bots = true, resolved = true))
        assertEquals(listOf("human", "bot", "dismissed"), result.activity.comments.map { it.id })
        assertEquals(1, result.activity.reviewThreads.size)
        assertEquals(0, result.hiddenCount)
    }

    @Test
    fun `keeps partially loaded bot discussions visible`() {
        val botThread =
            thread.copy(
                isResolved = false,
                comments = listOf(PullRequestThreadCommentDto(id = "bot-reply", author = bot, createdAt = date)),
            )
        for (incomplete in listOf(botThread.copy(commentCount = 2), botThread.copy(nextCommentsCursor = "next"))) {
            val result = filterPullRequestActivity(activity.copy(reviewThreads = listOf(incomplete)), PullRequestActivityFilter())
            assertEquals(1, result.activity.reviewThreads.size)
        }
        assertTrue(
            filterPullRequestActivity(activity.copy(reviewThreads = listOf(botThread)), PullRequestActivityFilter())
                .activity.reviewThreads.isEmpty()
        )
    }

    @Test
    fun `provides a commits-only view and a comments-only view`() {
        val commits = filterPullRequestActivity(activity, PullRequestActivityFilter(kind = PullRequestActivityKind.Commits)).activity
        assertTrue(commits.comments.isEmpty())
        assertTrue(commits.reviewThreads.isEmpty())
        assertEquals(1, commits.commits.size)
        assertTrue(
            filterPullRequestActivity(activity, PullRequestActivityFilter(kind = PullRequestActivityKind.Comments)).activity.commits.isEmpty()
        )
    }

    /* ── Timeline ────────────────────────────────────────────────────── */

    @Test
    fun `orders comments, discussions and commits by date and leaves thread replies inside their discussion`() {
        val timeline =
            PullRequestActivityDto(
                comments = listOf(comment("late").copy(createdAt = "2026-10-12T00:00:00Z"), comment("reply")),
                commits = listOf(PullRequestCommitDto(oid = "early", committedDate = "2026-10-10T00:00:00Z")),
                reviewThreads = listOf(thread),
            )
        assertEquals(
            listOf("commit:early", "discussion:thread", "comment:late"),
            pullRequestTimeline(timeline, newestFirst = false).events.map { it.key },
        )
        assertEquals(
            listOf("comment:late", "discussion:thread", "commit:early"),
            pullRequestTimeline(timeline, newestFirst = true).events.map { it.key },
        )
        assertFalse(pullRequestTimeline(timeline, newestFirst = true).truncated)
        assertTrue(pullRequestTimeline(timeline.copy(reviewThreadsTruncated = true), newestFirst = true).truncated)
    }

    /* ── Bot badges ──────────────────────────────────────────────────── */

    @Test
    fun `matches bot badges but not screenshots hosted by the same services`() {
        val badges =
            listOf(
                "https://static.devin.ai/assets/gh-devin-review-light.svg?v=4",
                "https://cursor.com/assets/images/open-in-web-dark.png",
                "https://storage.googleapis.com/coderabbit_public_assets/review-stack-in-coderabbit-ui.svg",
                "https://img.shields.io/badge/P1-orange",
                "https://greptile-static-assets.s3.amazonaws.com/badges/ViewArtifacts.svg?v=4",
                "https://www.gstatic.com/codereviewagent/high-priority.svg",
                "https://user-images.githubusercontent.com/23264/106598434-9e719e00-654f-11eb-9e59-6167043cfa01.png",
            )
        val screenshots =
            listOf(
                "https://cursor.com/artifacts/c/art-41567e7e-97f5-46ff-9e6e-df6a85723409",
                "https://www.cursor.com/artifacts/c/art-98360ed1-44f8-4459-bea3-43bf93cc74fc",
                "https://app.devin.ai/attachments/abc/ss_1.png",
                "https://user-images.githubusercontent.com/23264/1-other-upload.png",
                "https://storage.googleapis.com/some-bucket/before.png",
            )
        assertEquals(badges, badges.filter(::isBotBadgeImage))
        assertEquals(emptyList<String>(), screenshots.filter(::isBotBadgeImage))
    }

    @Test
    fun `lists description images once each and leaves bot badges out of the attachments`() {
        val body =
            "![before](https://example.com/before.png) ![](https://img.shields.io/badge/P1-orange)\n\n" +
                "> ![before](https://example.com/before.png) ![after](https://example.com/after.png)"
        assertEquals(
            listOf(
                PullRequestAttachment("https://example.com/before.png", "before"),
                PullRequestAttachment("https://example.com/after.png", "after"),
            ),
            pullRequestAttachments(body),
        )
    }

    /* ── Review status ───────────────────────────────────────────────── */

    private val author = PullRequestActorDto(login = "julius")

    private fun review(state: String, at: String) =
        PullRequestCommentDto(id = "$state:$at", kind = "review", author = author, createdAt = at, reviewState = state)

    private fun reviews(vararg comments: PullRequestCommentDto) = PullRequestActivityDto(comments = comments.toList())

    @Test
    fun `uses each reviewer's latest verdict even when history arrives out of order`() {
        val result =
            pullRequestReviewSummary(
                emptyList(),
                reviews(review("CHANGES_REQUESTED", "2026-10-10T10:00:00Z"), review("APPROVED", "2026-10-10T09:00:00Z")),
            )
        assertEquals("Changes requested", result.title)
        assertEquals(1, result.reviews.size)
    }

    @Test
    fun `offers a review request only when one can be made`() {
        assertEquals("No reviewers", pullRequestReviewSummary(emptyList(), null).title)
        assertEquals("Request a review", pullRequestReviewSummary(emptyList(), null, canRequest = true).title)
        assertEquals("Awaiting review", pullRequestReviewSummary(listOf(author), null, canRequest = true).title)
    }

    @Test
    fun `does not claim a dismissed approval or an approval preceding the latest commit`() {
        val approved = review("APPROVED", "2026-10-10T09:00:00Z")
        assertTrue(pullRequestReviewSummary(emptyList(), reviews(approved, review("DISMISSED", "2026-10-10T10:00:00Z"))).reviews.isEmpty())
        val result =
            pullRequestReviewSummary(
                listOf(author),
                PullRequestActivityDto(
                    comments = listOf(approved),
                    commits = listOf(PullRequestCommitDto(oid = "abc123", messageHeadline = "Update", committedDate = "2026-10-10T10:00:00Z")),
                ),
            )
        assertEquals("Awaiting review", result.title)
    }

    @Test
    fun `keeps an approval when a later review only comments, and normalizes provider spelling`() {
        val result =
            pullRequestReviewSummary(
                emptyList(),
                reviews(review("approved", "2026-10-10T09:00:00Z"), review("COMMENTED", "2026-10-10T10:00:00Z")),
            )
        assertEquals("Approved by julius", result.title)
        assertEquals(1, result.reviews.size)
        assertEquals(
            "Changes requested",
            pullRequestReviewSummary(emptyList(), reviews(review("changes-requested", "2026-10-10T11:00:00Z"))).title,
        )
    }

    @Test
    fun `includes comment-only reviewers once without treating their comments as approval`() {
        val result =
            pullRequestReviewSummary(
                emptyList(),
                reviews(review("COMMENTED", "2026-10-10T09:00:00Z"), review("commented", "2026-10-10T10:00:00Z")),
            )
        assertEquals("Reviews", result.title)
        assertEquals(listOf(review("commented", "2026-10-10T10:00:00Z").copy(reviewState = "COMMENTED")), result.reviews)
    }

    /* ── Check status ────────────────────────────────────────────────── */

    private fun check(status: String) = PullRequestCheckDto(name = status, status = status)

    @Test
    fun `only says all passed when every check succeeded`() {
        assertEquals("All checks passed", pullRequestChecksSummary(listOf(check("success"), check("success"))).title)
        assertEquals("Checks completed", pullRequestChecksSummary(listOf(check("success"), check("skipped"))).title)
        assertEquals("No checks reported", pullRequestChecksSummary(emptyList()).title)
    }

    @Test
    fun `prioritizes failed checks while others are running`() {
        val summary = pullRequestChecksSummary(listOf(check("success"), check("pending"), check("failure")))
        assertEquals("1 check failing", summary.title)
        assertEquals("1 of 3", summary.count)
    }

    /* ── Conflicts ───────────────────────────────────────────────────── */

    @Test
    fun `names the base branch of an open pull request with conflicts`() {
        assertEquals("Conflicts with main", pullRequestConflict("open", false, "conflicting", "main"))
        assertEquals("Has conflicts", pullRequestConflict("open", false, "conflicting", null))
    }

    @Test
    fun `stays quiet when it cannot conflict or has not been reported`() {
        assertNull(pullRequestConflict("open", true, "conflicting", null))
        assertNull(pullRequestConflict("merged", false, "conflicting", null))
        assertNull(pullRequestConflict("closed", false, "conflicting", null))
        assertNull(pullRequestConflict("open", false, "mergeable", null))
        assertNull(pullRequestConflict("open", false, "unknown", null))
        assertNull(pullRequestConflict("open", false, null, null))
    }

    /* ── Cache freshness ─────────────────────────────────────────────── */

    @Test
    fun `refetches a missing read, an expired one, and one the server's revision has moved past`() {
        assertTrue(isPullRequestQueryStale(null, 0, 30_000, null, null))
        assertFalse(isPullRequestQueryStale(1_000, 30_999, 30_000, 1, 1))
        assertTrue(isPullRequestQueryStale(1_000, 31_000, 30_000, 1, 1))
        assertTrue(isPullRequestQueryStale(1_000, 2_000, 30_000, 1, 2))
    }

    @Test
    fun `judges a read made before the first revision by age alone`() {
        assertFalse(isPullRequestQueryStale(1_000, 2_000, 30_000, null, 3))
        assertFalse(isPullRequestQueryStale(1_000, 2_000, 30_000, 2, null))
    }

    @Test
    fun `a merged pull request's reads still expire, so a reopened or edited PR is not frozen`() {
        // Upstream only treats the linked summary as final; detail, activity and stack keep their TTL.
        assertTrue(isPullRequestQueryStale(1_000, 999_999, 60_000, 1, 1))
        assertTrue(isPullRequestQueryStale(1_000, 2_000, 60_000, 1, 9))
    }
}
