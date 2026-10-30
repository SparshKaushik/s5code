package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.LimitAccount
import club.touchtech.s5code.kotlin.model.ProviderUsageLimits
import club.touchtech.s5code.kotlin.model.UsageLimitWindow
import java.time.Instant
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Ports of the shared `usageLimits` tests for weekly-blocked sessions and even-pace marks. */
class UsageLimitsPoolsTest {
    private fun at(time: String) = Instant.parse("2026-09-03T$time:00.000Z").toEpochMilli()

    private val now = at("12:00")
    private val session = UsageLimitWindow("five_hour", "session", "Session", 40.0, at("14:00"), 300)
    private val weekly = UsageLimitWindow("seven_day", "weekly", "Weekly", 40.0, at("14:00"), 7 * 24 * 60)

    private fun account(key: String, windows: List<UsageLimitWindow>, driver: String = "claudeAgent", checkedAt: Long = at("11:00")) =
        LimitAccount(
            key = key,
            driver = driver,
            displayName = key,
            email = null,
            plan = null,
            accentColor = null,
            environments = emptyList(),
            sourceLabel = "Hub",
            redeem = null,
            limits = ProviderUsageLimits(checkedAt, windows, null, null, null),
        )

    @Test
    fun `a weekly-blocked session becomes usable once both clocks allow it`() {
        // driver, session reset, session spent, usable at, restored
        for ((driver, sessionReset, used, usableAt, restores) in
            listOf(
                listOf("claudeAgent", "13:00", 40, "14:00", 100),
                listOf("codex", "14:00", 100, "14:00", 100),
                listOf("claudeAgent", "15:00", 100, "15:00", 100),
                listOf("codex", "15:00", 40, "14:00", 60),
            )) {
            driver as String
            val codex = driver == "codex"
            val a =
                account(
                    "a",
                    listOf(
                        session.copy(
                            id = if (codex) "primary" else session.id,
                            usedPercent = (used as Int).toDouble(),
                            resetsAtMillis = at(sessionReset as String),
                        ),
                        weekly.copy(id = if (codex) "secondary" else weekly.id, usedPercent = 100.0, resetsAtMillis = at("14:00")),
                    ),
                    driver = driver,
                )
            val row = collectLimitPools(listOf(a), now)[0].windows[0]
            assertEquals("$driver $sessionReset", 0, row.remainingPercent)
            assertEquals("$driver $sessionReset", at(usableAt as String), row.resets[0].atMillis)
            assertEquals("$driver $sessionReset", restores, row.resets[0].restoresPercent)
        }
    }

    @Test
    fun `keeps blocked sessions empty without predicting a refill from a missing or stale clock`() {
        for (resetsAt in listOf(null, at("11:30"))) {
            val pool =
                collectLimitPools(
                    listOf(
                        account("session-clock", listOf(session.copy(resetsAtMillis = resetsAt), weekly.copy(usedPercent = 100.0))),
                        account("weekly-clock", listOf(session, weekly.copy(usedPercent = 100.0, resetsAtMillis = resetsAt))),
                    ),
                    now,
                )[0]
            val row = pool.windows[0]
            assertEquals(0, row.remainingPercent)
            assertNull(row.pace)
            assertEquals(emptyList<Any>(), row.resets)
            assertEquals(listOf(null, null), row.columns.map { it.second?.resetsAtMillis })
        }
    }

    @Test
    fun `orders by usable session refill and clears suppression after a fresh weekly reading`() {
        val blocked =
            account("blocked", listOf(session.copy(resetsAtMillis = at("13:00")), weekly.copy(usedPercent = 100.0, resetsAtMillis = at("15:00"))))
        val available = account("available", listOf(session))
        val before = collectLimitPools(listOf(blocked, available), now)[0]
        assertEquals(listOf("available", "blocked"), before.accounts.map { it.key })
        assertEquals(30, before.windows[0].remainingPercent)

        val refreshed =
            blocked.copy(
                limits = blocked.limits.copy(checkedAtMillis = at("12:00"), windows = listOf(blocked.limits.windows[0], weekly.copy(usedPercent = 10.0))),
            )
        val after = collectLimitPools(listOf(refreshed, available), now)[0]
        assertEquals(listOf("blocked", "available"), after.accounts.map { it.key })
        assertEquals(60, after.windows[0].remainingPercent)
        assertEquals(at("13:00"), after.windows[0].resets[0].atMillis)
        assertEquals(20, after.windows[0].resets[0].restoresPercent)
    }

    @Test
    fun `even pace tracks the time left in the window`() {
        assertEquals(40, paceRemainingPercent(session, now))
        assertNull(paceRemainingPercent(session.copy(resetsAtMillis = null), now))
        assertNull(paceRemainingPercent(session.copy(windowDurationMins = null), now))
        assertEquals(100, paceRemainingPercent(session, at("08:00")))
        assertEquals(0, paceRemainingPercent(session, at("15:00")))
    }
}
