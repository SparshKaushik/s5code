package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.PullRequestActorDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestCapabilitiesDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestCheckDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestChecksDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDetailDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestEditCapabilitiesDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReactionDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestViewerPermissionsDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

/** Mirrors client-runtime's pullRequestEditing, pullRequestReactions and pullRequestChecks tests. */
class PullRequestEditingTest {

    private fun detail(
        edit: PullRequestEditCapabilitiesDto? = PullRequestEditCapabilitiesDto(changeRequest = true, comment = true),
        permissions: PullRequestViewerPermissionsDto = PullRequestViewerPermissionsDto(),
        labels: Boolean? = true,
        viewer: String? = "Octocat",
        author: String? = "octocat",
    ) = PullRequestDetailDto(
        capabilities = PullRequestCapabilitiesDto(edit = edit, labels = labels),
        viewerPermissions = permissions,
        viewer = viewer,
        author = author?.let { PullRequestActorDto(login = it) },
    )

    @Test
    fun `the author or a merger edits the PR unless the host says otherwise`() {
        assertTrue(canEditPullRequestChangeRequest(detail()))
        assertFalse(canEditPullRequestChangeRequest(detail(viewer = "someone")))
        assertTrue(
            canEditPullRequestChangeRequest(detail(viewer = "someone", permissions = PullRequestViewerPermissionsDto(actions = listOf("merge"))))
        )
        assertFalse(canEditPullRequestChangeRequest(detail(permissions = PullRequestViewerPermissionsDto(editChangeRequest = false))))
        assertFalse(canEditPullRequestChangeRequest(detail(edit = null)))
    }

    @Test
    fun `only issue and review comments are editable, by their author unless the host answers`() {
        val pr = detail()
        assertTrue(canEditPullRequestComment(pr, "issue-comment", "OCTOCAT ", null))
        assertFalse(canEditPullRequestComment(pr, "review", "octocat", true))
        assertFalse(canEditPullRequestComment(pr, "review-comment", "someone", null))
        assertTrue(canEditPullRequestComment(pr, "review-comment", "someone", true))
        assertFalse(canEditPullRequestComment(pr, "issue-comment", "octocat", false))
        assertFalse(canEditPullRequestComment(detail(edit = PullRequestEditCapabilitiesDto(changeRequest = true)), "issue-comment", "octocat", true))
    }

    @Test
    fun `labels are manageable unless the viewer is refused`() {
        assertTrue(canManagePullRequestLabels(detail()))
        assertFalse(canManagePullRequestLabels(detail(permissions = PullRequestViewerPermissionsDto(labels = false))))
        assertFalse(canManagePullRequestLabels(detail(labels = null)))
    }

    @Test
    fun `permissions need a connected session allowed to write`() {
        val pr = detail()
        val granted = pullRequestPermissions(pr, connected = true, canWriteSourceControl = true)
        assertTrue(granted.canEditChangeRequest && granted.canManageLabels)
        assertTrue(granted.canEditComment("issue-comment", "octocat", null))
        val offline = pullRequestPermissions(pr, connected = false, canWriteSourceControl = true)
        assertFalse(offline.canEditChangeRequest || offline.canManageLabels || offline.canReact)
        assertFalse(offline.canEditComment("issue-comment", "octocat", true))
    }

    /* ── Reactions ───────────────────────────────────────────────────── */

    private fun reaction(content: String = "heart", count: Int = 1, actors: List<String> = listOf("octocat"), mine: Boolean = false) =
        PullRequestReactionDto(content, count, actors, mine)

    @Test
    fun `adds a reaction nobody had yet, in picker order`() {
        assertEquals(
            listOf(reaction("thumbs-up", 1, emptyList(), true), reaction("rocket", 2, listOf("a", "b"))),
            applyPendingPullRequestReactions(listOf(reaction("rocket", 2, listOf("a", "b"))), mapOf("thumbs-up" to true)),
        )
    }

    @Test
    fun `joins and leaves an existing reaction, and drops the pill nobody is left on`() {
        assertEquals(
            listOf(reaction(count = 3, actors = listOf("a", "b"), mine = true)),
            applyPendingPullRequestReactions(listOf(reaction(count = 2, actors = listOf("a", "b"))), mapOf("heart" to true)),
        )
        assertEquals(
            listOf(reaction(count = 1, actors = listOf("a", "b"))),
            applyPendingPullRequestReactions(listOf(reaction(count = 2, actors = listOf("a", "b"), mine = true)), mapOf("heart" to false)),
        )
        assertEquals(emptyList<PullRequestReactionDto>(), applyPendingPullRequestReactions(listOf(reaction(mine = true)), mapOf("heart" to false)))
    }

    @Test
    fun `ignores a pending state the host has already caught up with`() {
        val reactions = listOf(reaction(count = 3, mine = true))
        assertEquals(reactions, applyPendingPullRequestReactions(reactions, mapOf("heart" to true)))
        assertEquals(emptyList<PullRequestReactionDto>(), applyPendingPullRequestReactions(emptyList(), mapOf("heart" to false)))
    }

    @Test
    fun `names the viewer only when the count has room for them`() {
        assertEquals(
            "You and octocat reacted with heart emoji",
            pullRequestReactionTooltip(reaction(count = 2, mine = true)),
        )
        assertEquals("octocat reacted with heart emoji", pullRequestReactionTooltip(reaction(count = 1, mine = true)))
        assertEquals(
            "a, b, c, and 2 others reacted with thumbs up emoji",
            pullRequestReactionTooltip(reaction("thumbs-up", 5, listOf("a", "b", "c", "d"))),
        )
        assertEquals("3 people reacted with eyes emoji", pullRequestReactionTooltip(reaction("eyes", 3, emptyList())))
    }

    /* ── Live checks ─────────────────────────────────────────────────── */

    private val pending = PullRequestCheckDto(name = "build", status = "pending")
    private val passed = pending.copy(status = "success")

    @Test
    fun `fresh checks replace the detail's checks and state, and nothing else`() {
        val original = PullRequestDetailDto(title = "Cache the title", checks = listOf(pending))
        val checks = PullRequestChecksDto(state = "closed", checks = listOf(passed))
        assertEquals(original.copy(state = "closed", checks = listOf(passed)), resolvePullRequestDetailChecks(original, checks, true))
        assertSame(original, resolvePullRequestDetailChecks(original, checks, false))
        assertSame(original, resolvePullRequestDetailChecks(original, null, true))
        assertNull(resolvePullRequestDetailChecks(null, checks, true))
    }

    @Test
    fun `the newer observation wins when both carry one, and merged stays merged`() {
        val cached = PullRequestDetailDto(observedAt = 1_000.0, checks = listOf(pending))
        val checks = PullRequestChecksDto(observedAt = 2_000.0, checks = listOf(passed))
        assertEquals(listOf(passed), resolvePullRequestDetailChecks(cached, checks, false)?.checks)
        val refreshed = cached.copy(observedAt = 3_000.0)
        assertSame(refreshed, resolvePullRequestDetailChecks(refreshed, checks, true))
        val merged = PullRequestDetailDto(state = "merged", observedAt = 1_000.0)
        assertEquals("merged", resolvePullRequestDetailChecks(merged, checks, true)?.state)
    }

    @Test
    fun `running checks poll tighter than a settled head`() {
        assertEquals(45_000L, pullRequestChecksRefreshIntervalMs(emptyList()))
        assertEquals(45_000L, pullRequestChecksRefreshIntervalMs(listOf(passed, pending)))
        assertEquals(45_000L, pullRequestChecksRefreshIntervalMs(listOf(passed.copy(status = "action-required"))))
        assertEquals(60_000L, pullRequestChecksRefreshIntervalMs(listOf(passed, passed.copy(status = "failure"))))
    }

    @Test
    fun `GitLab says merge request`() {
        assertEquals("merge request", changeRequestTerms("gitlab").singular)
        assertEquals("MR", changeRequestTerms("gitlab").shortLabel)
        assertEquals("Pull request", changeRequestTerms("forgejo").title)
    }
}
