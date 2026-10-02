package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ComposerPullRequestCandidate
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
}
