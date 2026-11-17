package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.data.changeRequestUrlFor
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Port of the shared `changeRequestUrl` tests for host routing and Azure DevOps Server. */
class ChangeRequestUrlTest {

    @Test
    fun `reads Azure DevOps Server links and keeps servers on different ports distinct`() {
        for (host in listOf("scm.example.test", "git.cafe", "code.bitbucket.example.test")) {
            val repository = "tfs/collection/project/_git/repo"
            val url = "https://${host.uppercase()}:8443/tfs/Collection/Project/_git/Repo/pullrequest/123"
            val other = url.replace(":8443/", ":9443/")
            assertEquals(host, ChangeRequestLink(host, repository, 123, "$host:8443"), parseChangeRequestUrl(url))
            assertEquals(host, parseChangeRequestUrl(url), parseChangeRequestUrl("$url?view=files"))
            assertEquals(
                host,
                listOf(
                    ThreadPullRequestKey("$host:8443", repository, 123),
                    ThreadPullRequestKey("$host:9443", repository, 123),
                ),
                listOf(url, other).map { normalizeThreadPullRequestKey(host, repository, 123, it) },
            )
        }
    }

    @Test
    fun `reads Bitbucket and both Azure DevOps URL forms`() {
        assertEquals(
            ChangeRequestLink("bitbucket.org", "acme/t3code", 9),
            parseChangeRequestUrl("https://bitbucket.org/acme/t3code/pull-requests/9"),
        )
        assertEquals(
            ChangeRequestLink("dev.azure.com", "acme/platform/_git/t3code", 17),
            parseChangeRequestUrl("https://dev.azure.com/acme/platform/_git/t3code/pullrequest/17"),
        )
        assertEquals(
            ChangeRequestLink("acme.visualstudio.com", "platform/_git/t3code", 17),
            parseChangeRequestUrl("https://acme.visualstudio.com/platform/_git/t3code/pullrequest/17"),
        )
    }

    @Test
    fun `git cafe pulls are not read as Forgejo`() {
        assertEquals(
            ChangeRequestLink("git.cafe", "acme/web", 4),
            parseChangeRequestUrl("https://git.cafe/acme/web/pulls/4"),
        )
    }

    @Test
    fun `claims nothing it cannot be sure of`() {
        for (link in
            listOf(
                "https://github.com/t3tools/t3code/issues/123",
                "https://github.com/t3tools/t3code/commit/0a1b2c3",
                "https://github.com/t3tools/t3code",
                "https://github.com/t3tools/t3code/pull/abc",
                "https://gitlab.com/t3tools/t3code/-/issues/12",
                "https://blog.example.test/2026/updates/pull/3",
                "https://scm.example.test/tfs/Collection/Project/_git/Repo",
                "https://scm.example.test/tfs/Collection/Project/pullrequest/123",
                "https://scm.example.test/tfs/Collection/Project/_git/Repo/pullrequest/abc",
                "https://scm.example.test/tfs/Collection/Project/_git/Repo/pullrequest/0",
                "https://scm.example.test/tfs/Collection/Project/_git/Repo/pullrequest/9007199254740992",
                "ftp://scm.example.test/tfs/Collection/Project/_git/Repo/pullrequest/123",
                "javascript:alert(1)//github.com/t3tools/t3code/pull/1",
                "not a url",
            )) {
            assertNull(link, parseChangeRequestUrl(link))
        }
    }

    @Test
    fun `builds Azure browser URLs from the remote's web origin`() {
        for ((host, repository) in
            listOf(
                "ssh.dev.azure.com" to "v3/org/project/web",
                "vs-ssh.visualstudio.com" to "v3/org/project/web",
                "org.visualstudio.com" to "defaultcollection/project/_git/web",
                "dev.azure.com" to "org/project/_git/web",
            )) {
            assertEquals(
                "https://dev.azure.com/org/project/_git/web/pullrequest/42",
                changeRequestUrlFor("azure-devops", host, repository, 42),
            )
        }
        assertEquals(
            "http://tfs.example.test:8080/tfs/collection/project/_git/repo/pullrequest/7",
            changeRequestUrlFor(
                "azure-devops",
                "tfs.example.test",
                "tfs/collection/project/_git/repo",
                7,
                remoteUrl = "http://tfs.example.test:8080/tfs/Collection/Project/_git/Repo",
            ),
        )
        assertEquals(
            "https://tfs.example.test/tfs/collection/project/_git/repo/pullrequest/7",
            changeRequestUrlFor(
                "azure-devops",
                "tfs.example.test",
                "tfs/collection/project/_git/repo",
                7,
                remoteUrl = "ssh://tfs.example.test:22/tfs/Collection/Project/_git/Repo",
            ),
        )
    }
}
