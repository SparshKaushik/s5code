package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.model.FeedEntry
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

    data class Entry(val entry: FeedEntry) : FeedRow {
        override val key: String
            get() = entry.id
    }

    /**
     * Discloses the tool calls folded away above it.
     *
     * [onlyTools] picks the noun, following `ThreadWorkGroupToggle`: a run of pure
     * tool calls says "tool calls", a mixed run says "log entries", because
     * "3 previous tool calls" is a lie when one of them is a subagent.
     */
    data class WorkToggle(
        val groupId: String,
        val hiddenCount: Int,
        val expanded: Boolean,
        val onlyTools: Boolean,
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
    ) : FeedRow {
        enum class Placement { Header, Footer }

        override val key: String
            get() = "turn-fold:$turnId:${placement.name.lowercase()}"

        /**
         * What the row reads.
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
}

/**
 * How many rows of a run of work stay visible when it is folded.
 *
 * One, matching `MAX_VISIBLE_WORK_LOG_ENTRIES` in
 * `apps/mobile/src/lib/threadActivity.ts`. A phone transcript is mostly tool
 * calls, and the last one is the only one that is still news.
 */
const val MAX_VISIBLE_WORK_ROWS = 1

/**
 * Folds runs of adjacent work rows, mirroring `appendPresentedFeedEntry` in the RN
 * client and `MessagesTimeline.logic.ts` on the desktop/web side.
 *
 * Only tool calls fold. Subagent rows stay visible however long the run is: a
 * running fleet hidden behind "+7 previous tool calls" is exactly the stall the
 * subagent row exists to explain. Errors stay for the same reason — an error
 * folded away is an error the user does not know about.
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

        val foldable = group.filter { it is FeedEntry.ToolCall }
        if (foldable.size <= MAX_VISIBLE_WORK_ROWS) {
            group.forEach { rows += FeedRow.Entry(it) }
            appendFooterFor(group.map { it.id })
            continue
        }

        val groupId = "work-group:${group.first().id}"
        val expanded = groupId in expandedGroups
        // Fold from the top: the newest calls are the ones worth keeping. Filtering
        // the original group (rather than concatenating two filtered lists) is what
        // keeps a subagent that ran mid-run in its own chronological place.
        val foldedAway = foldable.dropLast(MAX_VISIBLE_WORK_ROWS).map { it.id }.toSet()
        val visible = if (expanded) group else group.filter { it.id !in foldedAway }
        visible.forEach { rows += FeedRow.Entry(it) }
        rows +=
            FeedRow.WorkToggle(
                groupId = groupId,
                hiddenCount = foldedAway.size,
                expanded = expanded,
                onlyTools = group.all { it is FeedEntry.ToolCall },
            )
        // After the work toggle, not before: the toggle belongs to the rows above
        // it, and the turn trigger closes the whole turn.
        appendFooterFor(group.map { it.id })
    }
    if (activeWorkStartedAtMillis != null) rows += FeedRow.Working(activeWorkStartedAtMillis)
    return rows
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

    // Grouped in feed order, so "first row" and "last answer" are positional rather
    // than derived from timestamps, which tie.
    val groups = LinkedHashMap<String, MutableList<FeedEntry>>()
    var boundary: Long? = null
    val boundaries = mutableMapOf<String, Long?>()
    feed.forEach { entry ->
        if (entry is FeedEntry.UserMessage) {
            // The prompt is where the user's clock starts, which is what "worked
            // for" should measure — not when the provider got round to the turn.
            boundary = entry.atMillis
            return@forEach
        }
        val turnId = entry.turnId ?: return@forEach
        if (turnId !in groups) {
            groups[turnId] = mutableListOf()
            boundaries[turnId] = boundary
            boundary = null
        }
        groups.getValue(turnId) += entry
    }

    val folds = mutableMapOf<String, TurnFold>()
    groups.forEach { (turnId, entries) ->
        if (turnId == openTurnId) return@forEach
        if (entries.any { it is FeedEntry.AgentMessage && it.streaming }) return@forEach
        val answerId = entries.lastOrNull { it is FeedEntry.AgentMessage }?.id
        val hidden = entries.filter { it.id != answerId }.map { it.id }.toSet()
        if (hidden.isEmpty()) return@forEach
        val anchor = entries.first()
        val answer = entries.lastOrNull { it.id == answerId }
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
        val startedAt = turnClock?.first ?: boundaries[turnId] ?: anchor.atMillis
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
    if (millis < 10_000) return String.format(java.util.Locale.US, "%.1fs", millis / 1_000.0)
    if (millis < 60_000) return "${Math.round(millis / 1_000.0)}s"
    val minutes = millis / 60_000
    val seconds = Math.round((millis % 60_000) / 1_000.0)
    return when (seconds) {
        0L -> "${minutes}m"
        60L -> "${minutes + 1}m"
        else -> "${minutes}m ${seconds}s"
    }
}

/**
 * Whether an entry is part of a run of work rather than conversation.
 *
 * Messages, plans, and errors break a run: they are the things you scroll to read,
 * and a plan card swallowed into a fold would be unreachable.
 */
private fun isWorkRow(entry: FeedEntry): Boolean =
    entry is FeedEntry.ToolCall || entry is FeedEntry.Subagent

/** Toggle label, matching `ThreadWorkGroupToggle`'s wording. */
fun workToggleLabel(row: FeedRow.WorkToggle): String {
    val noun =
        when {
            row.onlyTools && row.hiddenCount == 1 -> "tool call"
            row.onlyTools -> "tool calls"
            row.hiddenCount == 1 -> "log entry"
            else -> "log entries"
        }
    return if (row.expanded) {
        if (row.onlyTools) "Show fewer tool calls" else "Show fewer log entries"
    } else {
        "+${row.hiddenCount} previous $noun"
    }
}
