package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.transport.wire.LatestTurnDto
import club.touchtech.s5code.kotlin.transport.wire.SessionDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadShellDto
import java.time.Instant
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The thread row's Woke marker, ported from the RN `resolveThreadListV2WokeAt` cases. */
class ThreadWakeTest {

    private val now = Instant.parse("2026-04-10T12:00:00Z").toEpochMilli()
    private val minute = 60_000L

    private fun at(offsetMillis: Long): String = Instant.ofEpochMilli(now + offsetMillis).toString()

    private val wokeAt = now - minute
    private val woken =
        ThreadShellDto(
            id = "woke",
            latestUserMessageAt = at(-90 * minute),
            snoozedAt = at(-60 * minute),
            snoozedUntil = at(-minute),
        )

    private fun marker(shell: ThreadShellDto, nowMillis: Long = now, visitTracking: Boolean = true) =
        unacknowledgedWokeAtMillis(shell, nowMillis, visitTracking)

    @Test
    fun `a timer wake survives a plain visit until acknowledgement reaches its timestamp`() {
        assertEquals(wokeAt, marker(woken))
        assertEquals(wokeAt, marker(woken.copy(lastVisitedAt = at(-30 * minute))))
        assertNull(marker(woken.copy(lastVisitedAt = at(-minute))))
        assertEquals(wokeAt, marker(woken.copy(lastVisitedAt = "invalid")))
    }

    @Test
    fun `no marker while snoozed, after settling, without visit tracking, or without a snooze`() {
        assertNull(marker(woken, nowMillis = now - 2 * minute))
        assertNull(marker(woken.copy(settledOverride = "settled")))
        assertNull(marker(woken, visitTracking = false))
        assertNull(marker(woken.copy(snoozedUntil = null)))
    }

    @Test
    fun `an early failure wake stays acknowledged after the scheduled time passes`() {
        val completedAt = at(-2 * minute)
        val failed =
            woken.copy(
                snoozedUntil = at(minute),
                session = SessionDto(status = "error", updatedAt = completedAt),
                latestTurn =
                    LatestTurnDto(
                        turnId = "run",
                        state = "error",
                        requestedAt = at(-3 * minute),
                        startedAt = at(-3 * minute),
                        completedAt = completedAt,
                    ),
            )
        assertEquals(now - 2 * minute, marker(failed))
        assertNull(marker(failed.copy(lastVisitedAt = completedAt), nowMillis = now + 2 * minute))
    }

    @Test
    fun `an approval wake reports the activity, not an older failure`() {
        val shell =
            woken.copy(
                snoozedUntil = at(60 * minute),
                hasPendingApprovals = true,
                session = SessionDto(status = "ready", updatedAt = at(-5 * minute)),
            )
        assertEquals(now - 5 * minute, threadWokeAtMillis(shell, now))
    }

    @Test
    fun `the summary marks only active rows`() {
        fun summary(shell: ThreadShellDto) =
            threadSummaryFrom(
                EnvironmentId("env"),
                shell,
                { ProviderInstance(it, "codex") },
                now,
                visitTracking = true,
            )
        assertEquals(wokeAt, summary(woken).wokeAtMillis)
        assertNull(summary(woken.copy(snoozedUntil = at(minute))).wokeAtMillis)
    }
}
