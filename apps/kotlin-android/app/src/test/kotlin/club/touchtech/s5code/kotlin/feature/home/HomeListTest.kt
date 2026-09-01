package club.touchtech.s5code.kotlin.feature.home

import club.touchtech.s5code.kotlin.app.HomeUiState
import club.touchtech.s5code.kotlin.app.NewTaskDraft
import club.touchtech.s5code.kotlin.app.ThreadDraft
import club.touchtech.s5code.kotlin.data.HomePendingRequest
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.HomeListItem
import club.touchtech.s5code.kotlin.model.PendingApproval
import club.touchtech.s5code.kotlin.model.PendingUserInput
import club.touchtech.s5code.kotlin.model.ProjectGrouping
import club.touchtech.s5code.kotlin.model.SearchMatchSource
import club.touchtech.s5code.kotlin.model.ShelfKind
import club.touchtech.s5code.kotlin.model.ThreadFilter
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadSort
import club.touchtech.s5code.kotlin.model.ThreadSearchMatch
import club.touchtech.s5code.kotlin.model.ThreadStatus
import club.touchtech.s5code.kotlin.model.ApprovalKind
import club.touchtech.s5code.kotlin.model.UserInputKind
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class HomeListTest {

    private val threads = HomeFixture.threads
    private val projects = HomeFixture.projects
    private val environments = HomeFixture.environments

    private fun items(
        state: HomeUiState = HomeUiState(),
        grouping: ProjectGrouping = ProjectGrouping.ByProject,
        sort: ThreadSort = ThreadSort.Recent,
        drafts: Map<String, ThreadDraft> = emptyMap(),
        newTask: NewTaskDraft? = null,
        pendingRequests: List<HomePendingRequest> = emptyList(),
    ) =
        homeListItems(
            threads,
            projects,
            environments,
            state,
            grouping,
            sort,
            drafts,
            newTask,
            pendingRequests,
        )

    @Test
    fun `keys are unique so lazy list identity is stable`() {
        val keys = items().map { it.key }
        assertEquals(keys.size, keys.toSet().size)
    }

    @Test
    fun `snoozed and settled threads live behind their shelves`() {
        val result = items(HomeUiState(snoozedExpanded = false, settledExpanded = false))
        val shelves = result.filterIsInstance<HomeListItem.ShelfHeader>()
        assertEquals(setOf(ShelfKind.Snoozed, ShelfKind.Settled), shelves.map { it.kind }.toSet())
        val shown = result.filterIsInstance<HomeListItem.Thread>().map { it.thread.status }
        assertFalse(shown.contains(ThreadStatus.Snoozed))
        assertFalse(shown.contains(ThreadStatus.Settled))
    }

    @Test
    fun `expanding the settled shelf reveals its threads`() {
        val collapsed =
            items(HomeUiState(settledExpanded = false)).filterIsInstance<HomeListItem.Thread>().size
        val expanded =
            items(HomeUiState(settledExpanded = true)).filterIsInstance<HomeListItem.Thread>().size
        assertTrue(expanded > collapsed)
    }

    @Test
    fun `both shelves start collapsed`() {
        // Home opens on work that is still moving. Snoozed and settled are the two
        // piles the user put away, so the default state of a fresh install has to
        // be collapsed for both.
        val fresh = items()
        assertTrue(fresh.filterIsInstance<HomeListItem.ShelfHeader>().none { it.expanded })
        val shown = fresh.filterIsInstance<HomeListItem.Thread>().map { it.thread.status }
        assertFalse(shown.contains(ThreadStatus.Snoozed))
        assertFalse(shown.contains(ThreadStatus.Settled))
    }

    @Test
    fun `searching reveals threads behind a collapsed shelf`() {
        val settledTitle =
            threads.first { it.status == ThreadStatus.Settled }.title.substringBefore(' ')
        val result =
            items(HomeUiState(query = settledTitle, settledExpanded = false))
                .filterIsInstance<HomeListItem.Thread>()

        // Answering "no matches" while the match sits behind a collapsed shelf is
        // the failure this guards.
        assertTrue(result.any { it.thread.status == ThreadStatus.Settled })
    }

    @Test
    fun `a shelf filter opens its own shelf`() {
        val settled =
            items(HomeUiState(filter = ThreadFilter.Settled, settledExpanded = false))
        assertTrue(
            settled.filterIsInstance<HomeListItem.Thread>().isNotEmpty()
        )
        assertTrue(settled.filterIsInstance<HomeListItem.ShelfHeader>().single().expanded)

        // The other shelf is unaffected: filtering to Settled says nothing about
        // what the user wants Snoozed to do.
        val snoozed = items(HomeUiState(filter = ThreadFilter.Snoozed, snoozedExpanded = false))
        assertTrue(snoozed.filterIsInstance<HomeListItem.Thread>().isNotEmpty())
        assertEquals(
            setOf(ShelfKind.Snoozed),
            snoozed.filterIsInstance<HomeListItem.ShelfHeader>().map { it.kind }.toSet(),
        )
    }

    @Test
    fun `pending filter shows only approval and input threads`() {
        val result = items(HomeUiState(filter = ThreadFilter.Pending))
        val statuses = result.filterIsInstance<HomeListItem.Thread>().map { it.thread.status }
        assertTrue(statuses.isNotEmpty())
        assertTrue(
            statuses.all {
                it == ThreadStatus.AwaitingApproval || it == ThreadStatus.AwaitingInput
            }
        )
    }

    @Test
    fun `filter counts cover every filter`() {
        val counts = homeFilterCounts(threads)
        assertEquals(ThreadFilter.entries.size, counts.size)
        assertEquals(threads.size, counts[ThreadFilter.All])
        assertEquals(
            threads.count { it.status == ThreadStatus.Snoozed },
            counts[ThreadFilter.Snoozed],
        )
    }

    @Test
    fun `search matches title branch project and provider`() {
        assertTrue(
            items(HomeUiState(query = "kotlin")).filterIsInstance<HomeListItem.Thread>().isNotEmpty()
        )
        assertTrue(
            items(HomeUiState(query = "perf/home-stream"))
                .filterIsInstance<HomeListItem.Thread>()
                .isNotEmpty()
        )
        assertTrue(
            items(HomeUiState(query = "marketing"))
                .filterIsInstance<HomeListItem.Thread>()
                .isNotEmpty()
        )
        assertTrue(items(HomeUiState(query = "zzzz")).isEmpty())
    }

    @Test
    fun `search terms are combined with and`() {
        val single = items(HomeUiState(query = "relay")).filterIsInstance<HomeListItem.Thread>()
        val both = items(HomeUiState(query = "relay token")).filterIsInstance<HomeListItem.Thread>()
        assertTrue(both.size <= single.size)
    }

    @Test
    fun `queued row keys include the environment for cross-environment identity`() {
        val first =
            threads.first { it.status == ThreadStatus.Queued }.copy(
                id = club.touchtech.s5code.kotlin.model.ThreadId("shared-id"),
                environmentId = HomeFixture.laptop,
            )
        val second = first.copy(environmentId = HomeFixture.devbox)
        val result =
            homeListItems(
                    threads = listOf(first, second),
                    projects = projects,
                    environments = environments,
                    state = HomeUiState(),
                    grouping = ProjectGrouping.Flat,
                    sort = ThreadSort.Recent,
                )
                .filterIsInstance<HomeListItem.Queued>()

        assertEquals(2, result.size)
        assertEquals(2, result.map { it.key }.toSet().size)
    }

    @Test
    fun `pending request payloads replace their passive thread rows with actionable cards`() {
        val approvalThread = threads.first { it.id.value == "thr-approval" }
        val inputThread = threads.first { it.id.value == "thr-input" }
        val result =
            items(
                grouping = ProjectGrouping.Flat,
                pendingRequests =
                    listOf(
                        HomePendingRequest(
                            environmentId = approvalThread.environmentId,
                            threadId = approvalThread.id,
                            approval =
                                PendingApproval(
                                    id = "approval-1",
                                    title = "Run tests",
                                    detail = "The agent wants to run Gradle.",
                                    command = "./gradlew test",
                                    kind = ApprovalKind.Command,
                                ),
                        ),
                        HomePendingRequest(
                            environmentId = inputThread.environmentId,
                            threadId = inputThread.id,
                            userInput =
                                PendingUserInput(
                                    id = "input-1",
                                    questions =
                                        listOf(
                                            club.touchtech.s5code.kotlin.model.UserInputQuestion(
                                                id = "headline",
                                                header = "Headline",
                                                prompt = "Which headline?",
                                                kind = UserInputKind.SingleSelect,
                                                options = listOf("A", "B"),
                                            )
                                        ),
                                ),
                        ),
                    ),
            )

        val approval = result.single { it.key.contains("approval-1") }
        val input = result.single { it.key.contains("input-1") }
        assertTrue(approval is HomeListItem.PendingApprovalCard)
        assertTrue(input is HomeListItem.PendingInputCard)
        assertTrue(
            result.filterIsInstance<HomeListItem.Thread>().none {
                it.thread.id == approvalThread.id || it.thread.id == inputThread.id
            }
        )
    }

    @Test
    fun `a pending shell stays a normal row until its actionable payload arrives`() {
        val result = items(grouping = ProjectGrouping.Flat)
        assertTrue(
            result.filterIsInstance<HomeListItem.Thread>().any {
                it.thread.id.value == "thr-approval"
            }
        )
    }

    @Test
    fun `search result carries the matching field used for highlighting`() {
        val branchResult =
            items(HomeUiState(query = "HOME-STREAM"), ProjectGrouping.Flat)
                .filterIsInstance<HomeListItem.Thread>()
                .single()
        assertEquals(SearchMatchSource.Branch, branchResult.searchMatch?.source)
        assertEquals("perf/home-stream", branchResult.searchMatch?.text)

        val projectResult =
            items(HomeUiState(query = "marketing-site"), ProjectGrouping.Flat)
                .filterIsInstance<HomeListItem.Thread>()
                .first()
        assertEquals(SearchMatchSource.Project, projectResult.searchMatch?.source)
    }

    @Test
    fun `server message match includes a thread whose shell fields do not match`() {
        val target = threads.first { it.id.value == "thr-working" }
        val result =
            homeListItems(
                    threads = threads,
                    projects = projects,
                    environments = environments,
                    state = HomeUiState(query = "needle only in message"),
                    grouping = ProjectGrouping.Flat,
                    sort = ThreadSort.Recent,
                    serverSearchMatches =
                        listOf(
                            ThreadSearchMatch(
                                environmentId = target.environmentId,
                                threadId = target.id,
                                projectId = target.projectId,
                                source = SearchMatchSource.AssistantMessage,
                                snippet = "Found the needle only in message body",
                                messageCreatedAt = null,
                            )
                        ),
                )
                .filterIsInstance<HomeListItem.Thread>()

        assertEquals(listOf(target.id), result.map { it.thread.id })
        assertEquals(SearchMatchSource.AssistantMessage, result.single().searchMatch?.source)
        assertEquals("Found the needle only in message body", result.single().searchMatch?.text)
    }

    @Test
    fun `a local title match wins presentation over a server message match`() {
        val target = threads.first { it.id.value == "thr-working" }
        val query = target.title.substringBefore(' ')
        val result =
            homeListItems(
                    threads = threads,
                    projects = projects,
                    environments = environments,
                    state = HomeUiState(query = query),
                    grouping = ProjectGrouping.Flat,
                    sort = ThreadSort.Recent,
                    serverSearchMatches =
                        listOf(
                            ThreadSearchMatch(
                                environmentId = target.environmentId,
                                threadId = target.id,
                                projectId = target.projectId,
                                source = SearchMatchSource.UserMessage,
                                snippet = query,
                                messageCreatedAt = null,
                            )
                        ),
                )
                .filterIsInstance<HomeListItem.Thread>()
                .first { it.thread.id == target.id }

        assertEquals(SearchMatchSource.Title, result.searchMatch?.source)
    }

    @Test
    fun `an empty query does not add search presentation to rows`() {
        assertTrue(
            items(grouping = ProjectGrouping.Flat)
                .filterIsInstance<HomeListItem.Thread>()
                .all { it.searchMatch == null }
        )
    }

    @Test
    fun `environment filter narrows to one environment`() {
        val result = items(HomeUiState(environmentId = HomeFixture.devbox))
        val ids = result.filterIsInstance<HomeListItem.Thread>().map { it.thread.environmentId }
        assertTrue(ids.isNotEmpty())
        assertTrue(ids.all { it == HomeFixture.devbox })
    }

    @Test
    fun `flat grouping emits no sections`() {
        val result = items(grouping = ProjectGrouping.Flat)
        assertTrue(result.none { it is HomeListItem.Section })
    }

    @Test
    fun `project grouping emits one section per project`() {
        val sections = items().filterIsInstance<HomeListItem.Section>().map { it.label }
        assertEquals(sections.size, sections.toSet().size)
        assertTrue(sections.isNotEmpty())
    }

    @Test
    fun `pinned and attention-needing threads sort first in a group`() {
        val flat = items(grouping = ProjectGrouping.Flat).filterIsInstance<HomeListItem.Thread>()
        assertTrue(flat.first().thread.pinned)
        val firstUnpinned = flat.first { !it.thread.pinned }.thread.status
        assertTrue(
            firstUnpinned == ThreadStatus.AwaitingApproval ||
                firstUnpinned == ThreadStatus.AwaitingInput ||
                firstUnpinned == ThreadStatus.Failed ||
                firstUnpinned == ThreadStatus.Working
        )
    }

    @Test
    fun `recent sort orders newest first within a status rank`() {
        val settled =
            items(HomeUiState(settledExpanded = true), ProjectGrouping.Flat)
                .filterIsInstance<HomeListItem.Thread>()
                .map { it.thread }
                .filter { it.status == ThreadStatus.Settled }
                .map { it.updatedAtMillis }
        assertEquals(settled.sortedDescending(), settled)
    }

    @Test
    fun `alphabetical sort orders within status rank`() {
        val flat =
            items(grouping = ProjectGrouping.Flat, sort = ThreadSort.Alphabetical)
                .filterIsInstance<HomeListItem.Thread>()
                .map { it.thread.title }
        assertEquals(flat.size, flat.toSet().size)
    }

    @Test
    fun `queued threads render as queued rows`() {
        val queued = items(grouping = ProjectGrouping.Flat).filterIsInstance<HomeListItem.Queued>()
        assertEquals(threads.count { it.status == ThreadStatus.Queued }, queued.size)
    }

    @Test
    fun `environment label only appears when multiple environments are merged`() {
        val merged =
            items(grouping = ProjectGrouping.Flat).filterIsInstance<HomeListItem.Thread>()
        assertTrue(merged.any { it.environmentLabel != null })
        val scoped =
            items(HomeUiState(environmentId = HomeFixture.laptop), ProjectGrouping.Flat)
                .filterIsInstance<HomeListItem.Thread>()
        assertTrue(scoped.all { it.environmentLabel == null })
    }

    @Test
    fun `a thread with an unsent draft carries its first line`() {
        val result =
            items(
                grouping = ProjectGrouping.Flat,
                drafts = mapOf("thr-working" to ThreadDraft(text = "  \n also check the retry path\nsecond line")),
            )
                .filterIsInstance<HomeListItem.Thread>()
        assertEquals(
            "also check the retry path",
            result.single { it.thread.id.value == "thr-working" }.draftPreview,
        )
        // Every other row stays clean, so one draft cannot make the whole list
        // look edited.
        assertTrue(result.filter { it.thread.id.value != "thr-working" }.all { it.draftPreview == null })
    }

    @Test
    fun `same thread id in two environments keeps drafts isolated`() {
        val duplicated = threads.first().let { original ->
            listOf(
                original.copy(environmentId = HomeFixture.laptop, id = ThreadId("duplicate")),
                original.copy(environmentId = HomeFixture.devbox, id = ThreadId("duplicate")),
            )
        }
        val result =
            homeListItems(
                    duplicated,
                    projects,
                    environments,
                    HomeUiState(),
                    ProjectGrouping.Flat,
                    ThreadSort.Recent,
                    drafts =
                        mapOf(
                            "${HomeFixture.laptop.value}/duplicate" to ThreadDraft(text = "Laptop draft"),
                            "${HomeFixture.devbox.value}/duplicate" to ThreadDraft(text = "Desktop draft"),
                        ),
                )
                .filterIsInstance<HomeListItem.Thread>()
        assertEquals(
            mapOf(
                HomeFixture.laptop to "Laptop draft",
                HomeFixture.devbox to "Desktop draft",
            ),
            result.associate { it.thread.environmentId to it.draftPreview },
        )
    }

    @Test
    fun `an image-only draft still announces itself`() {
        val draft =
            ThreadDraft(
                text = "   ",
                attachments =
                    listOf(
                        ComposerAttachment("a", "shot.png", "image/png", 10, "file:///a"),
                        ComposerAttachment("b", "shot2.png", "image/png", 10, "file:///b"),
                    ),
            )
        assertEquals("2 attachments", draftPreview(draft))
        assertEquals("1 attachment", draftPreview(draft.copy(attachments = draft.attachments.take(1))))
    }

    @Test
    fun `an empty draft is not a draft`() {
        assertEquals(null, draftPreview(null))
        assertEquals(null, draftPreview(ThreadDraft()))
        assertEquals(null, draftPreview(ThreadDraft(text = "   \n  ")))
    }

    @Test
    fun `empty input produces an empty list`() {
        assertTrue(homeListItems(emptyList(), projects, environments, HomeUiState(), ProjectGrouping.ByProject, ThreadSort.Recent).isEmpty())
    }

    private fun newTask(
        prompt: String = "port the swipe actions",
        projectKey: String = "proj-t3code",
        attachments: List<ComposerAttachment> = emptyList(),
    ) =
        NewTaskDraft(
            environmentId = HomeFixture.laptop,
            projectKey = projectKey,
            prompt = prompt,
            attachments = attachments,
        )

    @Test
    fun `an unsent new task leads the list`() {
        val result = items(newTask = newTask())
        val draft = result.first() as HomeListItem.Draft
        assertEquals("port the swipe actions", draft.preview)
        assertEquals("s5code", draft.projectTitle)
        // Three environments in the fixture, so the row names which one.
        assertEquals("MacBook Pro", draft.environmentLabel)
    }

    @Test
    fun `a draft with no content is not offered`() {
        // A draft always carries a project and settings, so keying off those would
        // pin a row to the home screen of everyone who ever opened the flow.
        assertTrue(items(newTask = newTask(prompt = "  ")).none { it is HomeListItem.Draft })
        assertTrue(items(newTask = null).none { it is HomeListItem.Draft })
    }

    @Test
    fun `an image-only new task draft is still offered`() {
        val result =
            items(
                newTask =
                    newTask(
                        prompt = "",
                        attachments =
                            listOf(ComposerAttachment("a", "shot.png", "image/png", 10, "file:///a")),
                    )
            )
        assertEquals("1 attachment", (result.first() as HomeListItem.Draft).preview)
    }

    @Test
    fun `searching and filtering hide the draft row`() {
        // It has no status to match a filter and no title to match a search, so
        // leaving it above "no matches" would read as a result.
        assertTrue(
            items(state = HomeUiState(query = "pricing"), newTask = newTask())
                .none { it is HomeListItem.Draft }
        )
        assertTrue(
            items(state = HomeUiState(filter = ThreadFilter.Settled), newTask = newTask())
                .none { it is HomeListItem.Draft }
        )
    }

    @Test
    fun `a draft whose project is gone is still resumable`() {
        val draft = items(newTask = newTask(projectKey = "proj-deleted")).first() as HomeListItem.Draft
        assertEquals(null, draft.projectTitle)
        assertEquals("port the swipe actions", draft.preview)
    }
}
