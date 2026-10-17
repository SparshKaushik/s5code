package club.touchtech.s5code.kotlin.feature.home

import java.time.ZoneId
import java.time.ZonedDateTime
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SnoozePresetsTest {

    @Test
    fun `morning run offers evening tomorrow and next week`() {
        val morning = ZonedDateTime.of(2026, 4, 8, 10, 0, 0, 0, ZoneId.of("UTC")) // Wednesday
        val presets = resolveSnoozePresets(morning)
        assertEquals(listOf("hour", "evening", "tomorrow", "next-week"), presets.map { it.id })
        assertEquals("In 1 hour", presets[0].label)
        assertEquals("This evening", presets[1].label)
        assertEquals("Tomorrow", presets[2].label)
        assertEquals("Next week", presets[3].label)
        assertTrue(presets.all { it.snoozedUntilIso.isNotBlank() })
    }

    @Test
    fun `late afternoon skips this evening`() {
        val lateAfternoon = ZonedDateTime.of(2026, 4, 8, 17, 30, 0, 0, ZoneId.of("UTC")) // 5:30 PM
        val presets = resolveSnoozePresets(lateAfternoon)
        assertEquals(listOf("hour", "tomorrow", "next-week"), presets.map { it.id })
    }

    @Test
    fun `next week on Monday lands on the following Monday`() {
        val monday = ZonedDateTime.of(2026, 4, 6, 10, 0, 0, 0, ZoneId.of("UTC")) // Monday
        val presets = resolveSnoozePresets(monday)
        val nextWeek = presets.single { it.id == "next-week" }
        assertNotNull(nextWeek)
        assertTrue(nextWeek.snoozedUntilIso.startsWith("2026-04-13"))
    }
}
