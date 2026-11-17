package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.PullRequestActorDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewerCandidateDto
import org.junit.Assert.assertEquals
import org.junit.Test

class PullRequestReviewersTest {

    private val ada = PullRequestReviewerCandidateDto(login = "ada", name = "Ada Lovelace", id = "1")
    private val bob = PullRequestReviewerCandidateDto(login = "bob", id = "2", isRequested = true)
    private val core = PullRequestReviewerCandidateDto(login = "org/core", name = "Core", id = "2", kind = "team")
    private val candidates = listOf(core, ada, bob)

    @Test
    fun `filters on login or name and lists requested reviewers first`() {
        assertEquals(listOf(bob, ada, core), filterReviewerCandidates(candidates, ""))
        assertEquals(listOf(ada), filterReviewerCandidates(candidates, "  LOVE "))
        assertEquals(listOf(core), filterReviewerCandidates(candidates, "core"))
    }

    @Test
    fun `matches a request to candidates by id and kind`() {
        assertEquals(listOf(core), selectedReviewerCandidates(candidates, listOf(PullRequestReviewerRequest("2", "team"))))
        assertEquals(listOf(bob), selectedReviewerCandidates(candidates, listOf(PullRequestReviewerRequest("2", "user"))))
    }

    @Test
    fun `marks only the selected candidates`() {
        assertEquals(listOf(core, ada.copy(isRequested = true), bob), patchReviewerCandidates(candidates, listOf(ada), true))
        assertEquals(listOf(core, ada, bob.copy(isRequested = false)), patchReviewerCandidates(candidates, listOf(bob), false))
    }

    @Test
    fun `adds new reviewers once and removes them unless kept`() {
        val existing = listOf(PullRequestActorDto(login = "Bob"))
        assertEquals(existing + ada.actor, patchRequestedReviewers(existing, listOf(ada, bob), requested = true))
        assertEquals(emptyList<PullRequestActorDto>(), patchRequestedReviewers(existing, listOf(bob), requested = false))
        assertEquals(existing, patchRequestedReviewers(existing, listOf(bob), requested = false, keep = { it.login == "Bob" }))
    }
}
