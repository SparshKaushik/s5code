package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.model.FeedEntry
import club.touchtech.s5code.kotlin.model.ToolState
import club.touchtech.s5code.kotlin.model.TurnInfo

/**
 * One row of the presented transcript.
 *
 * The feed the projection produces is the full history; this is what the list
 * actually renders. The two differ because a long run of tool calls is folded
 * behind a toggle, which needs a row of its own.
 */
sealed interface FeedRow {
    val key: String

    /**
     * [standIn] is set on a lone call shown in place of its group's toggle, as
     * RN's `groupStandIn`: the row takes the toggle's key and label, and opening
     * it opens the group too, so a second call joining keeps everything open.
     */
    data class Entry(
        val entry: FeedEntry,
        val standIn: GroupStandIn? = null,
        /** A turn with no work row of its own shows its diffstat on its answer. */
        val diffStat: TurnDiffStat? = null,
    ) : FeedRow {
        override val key: String
            get() = standIn?.let { "work-toggle:${it.groupId}" } ?: entry.id
    }

    data class GroupStandIn(val groupId: String, val label: String)

    /**
     * Discloses the tool calls folded away beneath it.
     *
     * [summary] is the collapsed-group label the RN client computes in
     * `appendToolGroupRows`: "Ran 4 commands and read 2 files", "Thought", or
     * the live "Running x" line while the trailing call is in flight. A lone
     * call has no toggle; see [Entry.standIn]. It does not change when the group
     * opens — RN keeps the summary as the expanded group's header — so there is
     * no "Show fewer" wording here.
     */
    data class WorkToggle(
        val groupId: String,
        val hiddenCount: Int,
        val expanded: Boolean,
        val summary: String,
        val thought: String? = null,
        /** The turn the group's rows belong to. */
        val turnId: String? = null,
        val diffStat: TurnDiffStat? = null,
    ) : FeedRow {
        override val key: String
            get() = "work-toggle:$groupId"
    }

    /**
     * Discloses a whole finished turn. Expanded turns get a second instance at
     * the bottom so a long turn can be collapsed without scrolling back up.
     */
    data class TurnFold(
        val turnId: String,
        val label: String,
        val expanded: Boolean,
        val placement: Placement = Placement.Header,
        val diffStat: TurnDiffStat? = null,
    ) : FeedRow {
        enum class Placement { Header, Footer }

        override val key: String
            get() = "turn-fold:$turnId:${placement.name.lowercase()}"

        /**
         *
         * The footer says what it does rather than repeating the duration: the
         * header above already stated how long the turn took, and a second
         * "Worked for 2m 5s" at the bottom reads as a second turn.
         */
        val text: String
            get() =
                if (placement == Placement.Footer) "Hide this turn's work" else label
    }

    /**
     * "Working for 12s", at the bottom of the transcript while a turn is in flight.
     *
     * Carries only the start, never the elapsed time: the row ticks itself, so a
     * running turn does not rebuild the whole presented list once a second.
     */
    data class Working(val startedAtMillis: Long) : FeedRow {
        override val key: String
            get() = "working-indicator"
    }

    /**
     * The bootstrap worktree's progress card, spliced into the presented rows by
     * the screen (it carries live subscription state, which `presentFeed` does
     * not know). It is not an entry: nothing about it is work the folds group.
     */
    data object WorktreeSetup : FeedRow {
        override val key: String
            get() = "worktree-setup"
    }
}

/**
 * A finished turn's checkpoint totals: RN's `TurnDiffStat`, the "+12 −3" a turn's
 * work row carries and opens the review from.
 */
data class TurnDiffStat(val files: Int, val additions: Int, val deletions: Int) {
    val accessibilityLabel: String
        get() =
            "Review $files file${if (files == 1) "" else "s"} changed, " +
                "$additions additions, $deletions deletions"
}

/**
 * Folds runs of adjacent work rows, mirroring `appendPresentedFeedEntry` in the RN
 * client and `MessagesTimeline.logic.ts` on the desktop/web side.
 *
 * Tool calls and reasoning fold, and the whole run folds behind its summary
 * toggle, the way RN's `work-toggle` row replaces its `activity-group`. A failed
 * call does not fold: `appendActivityGroupRows` passes `tone !== "error"` rows
 * into the run and renders the rest standalone, and an error folded away is an
 * error the user does not know about. Subagent rows stay visible for the same
 * reason RN's `agent-spawn` card flushes the run instead of joining it.
 *
 * [expandedGroups] holds the groups the user has opened, keyed by the group's first
 * row. That key is stable while the run grows downward, which is the direction a
 * live turn grows, so expanding a group does not snap shut on the next tool call.
 *
 * [latestTurn] and [expandedTurns] drive the coarser fold: a finished turn collapses
 * to its last assistant message under a "Worked for 2m" header. See [turnFolds].
 *
 * [activeWorkStartedAtMillis] appends the working row, as `activeWorkStartedAt` does
 * in `deriveThreadFeedPresentation`. It is unconditional when work is in flight: the
 * case it exists for is a turn that has produced nothing yet, and a transcript that
 * only says "working" once there is something to show says it exactly when it is no
 * longer needed.
 */
fun presentFeed(
    feed: List<FeedEntry>,
    expandedGroups: Set<String>,
    latestTurn: TurnInfo? = null,
    expandedTurns: Set<String> = emptySet(),
    activeWorkStartedAtMillis: Long? = null,
    /** Checkpoint totals by turn id; see [attachTurnDiffStats]. */
    turnDiffStats: Map<String, TurnDiffStat> = emptyMap(),
): List<FeedRow> {
    val folds = turnFolds(feed, latestTurn)
    val hidden =
        folds.values
            .filterNot { it.turnId in expandedTurns }
            .flatMapTo(mutableSetOf()) { it.hiddenIds }

    val rows = mutableListOf<FeedRow>()
    val expandedFoldByLastEntryId =
        folds.values
            .filter { it.turnId in expandedTurns }
            .associateBy { it.entryIds.last() }

    // The footer trigger goes after whichever row ends an expanded turn, and that
    // row can leave through any of the three emission paths below. Attaching it in
    // each of them by hand is what left a turn ending in a short run of tool calls
    // with a trigger at the top and none at the bottom.
    fun appendFooterFor(emitted: List<String>) {
        emitted.forEach { id ->
            expandedFoldByLastEntryId[id]?.let { fold ->
                rows +=
                    FeedRow.TurnFold(
                        turnId = fold.turnId,
                        label = fold.label,
                        expanded = true,
                        placement = FeedRow.TurnFold.Placement.Footer,
                    )
            }
        }
    }

    var index = 0
    while (index < feed.size) {
        val entry = feed[index]
        folds[entry.id]?.let { fold ->
            rows +=
                FeedRow.TurnFold(
                    turnId = fold.turnId,
                    label = fold.label,
                    expanded = fold.turnId in expandedTurns,
                )
        }
        if (entry.id in hidden) {
            index += 1
            continue
        }
        if (!isWorkRow(entry)) {
            rows += FeedRow.Entry(entry)
            appendFooterFor(listOf(entry.id))
            index += 1
            continue
        }

        // A run is adjacent in the *visible* feed, so a folded turn's leftover rows
        // do not split a run that reads as continuous.
        var end = index
        while (end + 1 < feed.size && (feed[end + 1].id in hidden || isWorkRow(feed[end + 1]))) end += 1
        val group = feed.subList(index, end + 1).filter { it.id !in hidden }
        index = end + 1
        if (group.isEmpty()) continue

        val groupId = "work-group:${group.first().id}"
        val expanded = groupId in expandedGroups
        val summary = workGroupSummary(group, activeWorkStartedAtMillis != null)
        // A lone call needs no toggle: its own row stands in for it and discloses
        // its details directly. A lone thought keeps the toggle, whose expansion
        // already shows the text.
        val single = group.singleOrNull()
        if (single != null && single !is FeedEntry.Reasoning) {
            rows += FeedRow.Entry(single, FeedRow.GroupStandIn(groupId, summary))
            appendFooterFor(group.map { it.id })
            continue
        }
        // RN emits the toggle first and the rows under it only once expanded.
        rows +=
            FeedRow.WorkToggle(
                groupId = groupId,
                // A grouped thought counts its messages, not its rows —
                // `activities.length + thoughtCount` in RN's mixed run.
                hiddenCount = group.sumOf { it.thoughtParts },
                expanded = expanded,
                summary = summary,
                thought = if (!expanded && activeWorkStartedAtMillis != null && end == feed.lastIndex)
                    group.filterIsInstance<FeedEntry.Reasoning>().lastOrNull()?.let {
                        liveThoughtLine(it.extraParts.lastOrNull() ?: it.text).takeIf(String::isNotBlank)
                    } else null,
                turnId = group.first().turnId,
            )
        if (expanded) {
            group.forEach { rows += FeedRow.Entry(it) }
        }
        // After the work toggle, not before: the toggle belongs to the rows above
        // it, and the turn trigger closes the whole turn.
        appendFooterFor(group.map { it.id })
    }
    // A bare `/compact` turn's compaction row is its live activity row already.
    val compactionCommandRun = latestTurn?.turnId?.let { runId ->
        feed.any { it is FeedEntry.Note && it.compactionCommand && it.turnId == runId }
    } == true
    if (activeWorkStartedAtMillis != null && !compactionCommandRun) rows += FeedRow.Working(activeWorkStartedAtMillis)
    return attachTurnDiffStats(rows, feed, turnDiffStats)
}

/**
 * Puts each turn's diffstat on one row, as `deriveThreadFeedPresentation` does: the
 * turn's fold header when it has one, else its first work toggle, else its answer.
 * A turn with no assistant answer shows none — RN keys the checkpoint to the
 * answer, and a stat floating under a prompt would read as the user's change.
 */
internal fun attachTurnDiffStats(
    rows: List<FeedRow>,
    feed: List<FeedEntry>,
    stats: Map<String, TurnDiffStat>,
): List<FeedRow> {
    if (stats.isEmpty()) return rows
    val answerIdByTurn = mutableMapOf<String, String>()
    feed.forEach { entry ->
        val turnId = entry.turnId
        if (entry is FeedEntry.AgentMessage && turnId != null && turnId in stats) {
            answerIdByTurn[turnId] = entry.id
        }
    }
    if (answerIdByTurn.isEmpty()) return rows
    val target = mutableMapOf<String, Int>()
    rows.forEachIndexed { index, row ->
        if (
            row is FeedRow.TurnFold && row.placement == FeedRow.TurnFold.Placement.Header &&
                row.turnId in answerIdByTurn
        ) {
            target[row.turnId] = index
        }
    }
    rows.forEachIndexed { index, row ->
        val turnId = (row as? FeedRow.WorkToggle)?.turnId ?: return@forEachIndexed
        if (turnId in answerIdByTurn) target.putIfAbsent(turnId, index)
    }
    rows.forEachIndexed { index, row ->
        val entry = (row as? FeedRow.Entry)?.entry ?: return@forEachIndexed
        val turnId = entry.turnId ?: return@forEachIndexed
        if (answerIdByTurn[turnId] == entry.id) target.putIfAbsent(turnId, index)
    }
    if (target.isEmpty()) return rows
    return rows.toMutableList().apply {
        target.forEach { (turnId, index) ->
            val stat = stats.getValue(turnId)
            this[index] =
                when (val row = this[index]) {
                    is FeedRow.TurnFold -> row.copy(diffStat = stat)
                    is FeedRow.WorkToggle -> row.copy(diffStat = stat)
                    is FeedRow.Entry -> row.copy(diffStat = stat)
                    else -> row
                }
        }
    }
}

/**
 * One finished turn, folded down to its answer.
 *
 * [hiddenIds] is everything in the turn except its last assistant message, and the
 * fold is anchored on the turn's first row so the header renders where the turn
 * began.
 */
data class TurnFold(
    val turnId: String,
    val anchorId: String,
    val hiddenIds: Set<String>,
    val entryIds: List<String>,
    val label: String,
)

/**
 * Which turns fold, keyed by the row the header goes above. Ported from
 * `deriveThreadFeedTurnFolds` in `apps/mobile/src/lib/threadActivity.ts`.
 *
 * Three turns never fold, and each exclusion is load-bearing:
 *
 * - **the open turn**, because folding work the agent is still doing hides the only
 *   evidence that anything is happening;
 * - **a turn that is still streaming**, for the same reason, and because the row
 *   heights would thrash as text arrives;
 * - **a turn with nothing to hide** — a turn that is only its answer would get a
 *   header that discloses zero rows.
 *
 * A turn with no assistant message hides everything: the work happened, it produced
 * no answer, and "Worked for 40s" with nothing under it is the honest summary.
 */
fun turnFolds(feed: List<FeedEntry>, latestTurn: TurnInfo?): Map<String, TurnFold> {
    val openTurnId = latestTurn?.takeIf { !it.settled }?.turnId

    // Where each run's own prompt sits. A steer joins a run already under way,
    // so only the prompt that started the run counts.
    val promptIndexByTurn = mutableMapOf<String, Int>()
    feed.forEachIndexed { index, entry ->
        if (entry !is FeedEntry.UserMessage) return@forEachIndexed
        if (entry.inputIntent == "steer" || entry.inputIntent == "promoted_queued_to_steer") return@forEachIndexed
        entry.promptTurnId?.let { promptIndexByTurn.putIfAbsent(it, index) }
    }

    // Only the answer a turn ended on stays outside the fold, matching
    // `terminalAssistantMessageIdByRun`: progress notes the agent wrote along the
    // way are settled work like its tool calls.
    val terminalAssistantIdByTurn = mutableMapOf<String, String>()

    // Grouped in feed order, so "first row" and "last answer" are positional rather
    // than derived from timestamps, which tie.
    val groups = LinkedHashMap<String, MutableList<FeedEntry>>()
    var boundary: Long? = null
    var responseKey: String? = null
    val boundaries = mutableMapOf<String, Long?>()
    feed.forEachIndexed { index, entry ->
        if (entry is FeedEntry.UserMessage) {
            // The prompt is where the user's clock starts, which is what "worked
            // for" should measure — not when the provider got round to the turn.
            boundary = entry.atMillis
            responseKey = entry.promptTurnId
            return@forEachIndexed
        }
        val ownTurnId = entry.turnId ?: return@forEachIndexed
        // Resuming a subagent moves its item to the resuming run, but the item
        // keeps its place in the turn that started it. It folds with that turn,
        // so the later run's fold still opens under its own prompt.
        val turnId = responseKey?.takeIf { (promptIndexByTurn[ownTurnId] ?: -1) > index } ?: ownTurnId
        if (turnId !in groups) {
            groups[turnId] = mutableListOf()
            boundaries[turnId] = boundary
            boundary = null
        }
        groups.getValue(turnId) += entry
        if (entry is FeedEntry.AgentMessage) {
            terminalAssistantIdByTurn[turnId] = entry.id
        }
    }

    val folds = mutableMapOf<String, TurnFold>()
    groups.forEach { (turnId, entries) ->
        if (turnId == openTurnId) return@forEach
        if (entries.any { it is FeedEntry.AgentMessage && it.streaming }) return@forEach
        // Answered questions and proposed plans never hide — RN keeps
        // `questionAnswer` and prominent activities outside the fold.
        val hidden =
            entries
                .filter {
                    it.id != terminalAssistantIdByTurn[turnId] &&
                        it !is FeedEntry.QuestionAnswer && it !is FeedEntry.McpApp &&
                        it !is FeedEntry.ProposedPlan &&
                        // Compaction rows never fold away (`deriveThreadFeedRunFolds`).
                        !(it is FeedEntry.Note && it.compaction) &&
                        !(it is FeedEntry.Subagent && it.active)
                }
                .map { it.id }
                .toSet()
        if (hidden.isEmpty()) return@forEach
        // A turn whose hidden rows are only thoughts (or a lone compaction)
        // does not collapse: "Worked for 40s" hiding nothing real is the lie RN
        // guards on with `hidesFoldableWork`.
        val hidesFoldableWork =
            entries.any {
                it.id in hidden &&
                    !(it is FeedEntry.Reasoning && it.thought) &&
                    !(it is FeedEntry.Note && it.compaction)
            }
        if (!hidesFoldableWork) return@forEach
        val anchor = entries.first { it.id in hidden }
        val answer = entries.lastOrNull { it.id == terminalAssistantIdByTurn[turnId] }
        val interrupted = latestTurn?.turnId == turnId && latestTurn.interrupted
        // The turn record's own clock is only trusted when it has both ends, as in RN:
        // a started-but-not-completed record on a turn the feed says is finished is a
        // stale snapshot, and mixing one end of it with a feed timestamp produces a
        // duration that belongs to neither.
        val turnClock =
            latestTurn
                ?.takeIf { it.turnId == turnId }
                ?.let { turn ->
                    val started = turn.startedAtMillis
                    val completed = turn.completedAtMillis
                    if (started != null && completed != null) started to completed else null
                }
        // The fallback start is the turn's first row, not the anchor: RN uses
        // `firstEntry.createdAt`, and the anchor moved to the first hidden row.
        val startedAt = turnClock?.first ?: boundaries[turnId] ?: entries.first().atMillis
        val endedAt =
            turnClock?.second
                ?: maxOf(answer?.endedAtMillis ?: 0L, entries.maxOf { it.endedAtMillis })
        folds[anchor.id] =
            TurnFold(
                turnId = turnId,
                anchorId = anchor.id,
                hiddenIds = hidden,
                entryIds = entries.map { it.id },
                label = turnFoldLabel(startedAt, endedAt, interrupted),
            )
    }
    return folds
}

/**
 * "Worked for 2m 5s", or the interrupted wording. Duration is dropped rather than
 * guessed when the clocks disagree: a turn labelled "Worked for 0ms" reads as a bug.
 */
internal fun turnFoldLabel(startedAt: Long, endedAt: Long, interrupted: Boolean): String {
    // Only the ordering is checked, not that the clock is nonzero: epoch 0 is a
    // legitimate timestamp in tests and on a machine with a bad clock, and dropping
    // the duration there would be a silent wrong answer.
    val duration = if (endedAt > startedAt) formatDuration(endedAt - startedAt) else null
    return when {
        interrupted && duration != null -> "You stopped after $duration"
        interrupted -> "You stopped this response"
        duration != null -> "Worked for $duration"
        else -> "Worked"
    }
}

/** Ported from `formatDuration` in `packages/shared/src/orchestrationTiming.ts`. */
internal fun formatDuration(millis: Long): String {
    if (millis < 0) return "0ms"
    if (millis < 1_000) return "${maxOf(1L, millis)}ms"
    if (millis < 10_000) {
        val tenths = Math.round(millis / 100.0) / 10.0
        return if (tenths >= 10) "10s" else String.format(java.util.Locale.US, "%.1fs", tenths)
    }
    // The unit is chosen after rounding so 59.5 seconds never renders as "60s".
    val totalSeconds = Math.round(millis / 1_000.0)
    if (totalSeconds < 60) return "${totalSeconds}s"
    val hours = totalSeconds / 3_600
    val minutes = (totalSeconds % 3_600) / 60
    val seconds = totalSeconds % 60
    return buildList {
        if (hours > 0) add("${hours}h")
        if (minutes > 0) add("${minutes}m")
        if (seconds > 0) add("${seconds}s")
    }.joinToString(" ")
}

/**
 * Whether an entry is part of a run of work rather than conversation.
 *
 * Messages, plans, errors, and subagent rows break a run: they are the things you
 * scroll to read. Subagents break it for the same reason RN's `agent-spawn` card
 * does — a spawn flushes the tool-call run on either side instead of folding into
 * it.
 */
private fun isWorkRow(entry: FeedEntry): Boolean =
    (entry is FeedEntry.ToolCall && entry.state != ToolState.Failed) ||
        entry is FeedEntry.Reasoning

/* ── Folded-run summaries ────────────────────────────────────────────────
 * The block below is `summarizeToolGroup` and friends from
 * `packages/client-runtime/src/work-log/presentation.ts`, ported against the
 * payload fields Projection extracts onto FeedEntry.ToolCall. Labels are
 * verbatim so a user reading the same thread on both clients sees the same
 * sentence.
 */

private enum class ToolGroupAction {
    LinkPr,
    UnlinkPr,
    WatchPr,
    UnwatchPr,
    ListPrs,
    Read,
    Edit,
    Command,
    Browser,
    Device,
    CodeSearch,
    Search,
    Other,
    Update,
}

private enum class T3ToolIcon { PullRequest, Browser, Device, T3Code }

private data class T3ToolPresentation(
    val displayName: String,
    val icon: T3ToolIcon,
    val action: ToolGroupAction?,
)

/** A `T3_MCP_TOOLS` entry: [action, running, completed, detail] plus its icon and PR bucket. */
private data class T3McpTool(
    val labels: List<String>,
    val icon: T3ToolIcon,
    val action: ToolGroupAction?,
)

private fun t3Tool(
    labels: List<String>,
    icon: T3ToolIcon = T3ToolIcon.T3Code,
    action: ToolGroupAction? = null,
) = T3McpTool(labels, icon, action)

/** Verbatim from `T3_MCP_TOOLS` in `packages/shared/src/t3McpToolPresentation.ts`. */
private val T3_MCP_TOOLS: Map<String, T3McpTool> =
    mapOf(
        "link_pull_request" to
            t3Tool(listOf("Link", "Linking", "Linked", "a pull request"), T3ToolIcon.PullRequest, action = ToolGroupAction.LinkPr),
        "unlink_pull_request" to
            t3Tool(listOf("Unlink", "Unlinking", "Unlinked", "a pull request"), T3ToolIcon.PullRequest, action = ToolGroupAction.UnlinkPr),
        "list_thread_pull_requests" to
            t3Tool(listOf("Check", "Checking", "Checked", "linked pull requests"), T3ToolIcon.PullRequest, action = ToolGroupAction.ListPrs),
        "watch_pull_request" to
            t3Tool(listOf("Watch", "Watching", "Watching", "a pull request"), T3ToolIcon.PullRequest, action = ToolGroupAction.WatchPr),
        "unwatch_pull_request" to
            t3Tool(listOf("Stop watching", "Unwatching", "Stopped watching", "a pull request"), T3ToolIcon.PullRequest, action = ToolGroupAction.UnwatchPr),
        "write_pull_request_guides" to
            t3Tool(listOf("Write", "Writing", "Started writing", "review guides"), T3ToolIcon.PullRequest),
        "read_pull_request_guide" to
            t3Tool(listOf("Read", "Reading", "Read", "a review guide"), T3ToolIcon.PullRequest),
        "orchestrator_capabilities" to t3Tool(listOf("Get", "Getting", "Got", "orchestration capabilities")),
        "delegate_task" to t3Tool(listOf("Delegate", "Delegating", "Delegated", "a child task")),
        "task_status" to t3Tool(listOf("Get", "Getting", "Got", "delegated task status")),
        "task_cancel" to
            t3Tool(listOf("Cancel", "Canceling", "Requested cancellation of", "a delegated task")),
        "schedule_task" to t3Tool(listOf("Schedule", "Scheduling", "Scheduled", "a recurring task")),
        "list_scheduled_tasks" to t3Tool(listOf("List", "Listing", "Listed", "scheduled tasks")),
        "update_scheduled_task" to t3Tool(listOf("Update", "Updating", "Updated", "a scheduled task")),
        "delete_scheduled_task" to
            t3Tool(listOf("Delete", "Deleting", "Requested deletion of", "a scheduled task")),
        "request_secret" to t3Tool(listOf("Ask for", "Asking for", "Asked for", "a secret")),
        "create_threads" to t3Tool(listOf("Create", "Creating", "Created", "T3 threads")),
        "t3_thread_start" to t3Tool(listOf("Start", "Starting", "Started", "a T3 thread")),
        "t3_thread_list" to t3Tool(listOf("List", "Listing", "Listed", "T3 threads")),
        "t3_thread_read" to t3Tool(listOf("Read", "Reading", "Read", "a T3 thread")),
        "t3_thread_send" to t3Tool(listOf("Send", "Sending", "Sent", "to a T3 thread")),
        "t3_thread_wait" to t3Tool(listOf("Wait", "Waiting", "Waited", "for a T3 thread")),
        "t3_workflow_stop" to t3Tool(listOf("Stop", "Stopping", "Requested a stop for", "a workflow")),
        "t3_background_wait" to t3Tool(listOf("Update", "Updating", "Updated", "background waits")),
        "t3_thread_interrupt" to
            t3Tool(listOf("Interrupt", "Interrupting", "Requested an interrupt of", "a T3 thread")),
        "t3_worktree_handoff" to
            t3Tool(listOf("Hand off", "Handing off", "Handed off", "thread to a git worktree")),
        "t3_worktree_status" to t3Tool(listOf("Get", "Getting", "Got", "thread worktree status")),
        "preview_status" to
            t3Tool(listOf("Get", "Getting", "Got", "preview browser status"), T3ToolIcon.Browser),
        "preview_open" to
            t3Tool(listOf("Open", "Opening", "Opened", "a page in the preview browser"), T3ToolIcon.Browser),
        "preview_navigate" to
            t3Tool(listOf("Navigate", "Navigating", "Navigated", "the preview browser"), T3ToolIcon.Browser),
        "preview_dialog" to
            t3Tool(listOf("Respond", "Responding", "Responded", "to a preview browser dialog"), T3ToolIcon.Browser),
        "preview_snapshot" to
            t3Tool(listOf("Take a snapshot of", "Taking a snapshot of", "Took a snapshot of", "the preview page"), T3ToolIcon.Browser),
        "preview_click" to
            t3Tool(listOf("Click", "Clicking", "Clicked", "in the preview browser"), T3ToolIcon.Browser),
        "preview_press" to
            t3Tool(listOf("Press", "Pressing", "Pressed", "a key in the preview browser"), T3ToolIcon.Browser),
        "preview_type" to
            t3Tool(listOf("Type", "Typing", "Typed", "in the preview browser"), T3ToolIcon.Browser),
        "preview_hover" to
            t3Tool(listOf("Hover", "Hovering", "Hovered", "in the preview browser"), T3ToolIcon.Browser),
        "preview_select" to
            t3Tool(listOf("Choose", "Choosing", "Chose", "an option in the preview browser"), T3ToolIcon.Browser),
        "preview_drag" to
            t3Tool(listOf("Drag", "Dragging", "Dragged", "in the preview browser"), T3ToolIcon.Browser),
        "preview_upload" to
            t3Tool(listOf("Upload", "Uploading", "Uploaded", "files to the preview browser"), T3ToolIcon.Browser),
        "preview_scroll" to
            t3Tool(listOf("Scroll", "Scrolling", "Scrolled", "the preview browser"), T3ToolIcon.Browser),
        "preview_resize" to
            t3Tool(listOf("Resize", "Resizing", "Resized", "the preview browser"), T3ToolIcon.Browser),
        "preview_evaluate" to
            t3Tool(listOf("Evaluate", "Evaluating", "Evaluated", "script in the preview browser"), T3ToolIcon.Browser),
        "preview_wait_for" to
            t3Tool(listOf("Wait", "Waiting", "Waited", "for the preview page"), T3ToolIcon.Browser),
        "preview_set_appearance" to
            t3Tool(listOf("Set", "Setting", "Set", "preview browser appearance"), T3ToolIcon.Browser),
        "preview_recording_start" to
            t3Tool(listOf("Start", "Starting", "Started", "recording the preview browser"), T3ToolIcon.Browser),
        "preview_recording_stop" to
            t3Tool(listOf("Stop", "Stopping", "Stopped", "recording the preview browser"), T3ToolIcon.Browser),
        "device_list" to
            t3Tool(listOf("List", "Listing", "Listed", "simulators and emulators"), T3ToolIcon.Device),
        "device_open" to
            t3Tool(listOf("Open", "Opening", "Opened", "a device in the Device panel"), T3ToolIcon.Device),
        "device_screenshot" to
            t3Tool(listOf("Take a screenshot of", "Taking a screenshot of", "Took a screenshot of", "the device"), T3ToolIcon.Device),
        "device_close" to t3Tool(listOf("Close", "Closing", "Closed", "a device"), T3ToolIcon.Device),
        "run_scheduled_task_now" to
            t3Tool(listOf("Run", "Running", "Requested a run of", "a scheduled task")),
        "t3_queue_list" to t3Tool(listOf("List", "Listing", "Listed", "queued messages")),
        "t3_queue_read" to t3Tool(listOf("Read", "Reading", "Read", "a queued message")),
        "t3_queue_edit" to t3Tool(listOf("Edit", "Editing", "Edited", "a queued message")),
        "t3_queue_cancel" to
            t3Tool(listOf("Cancel", "Canceling", "Requested cancellation of", "a queued run")),
        "t3_queue_reorder" to t3Tool(listOf("Reorder", "Reordering", "Reordered", "a queued run")),
        "t3_queue_promote_to_steer" to
            t3Tool(listOf("Steer with", "Steering with", "Requested steering with", "a queued message")),
        "t3_pending_request_list" to t3Tool(listOf("List", "Listing", "Listed", "pending questions")),
        "t3_pending_request_read" to t3Tool(listOf("Read", "Reading", "Read", "pending questions")),
        "t3_pending_request_respond" to
            t3Tool(listOf("Answer", "Answering", "Answered", "pending questions")),
        "t3_thread_configuration" to t3Tool(listOf("Read", "Reading", "Read", "thread configuration")),
        "t3_thread_configure" to t3Tool(listOf("Set", "Setting", "Set", "thread model")),
        "t3_thread_fork" to t3Tool(listOf("Fork", "Forking", "Requested a fork of", "this thread")),
        "t3_thread_merge_back" to
            t3Tool(listOf("Merge", "Merging", "Requested a merge of", "thread context")),
        "t3_thread_search" to t3Tool(listOf("Search", "Searching", "Searched", "thread content")),
        "t3_thread_transfers" to t3Tool(listOf("Read", "Reading", "Read", "thread transfers")),
        "t3_thread_organize" to t3Tool(listOf("Organize", "Organizing", "Organized", "a thread")),
        "t3_thread_update" to t3Tool(listOf("Update", "Updating", "Updated", "T3 thread metadata")),
        "t3_worktree_list" to t3Tool(listOf("List", "Listing", "Listed", "workspace branches")),
        "t3_preview_list" to t3Tool(listOf("List", "Listing", "Listed", "preview tabs"), T3ToolIcon.Browser),
        "t3_preview_close" to
            t3Tool(listOf("Close", "Closing", "Closed", "a preview tab"), T3ToolIcon.Browser),
        "t3_environment_read" to t3Tool(listOf("Read", "Reading", "Read", "environment preferences")),
        "t3_environment_preferences_update" to
            t3Tool(listOf("Update", "Updating", "Updated", "environment preferences")),
        "t3_thread_launch" to t3Tool(listOf("Launch", "Launching", "Launched", "a project thread")),
        "t3_project_list" to t3Tool(listOf("List", "Listing", "Listed", "projects")),
        "t3_project_read" to t3Tool(listOf("Read", "Reading", "Read", "a project")),
        "t3_project_create" to t3Tool(listOf("Register", "Registering", "Registered", "a project")),
        "t3_project_update" to t3Tool(listOf("Update", "Updating", "Updated", "a project")),
        "t3_project_delete" to t3Tool(listOf("Delete", "Deleting", "Deleted", "a project")),
        "t3_project_clone" to t3Tool(listOf("Clone", "Cloning", "Cloned", "a repository")),
        "t3_attachment_prepare_upload" to
            t3Tool(listOf("Prepare", "Preparing", "Prepared", "an attachment upload")),
        "t3_attachment_discard" to
            t3Tool(listOf("Discard", "Discarding", "Discarded", "a pending attachment")),
        "t3_thread_send_attachments" to t3Tool(listOf("Send", "Sending", "Sent", "attachments")),
        "html_preview" to t3Tool(listOf("Preview", "Previewing", "Previewed", "an HTML page")),
        "html_render" to t3Tool(listOf("Render", "Rendering", "Rendered", "an HTML page")),
    )

private val T3_MCP_PREFIX =
    Regex("^(?:mcp__(?:t3-code|t3_code|t3code)__|(?:t3-code|t3_code|t3code)(?:[.:/]|\\s*·\\s*))", RegexOption.IGNORE_CASE)

internal fun normalizeCompactToolLabel(value: String): String =
    value.replace(Regex("\\s+(?:complete|completed)\\s*$", RegexOption.IGNORE_CASE), "").trim()

/**
 * `resolveT3McpToolPresentation`: null when [value] is not a T3 Code MCP tool.
 * [status] is the lifecycle status (`inProgress`/`completed`/`failed`/
 * `declined`/`stopped`); anything else falls through to the running verb.
 */
private fun t3ToolPresentation(
    value: String?,
    status: String?,
    prNumber: Int?,
): T3ToolPresentation? {
    if (value.isNullOrEmpty()) return null
    val name = normalizeCompactToolLabel(value).replace(T3_MCP_PREFIX, "")
    val tool = T3_MCP_TOOLS[name] ?: return null
    val (action, running, completed, detail) = tool.labels
    val verb =
        when (status) {
            "inProgress" -> running
            "completed" -> completed
            "failed" -> "Failed to ${action.lowercase()}"
            "declined" -> "Declined to ${action.lowercase()}"
            "stopped" -> "Stopped ${running.lowercase()}"
            else -> running
        }
    val actionKind = tool.action
    val target =
        if (actionKind != null && actionKind != ToolGroupAction.ListPrs && prNumber != null) {
            "PR #$prNumber"
        } else {
            detail
        }
    return T3ToolPresentation("$verb $target", tool.icon, actionKind)
}

private val TOOL_LIFECYCLE_ITEM_TYPES =
    setOf(
        "command_execution",
        "file_change",
        "mcp_tool_call",
        "dynamic_tool_call",
        "collab_agent_tool_call",
        "web_search",
        "image_view",
    )

private fun FeedEntry.ToolCall.t3Presentation(statusOverride: String? = null): T3ToolPresentation? {
    val status =
        statusOverride
            ?: lifecycleStatus
            ?: when (state) {
                ToolState.Running -> "inProgress"
                ToolState.Succeeded -> "completed"
                ToolState.Failed -> "failed"
            }
    return t3ToolPresentation(toolName, status, prNumber)
        ?: t3ToolPresentation(toolTitle, status, prNumber)
        ?: t3ToolPresentation(name, status, prNumber)
}

private val WORKSPACE_IMAGE_PREVIEW_EXTENSIONS =
    listOf(".avif", ".gif", ".ico", ".jpeg", ".jpg", ".png", ".svg", ".webp")

private fun isWorkspaceImagePreviewPath(path: String): Boolean {
    val lower = path.substringBefore('?').substringBefore('#').lowercase()
    return WORKSPACE_IMAGE_PREVIEW_EXTENSIONS.any { lower.endsWith(it) }
}

/** `workLogEntryIsLocalCodeSearch`: a web-search row whose title is a grep call. */
private fun FeedEntry.ToolCall.isLocalCodeSearch(): Boolean =
    itemType == "web_search" &&
        Regex("\\bgrep\\b", RegexOption.IGNORE_CASE)
            .containsMatchIn(normalizeCompactToolLabel(toolTitle ?: name))

/** `toolGroupAction`: which verb bucket a call counts toward in the summary. */
private fun FeedEntry.ToolCall.groupAction(): ToolGroupAction {
    if (sourceKind == "approval.requested" ||
        sourceKind == "approval.resolved" ||
        sourceKind == "provider.approval.respond.failed"
    ) {
        return ToolGroupAction.Update
    }
    val presentation = t3Presentation()
    if (presentation?.action != null) return presentation.action
    if (presentation?.icon == T3ToolIcon.Browser) return ToolGroupAction.Browser
    if (presentation?.icon == T3ToolIcon.Device) return ToolGroupAction.Device
    if (requestKind == "file-read" ||
        itemType == "image_view" ||
        (itemType == "dynamic_tool_call" &&
            toolTitle?.trim()?.lowercase() == "read file")
    ) {
        return ToolGroupAction.Read
    }
    if (requestKind == "file-change" || itemType == "file_change" || changedFiles.isNotEmpty()) {
        return ToolGroupAction.Edit
    }
    if (requestKind == "command" || itemType == "command_execution" || command != null) {
        return ToolGroupAction.Command
    }
    if (isLocalCodeSearch()) return ToolGroupAction.CodeSearch
    if (itemType == "web_search") return ToolGroupAction.Search
    // Every FeedEntry.ToolCall is tool-like; the "update" bucket RN reserves for
    // non-tool rows is unreachable here.
    return ToolGroupAction.Other
}

private fun toolGroupActionCount(action: ToolGroupAction, entries: List<FeedEntry>): Int {
    if (action != ToolGroupAction.Edit) return entries.size
    val changedFiles = mutableSetOf<String>()
    var editsWithoutFileDetails = 0
    for (entry in entries) {
        val files = (entry as? FeedEntry.ToolCall)?.changedFiles.orEmpty()
        if (files.isEmpty()) {
            editsWithoutFileDetails += 1
            continue
        }
        changedFiles.addAll(files)
    }
    return changedFiles.size + editsWithoutFileDetails
}

private fun toolGroupActionLabel(action: ToolGroupAction, count: Int): String =
    when (action) {
        ToolGroupAction.LinkPr ->
            "Linked $count ${if (count == 1) "pull request" else "pull requests"}"
        ToolGroupAction.UnlinkPr ->
            "Unlinked $count ${if (count == 1) "pull request" else "pull requests"}"
        ToolGroupAction.WatchPr ->
            "Watching $count ${if (count == 1) "pull request" else "pull requests"}"
        ToolGroupAction.UnwatchPr ->
            "Stopped watching $count ${if (count == 1) "pull request" else "pull requests"}"
        ToolGroupAction.ListPrs ->
            if (count == 1) "Checked linked pull requests"
            else "Checked linked pull requests $count times"
        ToolGroupAction.Read -> "Read $count ${if (count == 1) "file" else "files"}"
        ToolGroupAction.Edit -> "Changed $count ${if (count == 1) "file" else "files"}"
        ToolGroupAction.Command -> "Ran $count ${if (count == 1) "command" else "commands"}"
        ToolGroupAction.Device ->
            "Used device controls $count ${if (count == 1) "time" else "times"}"
        ToolGroupAction.Browser -> "Used browser $count ${if (count == 1) "time" else "times"}"
        ToolGroupAction.Search ->
            "Searched the web $count ${if (count == 1) "time" else "times"}"
        ToolGroupAction.CodeSearch -> "Searched code $count ${if (count == 1) "time" else "times"}"
        ToolGroupAction.Other -> "Used $count ${if (count == 1) "tool" else "tools"}"
        ToolGroupAction.Update -> "Received $count ${if (count == 1) "update" else "updates"}"
    }

/**
 * `summarizeToolGroup`: integration sources are named up front ("Used GitHub
 * integration"), the rest bucket by action and join as an English list.
 */
private fun summarizeToolGroup(entries: List<FeedEntry>): String {
    val sources = LinkedHashMap<String, String>()
    val grouped = LinkedHashMap<ToolGroupAction, MutableList<FeedEntry>>()
    for (entry in entries) {
        if (entry is FeedEntry.ToolCall &&
            entry.toolSourceName != null &&
            entry.t3Presentation()?.icon != T3ToolIcon.PullRequest
        ) {
            sources[entry.toolSourceName] = entry.toolSourceKind.orEmpty()
            continue
        }
        // Thoughts never reach the verb summary: RN counts them into the
        // hidden count only, and a run of nothing else labels itself "Thought".
        if (entry is FeedEntry.Reasoning && entry.thought) continue
        val action =
            if (entry is FeedEntry.ToolCall) entry.groupAction()
            // Non-tool rows folded into the run (progress ticks) count toward "other".
            else ToolGroupAction.Other
        grouped.getOrPut(action) { mutableListOf() } += entry
    }
    val labels =
        grouped.map { (action, actionEntries) ->
            toolGroupActionLabel(action, toolGroupActionCount(action, actionEntries))
        }.toMutableList()
    if (sources.isNotEmpty()) {
        val names = sources.keys.toList()
        val formattedNames =
            when (names.size) {
                1 -> names[0]
                2 -> names.joinToString(" and ")
                else -> "${names.dropLast(1).joinToString(", ")}, and ${names.last()}"
            }
        val allIntegrations = sources.values.all { it == "integration" }
        labels.add(
            0,
            "Used $formattedNames" +
                if (allIntegrations) {
                    if (sources.size == 1) " integration" else " integrations"
                } else {
                    ""
                },
        )
    }
    val sentenceLabels =
        labels.mapIndexed { index, label ->
            if (index == 0) label else label.replaceFirstChar { it.lowercase() }
        }
    return when (sentenceLabels.size) {
        0 -> ""
        1 -> sentenceLabels[0]
        2 -> sentenceLabels.joinToString(" and ")
        else ->
            "${sentenceLabels.dropLast(1).joinToString(", ")}, and ${sentenceLabels.last()}"
    }
}

/** First token of the command — a light stand-in for RN's `commandProgramName`. */
private fun commandProgramName(command: String): String? =
    command.trim().split(Regex("\\s+"), limit = 2).firstOrNull()
        ?.substringAfterLast('/')
        ?.takeIf { it.isNotEmpty() }

/**
 * `liveToolActivitySummary`: the trailing group's present-tense label while its
 * latest call is still running ("Running bash", "Linked a pull request").
 */
private fun liveToolCallSummary(entry: FeedEntry.ToolCall): String {
    val status =
        when (entry.lifecycleStatus) {
            "failed", "declined", "stopped" -> entry.lifecycleStatus
            else -> "inProgress"
        }
    entry.t3Presentation(status)?.let { return it.displayName }
    val command = entry.command?.trim()
    if (!command.isNullOrEmpty()) {
        val verb =
            when (status) {
                "inProgress" -> "Running"
                "failed" -> "Failed"
                "declined" -> "Declined"
                "stopped" -> "Stopped"
                else -> "Ran"
            }
        return "$verb ${commandProgramName(command) ?: "command"}"
    }
    return entry.detail.takeIf { it.isNotBlank() }?.let(::truncateLabel) ?: entry.name
}

/**
 * The toggle's label, from `appendToolGroupRows`: a live run labels its latest
 * call in the present tense, a single non-edit call labels itself, and anything
 * else is the action-count summary.
 */
private fun workGroupSummary(group: List<FeedEntry>, workInFlight: Boolean): String {
    val latest = group.last()
    if (workInFlight && latest is FeedEntry.ToolCall && latest.state == ToolState.Running) {
        return liveToolCallSummary(latest)
    }
    if (group.size == 1) {
        val single = latest
        if (single is FeedEntry.ToolCall && single.groupAction() != ToolGroupAction.Edit) {
            return singleToolCallLabel(single)
        }
        if (single is FeedEntry.Reasoning) return reasoningLabel(single)
    }
    return summarizeToolGroup(group).takeIf { it.isNotEmpty() } ?: thoughtGroupLabel(group)
}

/**
 * The row label for a reasoning entry: a `task.progress` tick is the live
 * "Thinking" line, a provider trace is "Thought", and merged consecutive traces
 * count themselves — RN's `Thought (×N)`.
 */
fun reasoningLabel(entry: FeedEntry.Reasoning): String =
    when {
        !entry.thought -> "Thinking"
        entry.extraParts.isEmpty() -> "Thought"
        else -> "Thought (×${1 + entry.extraParts.size})"
    }

/** The count of messages a row stands for: a merged thought is still one row. */
private val FeedEntry.thoughtParts: Int
    get() = if (this is FeedEntry.Reasoning && thought) 1 + extraParts.size else 1

private fun thoughtGroupLabel(group: List<FeedEntry>): String {
    val count = group.sumOf { it.thoughtParts }
    return if (count == 1) "Thought" else "Thought (×$count)"
}

/* ── Row labels ────────────────────────────────────────────────────────── */

/**
 * Work-log labels render one line, but the whole string is still measured, so a
 * multi-kilobyte heredoc would stall layout. Expanded bodies and copy keep the
 * full text.
 */
private const val WORK_LOG_LABEL_MAX_LENGTH = 200

/** `truncate` from `packages/shared/src/String.ts`. */
private fun truncateLabel(value: String): String {
    val trimmed = value.trim()
    return if (trimmed.length <= WORK_LOG_LABEL_MAX_LENGTH) trimmed
    else trimmed.take(WORK_LOG_LABEL_MAX_LENGTH) + "..."
}

private fun collapseWhitespace(value: String): String =
    value.replace(Regex("\\s+"), " ").trim()

private fun stripShellWrapper(value: String): String {
    val trimmed = value.trim()
    val match =
        Regex("^/bin/zsh -lc ['\"]?([\\s\\S]*?)['\"]?$").matchEntire(trimmed)
    return (match?.groupValues?.get(1) ?: trimmed).trim()
}

private fun capitalizePhrase(value: String): String {
    val trimmed = value.trim()
    if (trimmed.isEmpty()) return value
    return trimmed.replaceFirstChar { it.uppercase() }
}

/** `workEntryPreview`: command, else detail, else the changed-file list. */
private fun toolCallPreview(entry: FeedEntry.ToolCall): String? {
    entry.command?.let { return it }
    if (entry.detail.isNotEmpty()) return entry.detail
    if (entry.changedFiles.isEmpty()) return null
    val first = entry.changedFiles.first()
    return if (entry.changedFiles.size == 1) first
    else "$first +${entry.changedFiles.size - 1} more"
}

private fun toolCallHeading(entry: FeedEntry.ToolCall): String {
    entry.t3Presentation()?.let { return it.displayName }
    return capitalizePhrase(normalizeCompactToolLabel(entry.toolTitle ?: entry.name))
}

/**
 * `workEntryRowLabel`: what the row's one line says. Collapsed it is the T3
 * tool's friendly name, else the command/detail/files preview squashed to one
 * line, else the title. Expanded rows heading a command say "Command" and leave
 * the command itself to the body.
 */
fun toolCallRowLabel(entry: FeedEntry.ToolCall, expanded: Boolean = false): String {
    entry.t3Presentation()?.let { return it.displayName }
    if (expanded && !entry.command.isNullOrBlank()) return "Command"
    val preview = toolCallPreview(entry)
    if (expanded) return preview?.takeIf { it.isNotBlank() }?.let(::truncateLabel) ?: toolCallHeading(entry)
    return preview?.let { truncateLabel(collapseWhitespace(stripShellWrapper(it))) }
        ?.takeIf { it.isNotEmpty() }
        ?: toolCallHeading(entry)
}

/** `singleToolCallLabel`: the toggle's label when the run is one non-edit call. */
private fun singleToolCallLabel(entry: FeedEntry.ToolCall): String =
    entry.t3Presentation()?.displayName
        ?: entry.command?.takeIf { it.isNotBlank() }?.let(::truncateLabel)
        ?: entry.name

/**
 * `workEntryCanExpand`: something must be behind the chevron — MCP payload,
 * files, a command, detail text, or a withheld payload the expansion fetches
 * (`turnItemNeedsDetailFetch` arriving as [FeedEntry.ToolCall.fetchesDetail]).
 */
fun toolCallCanExpand(entry: FeedEntry.ToolCall): Boolean =
    (entry.itemType == "mcp_tool_call" && entry.toolDataJson != null) ||
        entry.fetchesDetail ||
        entry.outputImageCount > 0 ||
        entry.changedFiles.any { it.isNotBlank() } ||
        !entry.command.isNullOrBlank() ||
        entry.detail.isNotBlank()

/**
 * `buildWorkEntryExpandedBody`: MCP payload, then the command, then detail,
 * then the file list — each block deduped against the row's visible label the
 * way RN's `appendBlock` does. [fetchedDetail] is the full item text from a
 * `getTurnItem` read, appended last so withheld output lands after the call.
 */
fun toolCallExpandedBody(entry: FeedEntry.ToolCall, fetchedDetail: String? = null): String? {
    val blocks = mutableListOf<String>()
    val visibleLabel = toolCallRowLabel(entry, expanded = true).trim()
    fun appendBlock(value: String?) {
        val trimmed = value?.trim() ?: return
        if (trimmed.isEmpty()) return
        if (entry.command == null && (trimmed == visibleLabel || trimmed in blocks)) return
        blocks += trimmed
    }
    if (entry.itemType == "mcp_tool_call" && entry.toolDataJson != null) {
        appendBlock("MCP call\n${entry.toolDataJson}")
    }
    appendBlock(entry.command)
    appendBlock(entry.detail)
    appendBlock(fetchedDetail)
    if (entry.changedFiles.isNotEmpty()) appendBlock(entry.changedFiles.joinToString("\n"))
    return blocks.takeIf { it.isNotEmpty() }?.joinToString("\n\n")
}
