package club.touchtech.s5code.kotlin.feature.home

import club.touchtech.s5code.kotlin.app.HomeUiState
import club.touchtech.s5code.kotlin.app.NewTaskDraft
import club.touchtech.s5code.kotlin.app.ThreadDraft
import club.touchtech.s5code.kotlin.data.HomePendingRequest
import club.touchtech.s5code.kotlin.model.Environment
import club.touchtech.s5code.kotlin.model.HomeListItem
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.ProjectGrouping
import club.touchtech.s5code.kotlin.model.SearchMatch
import club.touchtech.s5code.kotlin.model.SearchMatchSource
import club.touchtech.s5code.kotlin.model.ShelfKind
import club.touchtech.s5code.kotlin.model.ThreadFilter
import club.touchtech.s5code.kotlin.model.ThreadSort
import club.touchtech.s5code.kotlin.model.ThreadStatus
import club.touchtech.s5code.kotlin.model.ThreadSummary
import club.touchtech.s5code.kotlin.model.ThreadSearchMatch

/**
 * Pure home-list layout. Matching the RN client, this is the single place that
 * decides ordering, grouping, filtering, and which threads live behind the
 * snoozed/settled shelves — so the list can be unit-tested without Compose and
 * the UI stays a dumb renderer.
 */
fun homeListItems(
    threads: List<ThreadSummary>,
    projects: List<Project>,
    environments: List<Environment>,
    state: HomeUiState,
    grouping: ProjectGrouping,
    sort: ThreadSort,
    /** Unsent composer drafts, keyed by `environmentId/threadId` (legacy id accepted). */
    drafts: Map<String, ThreadDraft> = emptyMap(),
    /**
     * The unsent new-task draft, so an interrupted "new task" is one tap from home
     * rather than something the user has to walk the whole flow to find again.
     */
    newTask: NewTaskDraft? = null,
    /** Actual request payloads for attention rows; shell-only rows remain usable until loaded. */
    pendingRequests: List<HomePendingRequest> = emptyList(),
    /** Message-body matches returned by `orchestration.searchThreads`. */
    serverSearchMatches: List<ThreadSearchMatch> = emptyList(),
): List<HomeListItem> {
    val projectsById = projects.associateBy { it.environmentId.value to it.id.value }
    val environmentLabels = environments.associate { it.id.value to it.label }
    val pendingByThread =
        pendingRequests.associateBy { it.environmentId.value to it.threadId.value }
    val serverMatchByThread =
        serverSearchMatches.associateBy { it.environmentId.value to it.threadId.value }

    val visible =
        threads
            .asSequence()
            .filter { thread -> state.environmentId == null || thread.environmentId == state.environmentId }
            .filter { thread ->
                state.projectKey == null || thread.projectId.value == state.projectKey
            }
            .filter { thread -> matchesFilter(thread, state.filter) }
            .filter { thread ->
                matchesQuery(thread, state.query, projectsById, environmentLabels) ||
                    serverMatchByThread.containsKey(thread.environmentId.value to thread.id.value)
            }
            .toList()

    val snoozed = visible.filter { it.status == ThreadStatus.Snoozed }
    val settled = visible.filter { it.status == ThreadStatus.Settled }
    val active = visible - snoozed.toSet() - settled.toSet()

    // A search reveals what the shelves hide. Searching is asking for a specific
    // thread, and answering "no matches" while the match sits behind a collapsed
    // shelf is the worst of both. Same rule as `showAllThreads: hasSearchQuery` in
    // `apps/mobile/src/features/home/HomeScreen.tsx`; the toggle itself is left
    // alone, so clearing the search restores the shelves as the user left them.
    //
    // Picking a filter that *is* a shelf does the same, for a blunter reason: with
    // the Settled filter on and the shelf collapsed, the whole screen was one
    // collapsed header.
    val searching = state.query.isNotBlank()
    val snoozedExpanded =
        state.snoozedExpanded || searching || state.filter == ThreadFilter.Snoozed
    val settledExpanded =
        state.settledExpanded || searching || state.filter == ThreadFilter.Settled

    val items = mutableListOf<HomeListItem>()

    // Above everything, and only on the unfiltered list: it is not a thread, so it
    // has no status to match a filter and no title to match a search, and leaving
    // it pinned above "no matches" would read as a result.
    if (state.query.isBlank() && state.filter == ThreadFilter.All) {
        newTaskDraftItem(newTask, projectsById, environmentLabels, environments.size > 1)?.let {
            items += it
        }
    }

    fun emitThreads(list: List<ThreadSummary>) {
        list.forEach { thread ->
            val project = projectsById[thread.environmentId.value to thread.projectId.value]
            val environmentLabel =
                environmentLabels[thread.environmentId.value].takeIf {
                    environments.size > 1 && state.environmentId == null
                }
            val pending = pendingByThread[thread.environmentId.value to thread.id.value]
            when {
                pending?.approval != null ->
                    items +=
                        HomeListItem.PendingApprovalCard(
                            thread = thread,
                            project = project,
                            environmentLabel = environmentLabel,
                            approval = pending.approval,
                        )
                pending?.userInput != null ->
                    items +=
                        HomeListItem.PendingInputCard(
                            thread = thread,
                            project = project,
                            environmentLabel = environmentLabel,
                            request = pending.userInput,
                        )
                thread.status == ThreadStatus.Queued ->
                    items += HomeListItem.Queued(thread, project)
                else ->
                    items +=
                        HomeListItem.Thread(
                            thread = thread,
                            project = project,
                            environmentLabel = environmentLabel,
                            searchMatch =
                                searchMatch(thread, state.query, project, environmentLabel)
                                    ?: serverMatchByThread[
                                        thread.environmentId.value to thread.id.value
                                    ]?.let { match ->
                                        SearchMatch(match.source, match.snippet)
                                    },
                            draftPreview =
                                draftPreview(
                                    drafts["${thread.environmentId.value}/${thread.id.value}"]
                                        ?: drafts[thread.id.value]
                                ),
                        )
            }
        }
    }

    when (grouping) {
        ProjectGrouping.Flat -> emitThreads(active.sortedWith(comparator(sort)))
        ProjectGrouping.ByProject,
        ProjectGrouping.ByRepository -> {
            val groups =
                active.groupBy { thread ->
                    val project = projectsById[thread.environmentId.value to thread.projectId.value]
                    when (grouping) {
                        ProjectGrouping.ByRepository ->
                            project?.repository ?: project?.title ?: "Ungrouped"
                        else -> project?.title ?: "Ungrouped"
                    }
                }
            groups.entries
                .sortedWith(
                    compareByDescending<Map.Entry<String, List<ThreadSummary>>> { entry ->
                        entry.value.any { it.pinned }
                    }
                        .thenBy { it.key.lowercase() }
                )
                .forEach { (label, groupThreads) ->
                    items += HomeListItem.Section(label)
                    emitThreads(groupThreads.sortedWith(comparator(sort)))
                }
        }
    }

    if (snoozed.isNotEmpty()) {
        items +=
            HomeListItem.ShelfHeader(
                label = "Snoozed",
                count = snoozed.size,
                expanded = snoozedExpanded,
                kind = ShelfKind.Snoozed,
            )
        if (snoozedExpanded) emitThreads(snoozed.sortedWith(comparator(sort)))
    }
    if (settled.isNotEmpty()) {
        items +=
            HomeListItem.ShelfHeader(
                label = "Settled",
                count = settled.size,
                expanded = settledExpanded,
                kind = ShelfKind.Settled,
            )
        if (settledExpanded) emitThreads(settled.sortedWith(comparator(sort)))
    }
    return items
}

/**
 * The home row for an unsent new-task draft, or null when there is nothing to
 * resume.
 *
 * "Nothing to resume" is deliberately about content only: a draft always carries a
 * project and settings, so keying off those would put a permanent row on the home
 * screen of every user who ever opened the new-task flow.
 */
private fun newTaskDraftItem(
    draft: NewTaskDraft?,
    projectsById: Map<Pair<String, String>, Project>,
    environmentLabels: Map<String, String>,
    showEnvironment: Boolean,
): HomeListItem.Draft? {
    if (draft == null) return null
    val preview =
        draftPreview(ThreadDraft(text = draft.prompt, attachments = draft.attachments))
            ?: return null
    return HomeListItem.Draft(
        preview = preview,
        projectTitle =
            projectsById[draft.environmentId.value to draft.projectKey]?.title,
        environmentLabel =
            environmentLabels[draft.environmentId.value].takeIf { showEnvironment },
    )
}

/**
 * One line describing an unsent draft, or null when there is nothing to say.
 *
 * Mirrors the web sidebar's draft row (`SidebarDraftRow` in
 * `apps/web/src/components/Sidebar.tsx`): the first line of the prompt, falling back
 * to an attachment count so an image-only draft still announces itself rather than
 * rendering as an empty row.
 */
fun draftPreview(draft: ThreadDraft?): String? {
    if (draft == null) return null
    val firstLine = draft.text.lineSequence().firstOrNull { it.isNotBlank() }?.trim()
    if (!firstLine.isNullOrEmpty()) return firstLine
    val count = draft.attachments.size
    if (count == 0) return null
    return if (count == 1) "1 attachment" else "$count attachments"
}

/** Counts per filter chip, so the chips can show live totals. */
fun homeFilterCounts(threads: List<ThreadSummary>): Map<ThreadFilter, Int> =
    ThreadFilter.entries.associateWith { filter -> threads.count { matchesFilter(it, filter) } }

private fun matchesFilter(thread: ThreadSummary, filter: ThreadFilter): Boolean =
    when (filter) {
        ThreadFilter.All -> true
        ThreadFilter.Active ->
            thread.status == ThreadStatus.Working ||
                thread.status == ThreadStatus.Queued ||
                thread.status == ThreadStatus.Idle ||
                thread.status == ThreadStatus.Failed
        ThreadFilter.Pending ->
            thread.status == ThreadStatus.AwaitingApproval ||
                thread.status == ThreadStatus.AwaitingInput
        ThreadFilter.Snoozed -> thread.status == ThreadStatus.Snoozed
        ThreadFilter.Settled -> thread.status == ThreadStatus.Settled
    }

private fun matchesQuery(
    thread: ThreadSummary,
    query: String,
    projects: Map<Pair<String, String>, Project>,
    environmentLabels: Map<String, String>,
): Boolean {
    val needle = query.trim().lowercase()
    if (needle.isEmpty()) return true
    val project = projects[thread.environmentId.value to thread.projectId.value]
    val haystack =
        buildString {
                append(thread.title)
                append(' ')
                append(thread.branch.orEmpty())
                append(' ')
                append(thread.excerpt.orEmpty())
                append(' ')
                append(thread.model)
                append(' ')
                append(thread.provider.label)
                append(' ')
                append(project?.title.orEmpty())
                append(' ')
                append(project?.repository.orEmpty())
                append(' ')
                append(environmentLabels[thread.environmentId.value].orEmpty())
            }
            .lowercase()
    return needle.split(' ').filter(String::isNotEmpty).all(haystack::contains)
}

/**
 * Picks the first visible field that contains the whole query. Filtering still
 * accepts AND-ed terms across fields; this excerpt is explanatory presentation,
 * and a single contiguous match is the only substring a row can truthfully
 * highlight.
 */
fun searchMatch(
    thread: ThreadSummary,
    query: String,
    project: Project?,
    environmentLabel: String?,
): SearchMatch? {
    val needle = query.trim()
    if (needle.isEmpty()) return null
    return listOf(
            SearchMatchSource.Title to thread.title,
            SearchMatchSource.Excerpt to thread.excerpt,
            SearchMatchSource.Branch to thread.branch,
            SearchMatchSource.Project to project?.title,
            SearchMatchSource.Repository to project?.repository,
            SearchMatchSource.Environment to environmentLabel,
            SearchMatchSource.Provider to thread.provider.label,
            SearchMatchSource.Model to thread.model,
        )
        .firstNotNullOfOrNull { (source, text) ->
            text?.takeIf { it.contains(needle, ignoreCase = true) }?.let { SearchMatch(source, it) }
        }
}

/**
 * Pinned first, then attention-needing statuses, then the requested order. This
 * keeps approvals and failures reachable without scrolling on a busy home.
 */
private fun comparator(sort: ThreadSort): Comparator<ThreadSummary> {
    val base =
        compareByDescending<ThreadSummary> { it.pinned }
            .thenBy { statusRank(it.status) }
    return when (sort) {
        // Newest first within a status rank, on the instant rather than the label:
        // the label is rounded, so two threads an hour apart can share a bucket.
        ThreadSort.Recent -> base.thenByDescending { it.updatedAtMillis }
        ThreadSort.Created -> base.thenBy { it.id.value }
        ThreadSort.Alphabetical -> base.thenBy { it.title.lowercase() }
    }
}

private fun statusRank(status: ThreadStatus): Int =
    when (status) {
        ThreadStatus.AwaitingApproval -> 0
        ThreadStatus.AwaitingInput -> 1
        ThreadStatus.Failed -> 2
        ThreadStatus.Working -> 3
        ThreadStatus.Queued -> 4
        ThreadStatus.Idle -> 5
        ThreadStatus.Snoozed -> 6
        ThreadStatus.Settled -> 7
    }
