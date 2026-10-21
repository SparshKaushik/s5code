package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.RepositoryIdentity
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Port of client-runtime's `pullRequestLinkInput` tests. */
class PullRequestLinkInputTest {
    private val project =
        PullRequestLinkProject("github.com", "acme/web") { number -> changeRequestUrlFor("github", "github.com", "acme/web", number) }

    private fun link(result: PullRequestLinkInputResult?) = (result as PullRequestLinkInputResult.Link).link

    private fun error(result: PullRequestLinkInputResult?) = (result as PullRequestLinkInputResult.Error).message

    @Test
    fun `links supported host URLs without a thread project`() {
        for ((url, host) in
            listOf(
                "https://bitbucket.org/acme/web/pull-requests/42" to "bitbucket.org",
                "https://github.acme.test/acme/web/pull/42" to "github.acme.test",
                "https://git.acme.test/acme/web/-/merge_requests/42" to "git.acme.test",
            )) {
            assertEquals(
                ResolvedPullRequestLink(host, "acme/web", 42, url),
                link(resolveLinkPullRequestInput(" $url ", null) { it.host == host }),
            )
        }
    }

    @Test
    fun `resolves a bare number against the thread's own repository`() {
        val expected = ResolvedPullRequestLink("github.com", "acme/web", 42, "https://github.com/acme/web/pull/42")
        assertEquals(expected, link(resolveLinkPullRequestInput("#42", project)))
        assertEquals(expected, link(resolveLinkPullRequestInput("42", project)))
    }

    @Test
    fun `links a URL from another repository on a host with a project`() {
        assertEquals(
            ResolvedPullRequestLink("github.com", "acme/api", 7, "https://github.com/acme/api/pull/7"),
            link(resolveLinkPullRequestInput("https://github.com/acme/api/pull/7", project) { it.host == "github.com" }),
        )
    }

    @Test
    fun `refuses a URL no project can read, and a bare number without a project`() {
        assertTrue(
            error(resolveLinkPullRequestInput("https://gitlab.com/acme/api/-/merge_requests/7", project) { false })
                .contains("gitlab.com")
        )
    }

    @Test
    fun `names the missing repository when resolving a bare number`() {
        for (reference in listOf("12", "#12")) {
            assertEquals(
                "Paste the full pull request URL; this thread has no repository to resolve the number against.",
                error(resolveLinkPullRequestInput(reference, null)),
            )
        }
    }

    @Test
    fun `returns null for input that names no pull request`() {
        assertNull(resolveLinkPullRequestInput("hello", project))
        assertNull(resolveLinkPullRequestInput("", project))
        assertNull(resolveLinkPullRequestInput("#0", project))
    }

    @Test
    fun `accepts a host CLI checkout command`() {
        val resolved = link(resolveLinkPullRequestInput("gh pr checkout https://github.com/acme/web/pull/3", project))
        assertEquals(3, resolved.number)
        assertEquals("acme/web", resolved.repository)
        assertEquals("15", parsePullRequestReference("glab mr checkout 15"))
        assertEquals("9", parsePullRequestReference("az repos pr checkout --id 9"))
    }

    @Test
    fun `builds the project from its repository identity`() {
        val github =
            pullRequestLinkProject(RepositoryIdentity(canonicalKey = "github.com/acme/web", displayName = "acme/web", provider = "github"))
        assertEquals("github.com", github?.host)
        assertEquals("https://github.com/acme/web/pull/5", github?.webUrl?.invoke(5))
        val unknown = pullRequestLinkProject(RepositoryIdentity(canonicalKey = "git.example/acme/web", displayName = "acme/web"))
        assertNull(unknown?.webUrl?.invoke(5))
        assertTrue(error(resolveLinkPullRequestInput("#5", unknown)).contains("no known pull request URL"))
        assertNull(pullRequestLinkProject(RepositoryIdentity(canonicalKey = "github.com/acme/web")))
    }

    @Test
    fun `knows each host's pull request page`() {
        assertEquals("https://gitlab.com/g/sub/repo/-/merge_requests/5", changeRequestUrlFor("gitlab", "gitlab.com", "g/sub/repo", 5))
        assertEquals(
            "http://forge.test:3000/acme/web/pulls/5",
            changeRequestUrlFor("forgejo", "forge.test:3000", "acme/web", 5, remoteUrl = "http://forge.test:3000/acme/web.git"),
        )
        assertNull(changeRequestUrlFor("unknown", "x", "a/b", 1))
    }
}
