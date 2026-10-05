package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.model.PendingBackgroundTask
import club.touchtech.s5code.kotlin.model.TurnInfo

/**
 * Whether the thread's most recent turn is finished, ported from
 * `isLatestTurnSettled` in `packages/shared/src/orchestrationTiming.ts`.
 *
 * Both the turn record and the session get a say, and both are needed. A turn with
 * no `completedAt` is obviously open; a turn that has completed while the session is
 * still `running` is the provider having sent its completion before the orchestrator
 * finished the turn's follow-up work, and treating that as settled is what makes a
 * transcript go quiet while the agent is demonstrably still busy.
 *
 * A turn that never started is not settled either: that is the window between the
 * user pressing send and the provider accepting the turn.
 */
internal fun isLatestTurnSettled(latestTurn: TurnInfo?, sessionStatus: String?): Boolean {
    if (latestTurn?.startedAtMillis == null) return false
    if (latestTurn.completedAtMillis == null) return false
    if (sessionStatus == null) return true
    return sessionStatus != "running"
}

/**
 * When the work currently in flight began, or null if nothing is running. Ported
 * from `deriveActiveWorkStartedAt` in `packages/shared/src/orchestrationTiming.ts`.
 *
 * This is what drives the "Working for 12s" row, and the row's whole reason to exist
 * is the case where the agent has produced nothing yet: no message, no tool call,
 * just a provider thinking. Without it the transcript is indistinguishable from an
 * idle thread, which is the state users read as "it died".
 *
 * [sessionStartedAtMillis] is the deviation from RN, which passes its own
 * send-request timestamp here instead. The Kotlin client does not hold one — a send
 * is fire-and-forget into the gateway — so the session's own clock stands in. That
 * covers the `starting` window, where a session exists and no turn does yet, and it
 * is the honest reading either way: the session began working then.
 */
internal fun activeWorkStartedAtMillis(
    latestTurn: TurnInfo?,
    sessionStatus: String?,
    sessionStartedAtMillis: Long?,
): Long? {
    if (isLatestTurnSettled(latestTurn, sessionStatus)) return null
    if (sessionStatus != null && sessionStatus !in ACTIVE_SESSION_STATUSES) return null
    return latestTurn?.startedAtMillis ?: sessionStartedAtMillis
}

/**
 * Session statuses that mean work is in flight.
 *
 * `ready` is deliberately absent: a ready session with an unsettled turn record is a
 * stale record, not live work, and showing a timer that never stops is worse than
 * showing nothing.
 */
private val ACTIVE_SESSION_STATUSES = setOf("starting", "running")

/** "Working for 12s", with the same duration formatting as every other client. */
internal fun workingLabel(startedAtMillis: Long, nowMillis: Long): String {
    val elapsed = (nowMillis - startedAtMillis).coerceAtLeast(0L)
    return "Working for ${formatDuration(elapsed)}"
}

/**
 * The floating pill's own duration: "Working 1m 04s" like the RN
 * `WorkingTimer`, rather than the feed row's "Working for 1m 04s".
 */
internal fun floatingWorkingLabel(startedAtMillis: Long, nowMillis: Long): String {
    val totalSeconds = ((nowMillis - startedAtMillis).coerceAtLeast(0L) / 1_000L)
    val duration =
        when {
            totalSeconds < 60 -> "${totalSeconds}s"
            totalSeconds >= 3_600 -> formatDuration(totalSeconds * 1_000L)
            else -> {
                val minutes = totalSeconds / 60
                val seconds = (totalSeconds % 60).toString().padStart(2, '0')
                "${minutes}m ${seconds}s"
            }
        }
    return "Working $duration"
}

/* ── Background work left running after the turn settled ─────────────── */

private data class BackgroundKind(val order: Int, val singular: String, val plural: String)

// `order` groups work the way a reader thinks about it: agents first, loose
// tasks last — `BACKGROUND_WORK_KINDS` in `state/threadExecution.ts`.
private fun backgroundKind(kind: String): BackgroundKind =
    when (kind) {
        "subagent" -> BackgroundKind(0, "subagent", "subagents")
        "command" -> BackgroundKind(1, "command", "commands")
        "monitor" -> BackgroundKind(2, "monitor", "monitors")
        else -> BackgroundKind(3, "background task", "background tasks")
    }

/**
 * The floating "Waiting on …" pill label, ported from
 * `presentPendingBackgroundWork` in `packages/client-runtime/src/state/
 * threadExecution.ts`. A command does not hold completion, so "Running" is for
 * loose work and "Waiting on" for work the turn is blocked behind.
 */
internal fun pendingBackgroundWorkLabel(tasks: List<PendingBackgroundTask>): String? {
    if (tasks.isEmpty()) return null
    val waiting = tasks.any { it.kind != "command" }
    val items =
        tasks
            .map { task ->
                val kind = backgroundKind(task.kind)
                val label = task.description?.trim().orEmpty()
                kind to label.ifEmpty { kind.singular }
            }
            .sortedBy { it.first.order }
    if (items.size == 1) {
        val (kind, label) = items.single()
        val named = label != kind.singular
        return if (waiting) {
            if (named) "Waiting on ${kind.singular} $label" else "Waiting on a ${kind.singular}"
        } else {
            if (named) "Running: $label" else "Running a ${kind.singular}"
        }
    }
    val counts = items.groupingBy { it.first }.eachCount()
    val groups =
        counts.entries.sortedBy { it.key.order }.map { (kind, count) ->
            "$count ${if (count == 1) kind.singular else kind.plural}"
        }
    val joined =
        when (groups.size) {
            1 -> groups.first()
            2 -> "${groups[0]} and ${groups[1]}"
            else -> groups.dropLast(1).joinToString(", ") + ", and ${groups.last()}"
        }
    return "${if (waiting) "Waiting on" else "Running"} $joined"
}
