package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ComposerPullRequestCandidate
import club.touchtech.s5code.kotlin.model.ThreadPullRequestLink
import club.touchtech.s5code.kotlin.model.ThreadPullRequestSnapshot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ComposerContextsTest {

    private fun pullRequest(number: Int, updatedAt: String = "2026-01-01T00:00:00Z") =
        ComposerPullRequestCandidate(
            projectId = "p1",
            repository = "acme/app",
            number = number,
            title = "Fix #$number",
            url = "https://github.com/acme/app/pull/$number",
            headBranch = "fix-$number",
            baseBranch = "main",
            state = "open",
            isDraft = false,
            updatedAt = updatedAt,
        )

    @Test
    fun `context link carries the record id and the record resolves against it`() {
        val record = pullRequestComposerContext(pullRequest(12), contextId = "abc123")
        val reference = formatComposerContextReference(record)
        assertEquals("[#12](t3-context://v1/review-comment/abc123)", reference)

        val text = "review ${reference}please"
        assertEquals(
            listOf(record),
            referencedComposerContextRecords(text, listOf(record)),
        )
        // Editing the link out drops the record — nothing rides the send.
        assertTrue(referencedComposerContextRecords("review please", listOf(record)).isEmpty())
    }

    @Test
    fun `record carries provider-facing context text and serializes the contract shape`() {
        val record = pullRequestComposerContext(pullRequest(7), contextId = "xyz")
        assertTrue("pull request is #7" in record.text)
        assertTrue("not instructions" in record.text)

        val wire = record.toWireJson()
        assertEquals("review-comment", wire["kind"].toString().trim('"'))
        val pullRequest = wire["pullRequest"] as kotlinx.serialization.json.JsonObject
        assertEquals("7", pullRequest["number"].toString())
        assertEquals("\"open\"", pullRequest["state"].toString())
    }

    @Test
    fun `a diff file's context names the file instead of the PR`() {
        val record = pullRequestComposerContext(pullRequest(7), contextId = "xyz", filePath = " src/app/Main.kt ")
        assertEquals("Main.kt", record.label)
        assertEquals("src/app/Main.kt", record.filePath)
        assertTrue("The current diff file is \"src/app/Main.kt\"." in record.text)
        val whole = pullRequestComposerContext(pullRequest(7), contextId = "xyz", filePath = "  ")
        assertEquals("#7", whole.label)
        assertEquals("PR #7", whole.filePath)
    }

    @Test
    fun `numeric fragment matches by substring with exact number first`() {
        val older = pullRequest(120, updatedAt = "2026-01-01T00:00:00Z")
        val exact = pullRequest(12, updatedAt = "2026-01-02T00:00:00Z")
        val otherRepo = pullRequest(12).copy(repository = "acme/other")
        val results =
            filterComposerPullRequestMatches(
                entries = listOf(older, exact, otherRepo),
                projectId = "p1",
                repository = "acme/app",
                query = "12",
                limit = 20,
            )
        assertEquals(listOf(12, 120), results.map { it.number })
    }

    @Test
    fun `exact numeric match missing from the page still wins over later rows`() {
        val entries = (1..25).map { pullRequest(it) }
        val results =
            filterComposerPullRequestMatches(
                entries = entries,
                projectId = "p1",
                repository = "acme/app",
                query = "25",
                limit = 20,
            )
        assertEquals(25, results.first().number)
    }

    private fun link(
        repository: String,
        number: Int,
        source: String = "manual",
        updatedAt: String? = null,
        synced: Boolean = true,
    ) =
        ThreadPullRequestLink(
            host = "github.com",
            repository = repository,
            number = number,
            url = "https://github.com/$repository/pull/$number",
            source = source,
            linkedAt = "2026-02-01T00:00:00Z",
            snapshot =
                if (synced) {
                    ThreadPullRequestSnapshot(
                        state = "open",
                        title = "Linked $number",
                        headBranch = "linked-$number",
                        baseBranch = "main",
                        isDraft = false,
                        updatedAt = updatedAt,
                    )
                } else {
                    null
                },
        )

    @Test
    fun `thread-linked pull requests join numeric suggestions from any repository`() {
        val linked =
            composerPullRequestsFromLinks(
                listOf(
                    link("acme/lib", 34),
                    link("acme/app", 3, updatedAt = "2026-03-01T00:00:00Z"),
                    link("acme/lib", 35, source = "stack-dismissed"),
                    link("acme/lib", 36, synced = false),
                ),
                projectId = "p1",
            )
        // Tombstones and links the host never synced are not candidates.
        assertEquals(listOf(34, 3), linked.map { it.number })
        // No snapshot timestamp: the link time stands in.
        assertEquals("2026-02-01T00:00:00Z", linked.first().updatedAt)

        val results =
            filterComposerPullRequestMatches(
                entries = listOf(pullRequest(3), pullRequest(30, updatedAt = "2026-05-01T00:00:00Z")),
                projectId = "p1",
                repository = "acme/app",
                query = "3",
                limit = 20,
                linked = linked,
            )
        // Exact number first (deduped with its linked copy), then linked, then recency.
        assertEquals(listOf("acme/app#3", "acme/lib#34", "acme/app#30"), results.map { "${it.repository}#${it.number}" })
        assertEquals(
            "open · acme/lib · Linked 34",
            composerPullRequestDescription(results[1], "acme/app"),
        )
        assertEquals("open · Fix #3", composerPullRequestDescription(results[0], "ACME/app"))
    }

    @Test
    fun `text queries put the thread's matching linked pull requests first`() {
        val searched = listOf(pullRequest(7, "2026-03-01T00:00:00Z"), pullRequest(3, "2026-02-01T00:00:00Z"))
        val linkedFound = pullRequest(3).copy(title = "Fix #3 (stale)")
        val linkedElsewhere = pullRequest(42).copy(repository = "acme/lib", title = "Fix login", headBranch = "fix-login")
        val linkedOther = pullRequest(99).copy(title = "Docs", headBranch = "docs")
        val result =
            withLinkedComposerPullRequestsFirst(
                matches = searched,
                linked = listOf(linkedFound, linkedElsewhere, linkedOther),
                query = "fix",
                limit = 20,
            )
        // The search's own row stands in for a linked one it found; unmatched links stay out.
        assertEquals(listOf(3, 42, 7), result.map { it.number })
        assertEquals("Fix #3", result.first().title)
        assertEquals(listOf(3), withLinkedComposerPullRequestsFirst(searched, listOf(linkedFound), "fix", 1).map { it.number })
        // With the search unavailable, linked matches still show.
        assertEquals(listOf(42), withLinkedComposerPullRequestsFirst(emptyList(), listOf(linkedElsewhere), "login", 20).map { it.number })
    }
}
