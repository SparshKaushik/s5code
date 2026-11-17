package club.touchtech.s5code.kotlin.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Port of `packages/client-runtime/src/pullRequestQuery.test.ts`. */
class PullRequestQueryTest {

    @Test
    fun `keeps quoted values whole and separates negation`() {
        assertEquals(
            ParsedPullRequestQuery(
                text = "Fix the parser",
                filters = PullRequestListFilters(labels = listOf(listOf("needs design")), excludedLabels = listOf("wip")),
            ),
            parsePullRequestQuery("label:\"needs design\" -label:wip Fix the parser"),
        )
    }

    @Test
    fun `reads a comma-separated list as one group either name satisfies`() {
        assertEquals(listOf(listOf("size:S", "size:XS")), parsePullRequestQuery("label:size:S,size:XS").filters.labels)
    }

    @Test
    fun `keeps the spaces in a quoted name of a list, and drops an empty part`() {
        assertEquals(listOf(listOf("needs design", "wip")), parsePullRequestQuery("label:\"needs design\",\"wip\"").filters.labels)
        assertEquals(listOf(listOf("a", "b")), parsePullRequestQuery("label:a,,b").filters.labels)
    }

    @Test
    fun `excludes every name of a negated list`() {
        assertEquals(PullRequestListFilters(excludedLabels = listOf("a", "b")), parsePullRequestQuery("-label:a,b").filters)
    }

    @Test
    fun `makes each label qualifier its own group, so the groups are an AND`() {
        assertEquals(listOf(listOf("bug"), listOf("a", "b")), parsePullRequestQuery("label:bug label:a,b").filters.labels)
    }

    @Test
    fun `reads the scalar qualifiers in GitHub's own spelling`() {
        assertEquals(
            PullRequestListFilters(author = "octocat", draft = "hide", review = "changes-requested", checks = "failing"),
            parsePullRequestQuery("author:octocat DRAFT:false review:changes_requested status:failure").filters,
        )
    }

    @Test
    fun `leaves an unknown value and a stray colon as text, and reads an unknown key as a label`() {
        val parsed = parsePullRequestQuery("milestone:v2 draft:maybe status: parser")
        assertEquals(PullRequestListFilters(labels = listOf(listOf("milestone:v2"))), parsed.filters)
        assertEquals("draft:maybe status: parser", parsed.text)
    }

    @Test
    fun `reads a quoted name holding a comma as the one label it is`() {
        assertEquals(listOf(listOf("needs,triage")), parsePullRequestQuery("label:\"needs,triage\"").filters.labels)
        assertEquals(listOf(listOf("needs,triage", "wip")), parsePullRequestQuery("label:\"needs,triage\",wip").filters.labels)
        assertEquals(listOf("needs,triage"), parsePullRequestQuery("-label:\"needs,triage\"").filters.excludedLabels)
    }

    @Test
    fun `cuts a typed query back to what the contract carries`() {
        val many = (0 until 14).joinToString(",") { "l$it" }
        val long = "x".repeat(240)
        val parsed = parsePullRequestQuery("label:$many label:$long author:$long")
        assertEquals(10, parsed.filters.labels?.get(0)?.size)
        assertEquals(listOf("x".repeat(200)), parsed.filters.labels?.get(1))
        assertEquals("x".repeat(200), parsed.filters.author)
    }

    @Test
    fun `cuts a namespaced label after its key is prefixed on`() {
        val parsed = parsePullRequestQuery("size:${"x".repeat(240)}")
        assertEquals(listOf(listOf("size:${"x".repeat(240)}".take(200))), parsed.filters.labels)
    }

    @Test
    fun `answers an empty query with an empty everything`() {
        assertEquals(ParsedPullRequestQuery("", PullRequestListFilters()), parsePullRequestQuery("   "))
    }

    @Test
    fun `reads an unknown key as the label it almost always is`() {
        assertEquals(listOf(listOf("size:XXL")), parsePullRequestQuery("size:XXL").filters.labels)
        assertEquals(listOf(listOf("vouch:trusted")), parsePullRequestQuery("vouch:trusted").filters.labels)
        assertEquals("", parsePullRequestQuery("size:XXL").text)
    }

    @Test
    fun `reads the key as the namespace of every bare name in its list`() {
        assertEquals(listOf(listOf("size:S", "size:XS")), parsePullRequestQuery("size:S,XS").filters.labels)
        assertEquals(listOf(listOf("size:S", "size:XS")), parsePullRequestQuery("size:S,size:XS").filters.labels)
    }

    @Test
    fun `excludes a namespaced label the same way`() {
        assertEquals(listOf("size:XXL"), parsePullRequestQuery("-size:XXL").filters.excludedLabels)
        assertEquals(listOf("size:S", "size:XS"), parsePullRequestQuery("-size:S,XS").filters.excludedLabels)
    }

    @Test
    fun `keeps a quoted token as text, which is the way back to a literal search`() {
        val parsed = parsePullRequestQuery("\"size:XXL\"")
        assertNull(parsed.filters.labels)
        assertEquals("\"size:XXL\"", parsed.text)
    }

    @Test
    fun `reads a hyphenated namespace as a label too`() {
        assertEquals(listOf(listOf("priority-1:high")), parsePullRequestQuery("priority-1:high").filters.labels)
        assertEquals("", parsePullRequestQuery("priority-1:high").text)
    }

    @Test
    fun `leaves a pasted link alone rather than naming a label after its scheme`() {
        val parsed = parsePullRequestQuery("https://github.com/pingdotgg/t3code/pull/1")
        assertNull(parsed.filters.labels)
        assertEquals("https://github.com/pingdotgg/t3code/pull/1", parsed.text)
    }

    @Test
    fun `mixes with the keys it does know, and with plain words`() {
        val parsed = parsePullRequestQuery("area:web draft:true wizard label:bug")
        assertEquals(PullRequestListFilters(labels = listOf(listOf("area:web"), listOf("bug")), draft = "only"), parsed.filters)
        assertEquals("wizard", parsed.text)
    }

    @Test
    fun `shares the ceiling with the labels typed as labels`() {
        val many = (0 until 12).joinToString(" ") { "area:$it" }
        assertEquals(10, parsePullRequestQuery(many).filters.labels?.size)
    }

    @Test
    fun `replaces or clears a typed draft qualifier and keeps the rest of the query`() {
        assertEquals(
            PullRequestDraftFilterChange("fix label:\"needs design\"", PullRequestListFilters(draft = "only")),
            applyPullRequestDraftFilterChange(
                "draft:false fix label:\"needs design\"",
                PullRequestListFilters(),
                PullRequestListFilters(draft = "only"),
            ),
        )
        assertEquals(
            PullRequestDraftFilterChange("\"draft:false\" fix", PullRequestListFilters()),
            applyPullRequestDraftFilterChange(
                "draft:true \"draft:false\" fix",
                PullRequestListFilters(draft = "only"),
                PullRequestListFilters(),
            ),
        )
    }

    @Test
    fun `keeps the typed query and the stored menu draft when another filter changes`() {
        assertEquals(
            PullRequestDraftFilterChange("draft:false fix", PullRequestListFilters(draft = "only", checks = "passing")),
            applyPullRequestDraftFilterChange(
                "draft:false fix",
                PullRequestListFilters(draft = "only"),
                PullRequestListFilters(draft = "hide", checks = "passing"),
            ),
        )
    }

    @Test
    fun `resolves me to the signed-in login and leaves other authors alone`() {
        assertEquals("octocat", resolvePullRequestAuthorFilter("@me", "octocat"))
        assertEquals("octocat", resolvePullRequestAuthorFilter("ME", "octocat"))
        assertEquals("@me", resolvePullRequestAuthorFilter("@me", null))
        assertEquals("hubot", resolvePullRequestAuthorFilter(" hubot ", "octocat"))
    }
}
