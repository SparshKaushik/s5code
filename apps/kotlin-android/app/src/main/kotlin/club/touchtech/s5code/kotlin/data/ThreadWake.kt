package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.ThreadShellDto

/**
 * The failed run's edge, following `failureEdgeAt`: the session timestamp also
 * moves on metadata writes, so the failed run's own completion wins when there
 * is one. Null when the thread is not failed.
 */
private fun failureEdgeAt(shell: ThreadShellDto): Long? {
    val session = shell.session ?: return null
    if (session.status !in setOf("error", "limited")) return null
    val turn = shell.latestTurn
    return turn?.completedAt?.takeIf { turn.state == "error" }?.let(::parseInstant)
        ?: parseInstant(session.updatedAt)
}

/**
 * When a snoozed thread woke, following `threadWokeAt` in
 * `packages/client-runtime/src/state/threadSettled.ts`. A thread that raised its
 * hand while snoozed woke at that activity, and stays woken at it even after the
 * scheduled time passes: reporting the timer then would resurface a Woke marker
 * the user already cleared by visiting. Otherwise it woke at the timer, once the
 * timer is behind [nowMillis].
 */
internal fun threadWokeAtMillis(shell: ThreadShellDto, nowMillis: Long, subagentRequest: String? = null): Long? {
    val wakeAt = shell.snoozedUntil?.let(::parseInstant) ?: return null
    val snoozedAt = shell.snoozedAt?.let(::parseInstant)
    val failedAt = failureEdgeAt(shell)
    val freshFailure = shell.session?.status in setOf("error", "limited") &&
        (snoozedAt == null || (failedAt != null && failedAt > snoozedAt))
    val raisedHand = shell.hasPendingApprovals || shell.hasPendingUserInput ||
        shell.pendingRuntimeRequest != null || subagentRequest != null || freshFailure
    if (raisedHand) {
        // An old failure must not replace the activity time of an approval or input wake.
        if (failedAt != null && (snoozedAt == null || failedAt > snoozedAt)) return failedAt
        return parseInstant(shell.session?.updatedAt) ?: snoozedAt
    }
    return wakeAt.takeIf { it <= nowMillis }
}

/**
 * The row's Woke marker, following `resolveThreadListV2WokeAt`: a wake the user
 * has not acknowledged by visiting at or after it. [visitTracking] is the
 * `threadVisitedTracking` capability — without it the server cannot persist the
 * acknowledgement, so the marker would never clear. A settled thread is done
 * with, so its wake is not news.
 */
internal fun unacknowledgedWokeAtMillis(
    shell: ThreadShellDto,
    nowMillis: Long,
    visitTracking: Boolean,
    subagentRequest: String? = null,
): Long? {
    if (!visitTracking || shell.settledOverride == "settled") return null
    val wokeAt = threadWokeAtMillis(shell, nowMillis, subagentRequest) ?: return null
    val lastVisitedAt = parseInstant(shell.lastVisitedAt)
    return wokeAt.takeIf { lastVisitedAt == null || lastVisitedAt < wokeAt }
}

/** The earliest `snoozedUntil` strictly after [nowMillis], following `nextSnoozeWakeAfter`. */
internal fun nextSnoozeWakeAfter(snoozedUntil: Sequence<String?>, nowMillis: Long): Long? =
    snoozedUntil.mapNotNull { parseInstant(it) }.filter { it > nowMillis }.minOrNull()

/**
 * The home clock ticks each minute and also the moment a snooze expires, so a
 * woken thread re-partitions on time instead of up to a minute late.
 */
internal fun clockTickDelayMillis(nowMillis: Long, nextWakeMillis: Long?): Long =
    nextWakeMillis?.let { (it - nowMillis).coerceIn(0, 60_000) } ?: 60_000
