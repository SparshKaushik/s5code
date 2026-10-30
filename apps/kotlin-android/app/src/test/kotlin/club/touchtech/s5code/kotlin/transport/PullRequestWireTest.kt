package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.transport.wire.PullRequestActivityDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDetailDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDiffResultDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestListResultDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestStackDto
import kotlinx.serialization.builtins.nullable
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** PR wire decoding and the client-side error text rebuilt from tagged failures. */
class PullRequestWireTest {

    private fun failure(tag: String?, message: String = "", vararg fields: Pair<String, String>) =
        RpcFailure(
            RpcFailureKind.Fail,
            tag,
            message,
            if (fields.isEmpty()) null else JsonObject(fields.associate { (key, value) -> key to JsonPrimitive(value) }),
        )

    /* ── Errors ──────────────────────────────────────────────────────── */

    @Test
    fun `rebuilds the provider requirement for an unavailable host`() {
        assertEquals(
            "GitLab CLI is not authenticated. Run `glab auth login` and retry.",
            pullRequestErrorMessage(
                failure("PullRequestUnavailableError", "", "reason" to "cli-unauthenticated", "provider" to "gitlab")
            ),
        )
        assertTrue(
            pullRequestErrorMessage(failure("PullRequestUnavailableError", "", "reason" to "cli-missing", "provider" to "github"))
                .startsWith("No GitHub credential on the server.")
        )
        assertEquals(
            "The tool this host is read through is not installed or set up.",
            pullRequestUnavailableMessage("cli-missing", "unknown-host"),
        )
        assertEquals(
            "Change requests cannot be browsed for this project's host yet.",
            pullRequestUnavailableMessage("provider-unsupported", null),
        )
        assertNull(pullRequestUnavailableMessage("something-new", "github"))
    }

    @Test
    fun `prefers the setup step the server names for the host`() {
        assertEquals(
            "Sign in with `glab auth login --hostname git.example.com`.",
            pullRequestErrorMessage(
                failure(
                    "PullRequestUnavailableError",
                    "",
                    "reason" to "cli-unauthenticated",
                    "provider" to "gitlab",
                    "requirement" to "Sign in with `glab auth login --hostname git.example.com`.",
                )
            ),
        )
        assertEquals(
            "Install the tool.",
            pullRequestUnavailableMessage("cli-missing", null, requirement = "Install the tool."),
        )
        assertEquals(
            "Change requests cannot be browsed for this project's host yet.",
            pullRequestUnavailableMessage("provider-unsupported", "github", requirement = "Ignored."),
        )
    }

    @Test
    fun `names the failed operation and its detail`() {
        assertEquals(
            "Pull request operation detail failed: rate limited",
            pullRequestErrorMessage(failure("PullRequestOperationError", "", "operation" to "detail", "detail" to "rate limited")),
        )
    }

    @Test
    fun `falls back to the failure's own message, then to a generic one`() {
        assertEquals("socket closed", pullRequestErrorMessage(RpcTransportClosed("socket closed")))
        assertEquals(
            "Server said no",
            pullRequestErrorMessage(failure("PullRequestUnavailableError", "Server said no", "reason" to "something-new")),
        )
        assertEquals("Couldn't load this pull request.", pullRequestErrorMessage(failure(null, " ")))
    }

    @Test
    fun `recognizes a pull request that is gone`() {
        assertTrue(isPullRequestNotFound(failure("PullRequestOperationError", "", "reason" to "not-found")))
        assertFalse(isPullRequestNotFound(failure("PullRequestOperationError", "", "reason" to "rate-limited")))
        assertFalse(isPullRequestNotFound(failure("PullRequestUnavailableError", "", "reason" to "not-found")))
    }

    /* ── Decoding ────────────────────────────────────────────────────── */

    @Test
    fun `decodes a list page leniently, tolerating unknown fields and nulls`() {
        val result =
            TransportJson.decodeFromString(
                PullRequestListResultDto.serializer(),
                """
                {
                  "viewers": {"github.com": "octocat"},
                  "providers": [{"host": "codeberg.org", "kind": "forgejo", "searchesOnHost": false, "future": 1}],
                  "entries": [{
                    "provider": "github", "host": "github.com", "projectId": "p", "projectTitle": "P",
                    "repository": "org/repo", "number": 7, "title": "Fix", "url": "https://github.com/org/repo/pull/7",
                    "author": {"login": "octocat", "name": null, "avatarUrl": null},
                    "headBranch": "fix", "baseBranch": "main", "state": "open", "isDraft": false,
                    "mergeability": "conflicting", "createdAt": "2026-10-01T00:00:00Z", "updatedAt": "2026-10-02T00:00:00Z",
                    "labels": [{"name": "bug", "color": "d73a4a"}], "reviewDecision": null, "checksState": "failing",
                    "stack": {"id": "s", "position": 2, "size": 3}, "somethingNew": true
                  }],
                  "errors": [{"projectId": "q", "projectTitle": "Q", "message": "No credential"}],
                  "truncated": true,
                  "nextCursors": {"github.com org/repo": "abc"}
                }
                """.trimIndent(),
            )
        val entry = result.entries.single()
        assertEquals(7, entry.number)
        assertEquals("octocat", entry.author?.login)
        assertEquals("bug", entry.labels.single().name)
        assertNull(entry.reviewDecision)
        assertEquals("failing", entry.checksState)
        assertFalse(result.providers.single().searchesOnHost)
        assertEquals("No credential", result.errors.single().message)
        assertEquals(mapOf("github.com org/repo" to "abc"), result.nextCursors)
    }

    @Test
    fun `decodes a sparse detail and activity with defaults`() {
        val detail =
            TransportJson.decodeFromString(
                PullRequestDetailDto.serializer(),
                """{"number": 3, "title": "T", "state": "merged", "checks": [{"name": "ci", "status": "success"}], "reviewers": null}""",
            )
        assertEquals("merged", detail.state)
        assertEquals("success", detail.checks.single().status)
        assertTrue(detail.reviewers.isEmpty())
        val activity =
            TransportJson.decodeFromString(
                PullRequestActivityDto.serializer(),
                """
                {"comments": [{"id": "c", "kind": "review", "body": "LGTM", "createdAt": "2026-10-01T00:00:00Z", "reviewState": "APPROVED"}],
                 "commentCount": 1, "commentsTruncated": false,
                 "reviewThreads": [{"id": "t", "path": "a.kt", "line": null, "side": "left", "isResolved": false, "isOutdated": true,
                                    "comments": [{"id": "r", "body": "nit", "createdAt": "2026-10-01T00:00:00Z"}]}],
                 "commits": [{"oid": "abc", "messageHeadline": "Init", "committedDate": "2026-09-30T00:00:00Z"}]}
                """.trimIndent(),
            )
        assertEquals("APPROVED", activity.comments.single().reviewState)
        assertNull(activity.reviewThreads.single().line)
        assertEquals("r", activity.reviewThreads.single().comments.single().id)
        assertEquals("abc", activity.commits.single().oid)
    }

    @Test
    fun `decodes a null stack as no stack and a diff slice with its cursor`() {
        assertNull(TransportJson.decodeFromString(PullRequestStackDto.serializer().nullable, "null"))
        val stack =
            TransportJson.decodeFromString(
                PullRequestStackDto.serializer().nullable,
                """{"id": "s", "number": 2, "url": "u", "base": "main", "layers": [{"number": 1, "headBranch": "a", "state": "merged"}]}""",
            )
        assertEquals("merged", stack?.layers?.single()?.state)
        val diff =
            TransportJson.decodeFromString(
                PullRequestDiffResultDto.serializer(),
                """{"patch": "diff --git a/x b/x", "truncated": true, "nextCursor": "page-2", "omittedFileStats": [{"path": "big.lock", "additions": 900}]}""",
            )
        assertEquals("page-2", diff.nextCursor)
        assertEquals(900, diff.omittedFileStats?.single()?.additions)
    }
}
