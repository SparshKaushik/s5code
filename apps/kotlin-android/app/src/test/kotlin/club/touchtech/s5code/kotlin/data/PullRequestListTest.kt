package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.PullRequestActorDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestLabelDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestListEntryDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestListResultDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestProviderSummaryDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test

/**
 * Port of `apps/mobile/src/features/pull-requests/pullRequestList.test.ts`,
 * less the recycled-row equality cases (Compose keys rows instead).
 */
class PullRequestListTest {

    private fun entry(number: Int, transform: PullRequestListEntryDto.() -> PullRequestListEntryDto = { this }) =
        PullRequestListEntryDto(
            number = number,
            provider = "github",
            host = "github.com",
            repository = "org/repo",
            projectId = "project",
            projectTitle = "Project",
            title = "Change $number",
            url = "https://github.com/org/repo/pull/$number",
            author = PullRequestActorDto(login = "octocat"),
            headBranch = "branch-$number",
            baseBranch = "main",
            createdAt = "2026-10-01T00:00:00Z",
            updatedAt = "2026-10-${number.toString().padStart(2, '0')}T00:00:00Z",
            observedAt = 1.0,
        ).transform()

    private fun page(entries: List<PullRequestListEntryDto>, nextCursors: Map<String, String> = emptyMap()) =
        PullRequestListResultDto(
            entries = entries,
            nextCursors = nextCursors,
            truncated = nextCursors.isNotEmpty(),
            viewers = mapOf("github.com" to "octocat"),
        )

    private fun unscoped(vararg entries: PullRequestListEntryDto) = entries.map { ScopedPullRequestEntry(null, it) }

    /* ── Pagination ──────────────────────────────────────────────────── */

    @Test
    fun `continues only the repositories that have more, preserving the query and filters`() {
        val first = pullRequestListHead(null, page(listOf(entry(3)), mapOf("github.com org/repo" to "opaque-boundary")))
        val input = PullRequestListInput(state = "open", query = "fix", filters = PullRequestListFilters(draft = "hide"), limit = 30)
        assertEquals(
            input.copy(cursors = mapOf("github.com org/repo" to "opaque-boundary")),
            nextPullRequestListInput(first, input),
        )
        val next = appendPullRequestListPage(first, page(listOf(entry(2), entry(1))), PullRequestListInput("open", cursors = first.nextCursors))
        assertEquals(listOf(3, 2, 1), next.entries.map { it.number })
        assertNull(nextPullRequestListInput(next, PullRequestListInput("open")))
    }

    @Test
    fun `deduplicates page boundaries and never overwrites a fresh row with an older slice`() {
        val current = entry(2) { copy(title = "Retitled", observedAt = 20.0) }
        assertEquals(
            listOf(current, entry(1)),
            mergePullRequestListEntries(listOf(current), listOf(entry(2) { copy(observedAt = 10.0) }, entry(1))),
        )
        assertEquals(3, mergePullRequestListEntries(listOf(entry(1)), listOf(entry(2) { copy(host = "enterprise.test") }, entry(2))).size)
    }

    @Test
    fun `refreshes the head without dropping older loaded pages or rewinding their cursor`() {
        val first = pullRequestListHead(null, page(listOf(entry(3)), mapOf("repo" to "first")))
        val continued = appendPullRequestListPage(first, page(listOf(entry(2)), mapOf("repo" to "second")), PullRequestListInput("open", cursors = first.nextCursors))
        val updated =
            pullRequestListHead(
                continued,
                page(listOf(entry(4), entry(3) { copy(title = "New title", observedAt = 2.0) }), mapOf("repo" to "first-new")),
            )
        assertEquals(listOf(4, 3, 2), updated.entries.map { it.number })
        assertEquals("New title", updated.entries[1].title)
        assertEquals(mapOf("repo" to "second"), updated.nextCursors)
        assertSame(updated, pullRequestListHead(updated, updated.head))
    }

    @Test
    fun `can grow legacy listings that provide no cursor, without exceeding the wire limit`() {
        val first = pullRequestListHead(null, page(listOf(entry(1))).copy(truncated = true))
        assertEquals(60, nextPullRequestListInput(first, PullRequestListInput("all"))?.limit)
        assertEquals(500, nextPullRequestListInput(first.copy(limit = 490), PullRequestListInput("all"))?.limit)
        assertNull(nextPullRequestListInput(first.copy(limit = 500), PullRequestListInput("all")))
    }

    /* ── All environments ────────────────────────────────────────────── */

    @Test
    fun `combines independent pages, collapses duplicate PRs, and keeps their source for navigation`() {
        val first = pullRequestListHead(null, page(listOf(entry(3), entry(1)), mapOf("repo" to "cups-next")))
        val second = pullRequestListHead(null, page(listOf(entry(2), entry(1)), mapOf("repo" to "mbp-next")))
        val merged = mergeEnvironmentPullRequestPages(listOf("cups" to first, "mbp" to second))
        val rows = filterPullRequestList(merged.entries, DEFAULT_PR_LIST_CONTROLS, merged.viewers, "", merged.localSearchHosts)
        assertEquals(listOf(3 to "cups", 2 to "mbp", 1 to "cups"), rows.map { it.entry.number to it.environmentId })
        assertEquals("mbp:github.com:org/repo:2", scopedPullRequestListKey(rows[1]))
        val continued = appendPullRequestListPage(first, page(listOf(entry(4))), PullRequestListInput("open", cursors = first.nextCursors))
        assertNull(nextPullRequestListInput(continued, PullRequestListInput("open")))
        assertEquals(mapOf("repo" to "mbp-next"), nextPullRequestListInput(second, PullRequestListInput("open"))?.cursors)
    }

    @Test
    fun `uses the viewer account and search support from each environment before deduplicating`() {
        val remote = page(listOf(entry(1) { copy(author = PullRequestActorDto(login = "other")) })).copy(viewers = mapOf("github.com" to "other"))
        val local = page(listOf(entry(1), entry(2)))
        val merged =
            mergeEnvironmentPullRequestPages(
                listOf("cups" to pullRequestListHead(null, remote), "mbp" to pullRequestListHead(null, local))
            )
        val byMe =
            filterPullRequestList(merged.entries, DEFAULT_PR_LIST_CONTROLS.copy(involvement = "authored"), merged.viewers, "", merged.localSearchHosts)
        assertEquals(listOf(2 to "mbp", 1 to "cups"), byMe.map { it.entry.number to it.environmentId })
        val searched = filterPullRequestList(merged.entries, DEFAULT_PR_LIST_CONTROLS, merged.viewers, "body match", setOf("cups github.com"))
        assertEquals(listOf(2 to "mbp", 1 to "mbp"), searched.map { it.entry.number to it.environmentId })
    }

    @Test
    fun `marks hosts that cannot search for local text filtering, per environment`() {
        val head = page(listOf(entry(1))).copy(providers = listOf(PullRequestProviderSummaryDto(host = "codeberg.org", kind = "forgejo", searchesOnHost = false)))
        val merged = mergeEnvironmentPullRequestPages(listOf("cups" to pullRequestListHead(null, head)))
        assertEquals(setOf("cups codeberg.org"), merged.localSearchHosts)
        assertEquals(mapOf("cups github.com" to "octocat"), merged.viewers)
    }

    @Test
    fun `scopes a project to its owning environment even if another has the same project id`() {
        val merged =
            mergeEnvironmentPullRequestPages(
                listOf("cups" to pullRequestListHead(null, page(listOf(entry(1)))), "mbp" to pullRequestListHead(null, page(listOf(entry(2)))))
            )
        assertEquals(
            listOf("mbp"),
            filterPullRequestList(
                merged.entries,
                DEFAULT_PR_LIST_CONTROLS.copy(projectId = "project", projectEnvironmentId = "mbp"),
                merged.viewers,
                "",
                merged.localSearchHosts,
            ).map { it.environmentId },
        )
    }

    /* ── Scan and filters ────────────────────────────────────────────── */

    @Test
    fun `shows review verdicts separately from checks and prioritizes changes requested`() {
        assertEquals("Changes requested", pullRequestListReviewStatus("changes-requested", true)?.accessibleLabel)
        assertEquals("Approved", pullRequestListReviewStatus("approved", false)?.label)
        assertEquals("Your review", pullRequestListReviewStatus(null, true)?.label)
        assertNull(pullRequestListReviewStatus(null, false))
    }

    @Test
    fun `resolves authored and author me per host rather than mixing account identities`() {
        val rows = unscoped(entry(1), entry(2) { copy(host = "enterprise.test") })
        val viewers = mapOf("github.com" to "Octocat", "enterprise.test" to "someone-else")
        assertEquals(listOf(1), filterPullRequestList(rows, DEFAULT_PR_LIST_CONTROLS.copy(involvement = "authored"), viewers, "", emptySet()).map { it.entry.number })
        assertEquals(
            listOf(1),
            filterPullRequestList(rows, DEFAULT_PR_LIST_CONTROLS.copy(filters = PullRequestListFilters(author = "me")), viewers, "", emptySet()).map { it.entry.number },
        )
        assertEquals(listOf(1), filterPullRequestList(rows, DEFAULT_PR_LIST_CONTROLS, viewers, "author:@me", emptySet()).map { it.entry.number })
    }

    @Test
    fun `combines label groups, exclusions, draft and review filters`() {
        val rows =
            unscoped(
                entry(1) { copy(labels = listOf(PullRequestLabelDto("size:XS"), PullRequestLabelDto("bug")), reviewDecision = "approved") },
                entry(2) { copy(isDraft = true, labels = listOf(PullRequestLabelDto("bug"))) },
                entry(3) { copy(labels = listOf(PullRequestLabelDto("feature"))) },
            )
        val filters =
            PullRequestListFilters(
                draft = "hide",
                review = "approved",
                labels = listOf(listOf("SIZE:XS", "size:S"), listOf("bug")),
                excludedLabels = listOf("feature"),
            )
        assertEquals(listOf(1), filterPullRequestList(rows, DEFAULT_PR_LIST_CONTROLS.copy(filters = filters), emptyMap(), "", emptySet()).map { it.entry.number })
    }

    @Test
    fun `reads a typed draft qualifier as a filter and searches only the text left over`() {
        assertEquals(
            PullRequestListInput(
                state = "open",
                involvement = "all",
                filters = PullRequestListFilters(draft = "hide"),
                limit = PR_LIST_PAGE_SIZE,
                query = "fix",
            ),
            pullRequestListInput(DEFAULT_PR_LIST_CONTROLS, "draft:false fix"),
        )
        val rows = unscoped(entry(1) { copy(title = "Fix parser") }, entry(2) { copy(title = "Fix drafts", isDraft = true) }, entry(3))
        assertEquals(listOf(1), filterPullRequestList(rows, DEFAULT_PR_LIST_CONTROLS, emptyMap(), "draft:false fix", setOf("github.com")).map { it.entry.number })
    }

    @Test
    fun `does not treat unsupported review summaries as reviewed or match text locally on host-search results`() {
        val reviewed = DEFAULT_PR_LIST_CONTROLS.copy(filters = PullRequestListFilters(review = "approved"))
        assertEquals(1, filterPullRequestList(unscoped(entry(1)), reviewed, emptyMap(), "body text", emptySet()).size)
        assertEquals(0, filterPullRequestList(unscoped(entry(1)), DEFAULT_PR_LIST_CONTROLS, emptyMap(), "body text", setOf("github.com")).size)
    }

    @Test
    fun `lists a shared repository's row from a connected environment over an offline one`() {
        val rows = listOf(ScopedPullRequestEntry("laptop", entry(1)), ScopedPullRequestEntry("server", entry(1)))
        fun pick(connected: Set<String>) =
            filterPullRequestList(rows, DEFAULT_PR_LIST_CONTROLS, emptyMap(), "", emptySet(), connected).map { it.environmentId }
        assertEquals(listOf("server"), pick(setOf("server")))
        assertEquals(listOf("laptop"), pick(setOf("laptop", "server")))
        assertEquals(listOf("laptop"), pick(emptySet()))
    }

    @Test
    fun `sorts loaded rows and builds unique author and label options`() {
        val rows =
            listOf(
                entry(1) { copy(labels = listOf(PullRequestLabelDto("bug"))) },
                entry(2) { copy(labels = listOf(PullRequestLabelDto("bug"), PullRequestLabelDto("feature"))) },
            )
        assertEquals(
            listOf(1, 2),
            filterPullRequestList(rows.map { ScopedPullRequestEntry(null, it) }, DEFAULT_PR_LIST_CONTROLS.copy(sort = "oldest"), emptyMap(), "", emptySet())
                .map { it.entry.number },
        )
        assertEquals(PullRequestListFacets(authors = listOf("octocat"), labels = listOf("bug", "feature")), pullRequestListFacets(rows))
        assertEquals(0xFFFFAB00, pullRequestLabelColor("ffab00"))
        assertNull(pullRequestLabelColor("not-a-color"))
    }

    @Test
    fun `counts each active filter and every picked label`() {
        assertEquals(0, pullRequestListFilterCount(DEFAULT_PR_LIST_CONTROLS))
        assertEquals(
            4,
            pullRequestListFilterCount(
                DEFAULT_PR_LIST_CONTROLS.copy(state = "all", filters = PullRequestListFilters(draft = "only", labels = listOf(listOf("a"), listOf("b")))),
            ),
        )
    }

    @Test
    fun `serializes list input without empty fields and with sorted cursors`() {
        assertEquals(
            """{"state":"open","filters":{"draft":"hide"},"limit":30,"cursors":{"a":"1","b":"2"},"query":"fix"}""",
            PullRequestListInput(
                state = "open",
                filters = PullRequestListFilters(draft = "hide"),
                limit = 30,
                cursors = mapOf("b" to "2", "a" to "1"),
                query = "fix",
            ).toJson().toString(),
        )
    }

    /* ── Scratch scope ───────────────────────────────────────────────── */

    private val scopes =
        listOf(PullRequestProjectScope("repo", "env-a", isScratch = false), PullRequestProjectScope("scratch", "env-a", isScratch = true))

    @Test
    fun `reads a scope saved on the Scratch folder as all projects`() {
        val controls = withoutScratchProjectScope(DEFAULT_PR_LIST_CONTROLS.copy(projectId = "scratch", projectEnvironmentId = "env-a"), scopes)
        assertNull(controls.projectId)
        assertNull(controls.projectEnvironmentId)
    }

    @Test
    fun `keeps a repository scope, and one whose projects have not arrived yet`() {
        val repository = DEFAULT_PR_LIST_CONTROLS.copy(projectId = "repo", projectEnvironmentId = "env-a")
        assertSame(repository, withoutScratchProjectScope(repository, scopes))
        val pending = repository.copy(projectId = "scratch")
        assertSame(pending, withoutScratchProjectScope(pending, emptyList()))
    }

    @Test
    fun `keeps another server's project that shares the Scratch folder's id`() {
        val elsewhere = DEFAULT_PR_LIST_CONTROLS.copy(projectId = "scratch", projectEnvironmentId = "env-b")
        assertSame(elsewhere, withoutScratchProjectScope(elsewhere, scopes))
    }

    @Test
    fun `filter picks stay first, then loaded matches, then the host's answers, each once`() {
        assertEquals(
            listOf("needs-review", "bug", "Bugfix", "bug-report"),
            pullRequestFilterChoices(
                selected = listOf("needs-review"),
                suggestions = listOf("bug", "feature", "Bugfix"),
                hosted = listOf("BUG", "bug-report"),
                query = " bug",
            ),
        )
        val many = (1..120).map { "label-$it" }
        val rows = pullRequestFilterChoices(emptyList(), many, listOf("hosted"), "")
        assertEquals(100, rows.size)
        // The host's answers get room after the first half of the loaded matches.
        assertEquals("hosted", rows[50])
    }

    @Test
    fun `label colors key by lowercased name and keep the first usable one`() {
        assertEquals(
            mapOf("bug" to 0xFFD73A4A),
            pullRequestLabelColors(
                listOf(PullRequestLabelDto("Bug", "nope"), PullRequestLabelDto("bug", "d73a4a"), PullRequestLabelDto("BUG", "000000"))
            ),
        )
    }
}
