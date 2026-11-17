package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.UsageWindow
import club.touchtech.s5code.kotlin.transport.wire.UsageBucketDto
import club.touchtech.s5code.kotlin.transport.wire.UsageTokenTotalsDto
import java.time.Instant
import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The usage chart is built from a sparse bucket list, so the interesting cases are
 * the slots the server never sent: they have to appear as zeroes rather than
 * closing the gap and misreporting the shape of the window.
 */
class UsageSeriesTest {
    private val zone = ZoneId.of("UTC")
    private val now = Instant.parse("2026-03-10T14:37:00Z")

    private fun bucket(day: String, hourStart: String? = null, cost: Double = 1.0, tokens: Long = 10) =
        UsageBucketDto(
            day = day,
            hourStart = hourStart,
            provider = "codex",
            model = "gpt-5",
            totals = UsageTokenTotalsDto(uncachedInputTokens = tokens),
            costUsd = cost,
            records = 1,
        )

    @Test
    fun `daily window emits one slot per day inclusive of both bounds`() {
        val series = usageSeries(emptyList(), UsageWindow.Week, zone, now)
        assertEquals(7, series.size)
        assertEquals("03-04", series.first().label)
        assertEquals("03-10", series.last().label)
    }

    @Test
    fun `longer windows keep their full length`() {
        assertEquals(30, usageSeries(emptyList(), UsageWindow.Month, zone, now).size)
        assertEquals(90, usageSeries(emptyList(), UsageWindow.Quarter, zone, now).size)
    }

    @Test
    fun `days with no buckets are zero filled rather than dropped`() {
        val series = usageSeries(listOf(bucket("2026-03-08", cost = 3.0)), UsageWindow.Week, zone, now)
        assertEquals(7, series.size)
        val eighth = series.single { it.label == "03-08" }
        assertEquals(3.0, eighth.costUsd, 0.0001)
        assertTrue(series.filter { it.label != "03-08" }.all { it.costUsd == 0.0 && it.tokens == 0L })
    }

    @Test
    fun `buckets on the same day are summed across providers and models`() {
        val series =
            usageSeries(
                listOf(
                    bucket("2026-03-10", cost = 1.5, tokens = 100),
                    bucket("2026-03-10", cost = 2.5, tokens = 200),
                ),
                UsageWindow.Week,
                zone,
                now,
            )
        val today = series.last()
        assertEquals("03-10", today.label)
        assertEquals(4.0, today.costUsd, 0.0001)
        assertEquals(300L, today.tokens)
    }

    @Test
    fun `buckets outside the window are ignored`() {
        val series = usageSeries(listOf(bucket("2025-01-01", cost = 99.0)), UsageWindow.Week, zone, now)
        assertTrue(series.all { it.costUsd == 0.0 })
    }

    @Test
    fun `rolling day emits twenty four hourly slots labelled by hour`() {
        val series = usageSeries(emptyList(), UsageWindow.Day, zone, now)
        assertEquals(24, series.size)
        // 14:37 truncates to the 14:00 bucket, so the window runs 15:00 yesterday
        // through 14:00 today.
        assertEquals("15", series.first().label)
        assertEquals("14", series.last().label)
    }

    @Test
    fun `hourly buckets are matched on hourStart not day`() {
        val series =
            usageSeries(
                listOf(
                    bucket("2026-03-10", hourStart = "2026-03-10T12:00:00Z", cost = 2.0),
                    bucket("2026-03-10", hourStart = "2026-03-10T14:00:00Z", cost = 5.0),
                ),
                UsageWindow.Day,
                zone,
                now,
            )
        assertEquals(2.0, series.single { it.label == "12" }.costUsd, 0.0001)
        assertEquals(5.0, series.last().costUsd, 0.0001)
        assertEquals(7.0, series.sumOf { it.costUsd }, 0.0001)
    }

    @Test
    fun `hourly buckets with a minute offset land in their containing hour`() {
        val series =
            usageSeries(
                listOf(bucket("2026-03-10", hourStart = "2026-03-10T13:37:00Z", cost = 4.0)),
                UsageWindow.Day,
                zone,
                now,
            )
        assertEquals(4.0, series.single { it.label == "13" }.costUsd, 0.0001)
    }

    @Test
    fun `hour labels render in the viewer's zone`() {
        val series = usageSeries(emptyList(), UsageWindow.Day, ZoneId.of("Asia/Kolkata"), now)
        // 14:00Z is 19:30 IST, and the label is the wall-clock hour there.
        assertEquals("19", series.last().label)
    }

    @Test
    fun `a daily window near a DST change keeps its day count`() {
        // US DST began 2026-03-08. Calendar arithmetic on the local end day must
        // still produce exactly seven slots.
        val series = usageSeries(emptyList(), UsageWindow.Week, ZoneId.of("America/New_York"), now)
        assertEquals(7, series.size)
        assertEquals("03-04", series.first().label)
        assertEquals("03-10", series.last().label)
    }
}
