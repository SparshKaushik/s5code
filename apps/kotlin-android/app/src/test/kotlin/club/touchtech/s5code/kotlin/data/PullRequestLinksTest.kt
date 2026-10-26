package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.RepositoryIdentity
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Mirrors client-runtime's `changeRequestLinks.test.ts`. */
class PullRequestLinksTest {

    private fun project(environmentId: String, projectId: String, key: String, provider: String = "github") =
        ChangeRequestLinkProject(
            environmentId,
            projectId,
            RepositoryIdentity(
                canonicalKey = key,
                displayName = key.substringAfter('/'),
                provider = provider,
                remoteUrl = "git@${key.substringBefore('/')}:${key.substringAfter('/')}.git",
            ),
        )

    private val projects = listOf(project("env-a", "repo", "github.com/org/repo"), project("env-b", "other", "github.com/org/other"))
    private val hostReads: (String) -> ChangeRequestLinkCapabilities? = { ChangeRequestLinkCapabilities(true, true) }

    @Test
    fun `opens a link under the project whose repository holds it`() {
        assertEquals(
            ChangeRequestLinkTarget("env-b", PullRequestReference("other", "org/other", 12, "github.com")),
            resolveChangeRequestLinkTarget("https://github.com/org/other/pull/12", projects, hostReads),
        )
    }

    @Test
    fun `lends a project on the host to a repository nobody checked out, as web does`() {
        assertEquals(
            ChangeRequestLinkTarget("env-a", PullRequestReference("repo", "someone/else", 3, "github.com")),
            resolveChangeRequestLinkTarget("https://github.com/someone/else/pull/3", projects, hostReads, environmentId = "env-a"),
        )
        assertNull(
            resolveChangeRequestLinkTarget(
                "https://github.com/someone/else/pull/3",
                projects,
                { ChangeRequestLinkCapabilities(pullRequests = true, threadPullRequests = false) },
            )
        )
        assertNull(resolveChangeRequestLinkTarget("https://gitlab.com/org/repo/-/merge_requests/3", projects, hostReads))
    }

    @Test
    fun `leaves the link to the browser when the environment cannot show it`() {
        assertNull(resolveChangeRequestLinkTarget("https://github.com/org/repo/pull/12", projects, { null }))
        assertNull(resolveChangeRequestLinkTarget("https://github.com/org/repo/issues/12", projects, hostReads))
    }

    @Test
    fun `an older server takes the project's own repository selector`() {
        assertEquals(
            ChangeRequestLinkTarget("env-a", PullRequestReference("repo", "org/repo", 9)),
            resolveChangeRequestLinkTarget(
                "https://github.com/ORG/REPO/pull/9",
                projects,
                { ChangeRequestLinkCapabilities(pullRequests = true, threadPullRequests = false) },
            ),
        )
    }

    @Test
    fun `Azure DevOps never lends a project to another repository`() {
        val azure = listOf(project("env-a", "web", "dev.azure.com/org/proj/_git/web", provider = "azure-devops"))
        assertNull(resolveChangeRequestLinkTarget("https://dev.azure.com/org/proj/_git/api/pullrequest/4", azure, hostReads))
        assertEquals(
            "web",
            resolveChangeRequestLinkTarget("https://dev.azure.com/org/proj/_git/web/pullrequest/4", azure, hostReads)?.reference?.projectId,
        )
    }
}
