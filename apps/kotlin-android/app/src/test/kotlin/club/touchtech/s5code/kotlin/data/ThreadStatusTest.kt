package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ThreadStatus
import club.touchtech.s5code.kotlin.transport.wire.LatestTurnDto
import club.touchtech.s5code.kotlin.transport.wire.SessionDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadShellDto
import java.time.Instant
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Thread status resolution.
 *
 * The bug these guard against: a thread whose last turn errored months ago was
 * badged Failed and sorted above live work forever, because the failure check ran
 * before the lifecycle checks and read the historical turn state. The RN list
 * partitions on the lifecycle first and only then asks for a badge.
 */
class ThreadStatusTest {

    private val now = Instant.parse("2026-03-01T12:00:00Z").toEpochMilli()

    private fun at(offsetMillis: Long): String = Instant.ofEpochMilli(now + offsetMillis).toString()

    private fun shell(
        session: SessionDto? = null,
        latestTurn: LatestTurnDto? = null,
        settledOverride: String? = null,
        settledAt: String? = null,
        snoozedUntil: String? = null,
        snoozedAt: String? = null,
        pinnedAt: String? = null,
        latestUserMessageAt: String? = null,
        hasPendingApprovals: Boolean = false,
        hasPendingUserInput: Boolean = false,
    ) =
        ThreadShellDto(
            id = "t-1",
            session = session,
            latestTurn = latestTurn,
            settledOverride = settledOverride,
            settledAt = settledAt,
            snoozedUntil = snoozedUntil,
            snoozedAt = snoozedAt,
            pinnedAt = pinnedAt,
            latestUserMessageAt = latestUserMessageAt,
            hasPendingApprovals = hasPendingApprovals,
            hasPendingUserInput = hasPendingUserInput,
        )

    private val hoursAgo = -2 * 60 * 60 * 1000L
    private val weekAgo = -7 * 24 * 60 * 60 * 1000L

    @Test
    fun `a settled thread that once failed stays settled`() {
        val status =
            threadStatusOf(
                shell(
                    session = SessionDto(status = "error", updatedAt = at(weekAgo)),
                    latestTurn = LatestTurnDto(turnId = "turn-1", state = "error", completedAt = at(weekAgo)),
                    settledOverride = "settled",
                    settledAt = at(hoursAgo),
                ),
                now,
            )
        assertEquals(ThreadStatus.Settled, status)
    }

    @Test
    fun `a quiet thread auto-settles rather than sitting at the top as failed`() {
        val status =
            threadStatusOf(
                shell(
                    session = SessionDto(status = "error", updatedAt = at(weekAgo)),
                    latestTurn = LatestTurnDto(turnId = "turn-1", state = "error", completedAt = at(weekAgo)),
                    latestUserMessageAt = at(weekAgo),
                ),
                now,
            )
        assertEquals(ThreadStatus.Settled, status)
    }

    @Test
    fun `a fresh failure is still surfaced`() {
        val status =
            threadStatusOf(
                shell(
                    session = SessionDto(status = "error", updatedAt = at(hoursAgo)),
                    latestTurn = LatestTurnDto(turnId = "turn-1", state = "error", completedAt = at(hoursAgo)),
                    latestUserMessageAt = at(hoursAgo),
                ),
                now,
            )
        assertEquals(ThreadStatus.Failed, status)
    }

    @Test
    fun `a finished turn that errored does not badge an idle thread`() {
        // The session recovered; only the historical turn carries the error. RN reads
        // the session, not the turn, so this is Idle rather than Failed.
        val status =
            threadStatusOf(
                shell(
                    session = SessionDto(status = "idle", updatedAt = at(-60_000)),
                    latestTurn = LatestTurnDto(turnId = "turn-1", state = "error", completedAt = at(-60_000)),
                    latestUserMessageAt = at(-60_000),
                ),
                now,
            )
        assertEquals(ThreadStatus.Idle, status)
    }

    @Test
    fun `blocked work outranks every lifecycle state`() {
        assertEquals(
            ThreadStatus.AwaitingApproval,
            threadStatusOf(
                shell(
                    settledOverride = "settled",
                    snoozedUntil = at(60 * 60 * 1000L),
                    hasPendingApprovals = true,
                ),
                now,
            ),
        )
        assertEquals(
            ThreadStatus.AwaitingInput,
            threadStatusOf(shell(settledOverride = "settled", hasPendingUserInput = true), now),
        )
    }

    @Test
    fun `a running session outranks a stale settle`() {
        assertEquals(
            ThreadStatus.Working,
            threadStatusOf(
                shell(session = SessionDto(status = "running"), settledOverride = "settled"),
                now,
            ),
        )
    }

    @Test
    fun `a snoozed thread wakes on a failure newer than the snooze`() {
        val base =
            shell(
                session = SessionDto(status = "error", updatedAt = at(-60_000)),
                snoozedUntil = at(60 * 60 * 1000L),
                snoozedAt = at(hoursAgo),
                latestUserMessageAt = at(hoursAgo),
            )
        assertEquals(ThreadStatus.Failed, threadStatusOf(base, now))
    }

    @Test
    fun `a thread snoozed after it failed stays snoozed`() {
        val base =
            shell(
                session = SessionDto(status = "error", updatedAt = at(weekAgo)),
                snoozedUntil = at(60 * 60 * 1000L),
                snoozedAt = at(hoursAgo),
                latestUserMessageAt = at(weekAgo),
            )
        assertEquals(ThreadStatus.Snoozed, threadStatusOf(base, now))
    }

    @Test
    fun `a snoozed thread wakes when a run finishes after the snooze`() {
        val base =
            shell(
                latestTurn =
                    LatestTurnDto(turnId = "turn-1", state = "completed", completedAt = at(-60_000)),
                snoozedUntil = at(60 * 60 * 1000L),
                snoozedAt = at(hoursAgo),
                latestUserMessageAt = at(hoursAgo),
            )
        assertEquals(ThreadStatus.Idle, threadStatusOf(base, now))
    }

    @Test
    fun `an expired snooze stops hiding the thread`() {
        val base =
            shell(snoozedUntil = at(hoursAgo), snoozedAt = at(weekAgo), latestUserMessageAt = at(hoursAgo))
        assertEquals(ThreadStatus.Idle, threadStatusOf(base, now))
    }

    @Test
    fun `a pinned thread never auto-settles out of sight`() {
        val status =
            threadStatusOf(
                shell(pinnedAt = at(weekAgo), latestUserMessageAt = at(weekAgo)),
                now,
            )
        assertEquals(ThreadStatus.Idle, status)
    }

    @Test
    fun `an explicit settle still wins on a pinned thread`() {
        // The user asked for both; the explicit settle is the newer intent and the
        // RN partition checks the override before the auto-settle window.
        val status =
            threadStatusOf(
                shell(
                    pinnedAt = at(weekAgo),
                    settledOverride = "settled",
                    settledAt = at(hoursAgo),
                    latestUserMessageAt = at(weekAgo),
                ),
                now,
            )
        assertEquals(ThreadStatus.Idle, status)
    }

    @Test
    fun `keep-active suppresses the inactivity settle`() {
        val status =
            threadStatusOf(
                shell(settledOverride = "active", latestUserMessageAt = at(weekAgo)),
                now,
            )
        assertEquals(ThreadStatus.Idle, status)
    }

    @Test
    fun `a just-sent message reads as queued`() {
        val status = threadStatusOf(shell(latestUserMessageAt = at(-5_000)), now)
        assertEquals(ThreadStatus.Queued, status)
    }
}
