package club.touchtech.s5code.kotlin.feature.home

import java.time.ZoneId
import java.time.ZonedDateTime
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class SnoozePresetsTest {
    private val zone = ZoneId.of("UTC")

    private fun at(day: Int, hour: Int, minute: Int = 0, second: Int = 0) =
        ZonedDateTime.of(2026, 4, day, hour, minute, second, 0, zone)

    private fun menu(now: ZonedDateTime, presets: List<SnoozePresetSetting> = DEFAULT_SNOOZE_PRESETS) =
        resolveSnoozePresets(now, presets) { "${it.hour}:%02d".format(it.minute) }
            .map { "${it.label} ${it.whenLabel}" }

    @Test
    fun `default menu on a weekday morning`() {
        // Wednesday 10:00.
        assertEquals(
            listOf("In 1 hour 11:00", "In 3 hours 13:00", "This evening 18:00", "Tomorrow 9:00", "Next week Mon 9:00"),
            menu(at(8, 10)),
        )
    }

    @Test
    fun `lists custom presets soonest first and names the day when the label does not`() {
        // Wednesday 20:00.
        val presets =
            listOf(
                SnoozePresetSetting.At("monday", 18, 0),
                SnoozePresetSetting.At("tomorrow", 21, 0),
                SnoozePresetSetting.After(360),
                SnoozePresetSetting.After(180),
            )
        assertEquals(
            listOf("In 3 hours 23:00", "In 6 hours Thu 2:00", "Tomorrow 21:00", "Next week Mon 18:00"),
            menu(at(8, 20), presets),
        )
    }

    @Test
    fun `drops a passed today preset and moves a weekday on that day to next week`() {
        // Monday 18:30.
        val presets =
            resolveSnoozePresets(
                at(13, 18, 30),
                listOf(SnoozePresetSetting.At("today", 18, 0), SnoozePresetSetting.At("monday", 9, 0)),
            )
        assertEquals(listOf("at:monday:09:00"), presets.map { it.id })
        assertEquals("2026-04-20T09:00:00Z", presets.single().snoozedUntilIso)
    }

    @Test
    fun `names the real day gap`() {
        val presets =
            listOf(
                SnoozePresetSetting.At("tomorrow", 21, 0),
                SnoozePresetSetting.At("monday", 18, 0),
                SnoozePresetSetting.At("friday", 17, 0),
            )
        // Sunday 14:00: the Monday option is tomorrow.
        assertEquals(listOf("Tomorrow 18:00", "Tomorrow 21:00", "Friday 17:00"), menu(at(12, 14), presets))
        // Friday 10:00: today's 17:00 is a week away.
        assertEquals(
            listOf("Tomorrow 21:00", "Next week Mon 18:00", "Next Friday 17:00"),
            menu(at(10, 10), presets),
        )
    }

    @Test
    fun `one row per wake minute`() {
        // Sunday, so Tomorrow 9:00 and Next week 9:00 are the same Monday.
        assertEquals(
            listOf("In 1 hour 8:01", "In 3 hours 10:01", "This evening 18:00", "Tomorrow 9:00"),
            menu(at(12, 7, 1)),
        )
        // A duration keeps the clock's seconds but still ties with a set time in that minute.
        assertEquals(
            listOf("In 3 hours 18:00"),
            menu(at(8, 15, 0, 25), listOf(SnoozePresetSetting.After(180), SnoozePresetSetting.At("today", 18, 0))),
        )
    }

    @Test
    fun `parses server presets and falls back to defaults without them`() {
        assertEquals(DEFAULT_SNOOZE_PRESETS, parseSnoozePresetSettings(null))
        val raw =
            Json.parseToJsonElement(
                """[{"kind":"after","minutes":90},{"kind":"at","day":"friday","time":"17:30"},
                   {"kind":"at","day":"someday","time":"09:00"},{"kind":"after","minutes":0}]"""
            ) as JsonArray
        assertEquals(
            listOf(SnoozePresetSetting.After(90), SnoozePresetSetting.At("friday", 17, 30)),
            parseSnoozePresetSettings(raw),
        )
        assertEquals("In 90 minutes", resolveSnoozePreset(SnoozePresetSetting.After(90), at(8, 10))?.label)
    }

    @Test
    fun `a duration starts when picked and a set time keeps its shown wake`() {
        val shown = resolveSnoozePresets(at(8, 15), listOf(SnoozePresetSetting.After(180), SnoozePresetSetting.At("tomorrow", 9, 0)))
        val actions = shown.map(::snoozeMenuAction)
        assertEquals(listOf("snooze:after:180", "snooze:2026-04-09T09:00:00Z"), actions)
        val later = at(8, 15, 30).toInstant()
        assertEquals("snooze:2026-04-08T18:30:00Z", resolveSnoozeMenuAction(actions[0], later))
        // Past midnight, "Tomorrow" still means the morning the menu showed.
        assertEquals(actions[1], resolveSnoozeMenuAction(actions[1], at(9, 0, 1).toInstant()))
        assertNull(resolveSnoozeMenuAction(actions[1], at(9, 9, 1).toInstant()))
        assertEquals("pin", resolveSnoozeMenuAction("pin", later))
        assertEquals("snooze:custom", resolveSnoozeMenuAction("snooze:custom", later))
    }
}
