package club.touchtech.s5code.kotlin.feature.thread

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
