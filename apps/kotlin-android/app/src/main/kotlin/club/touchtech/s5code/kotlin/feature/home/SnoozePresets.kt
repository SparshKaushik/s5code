package club.touchtech.s5code.kotlin.feature.home

import java.time.DayOfWeek
import java.time.Instant
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.temporal.ChronoUnit
import java.util.Locale
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonPrimitive

/** A snooze preset choice matching the desktop and web clients. */
data class SnoozePreset(
    val id: String,
    val label: String,
    val whenLabel: String,
    val snoozedUntilIso: String,
)

/**
 * One configured snooze option, the contract's `SnoozePresetSetting`: [After]
 * wakes a fixed time from now, [At] at a local time on a day ("today",
 * "tomorrow", or a weekday's next occurrence after today).
 */
sealed interface SnoozePresetSetting {
    data class After(val minutes: Int) : SnoozePresetSetting

    data class At(val day: String, val hour: Int, val minute: Int) : SnoozePresetSetting
}

/** `DEFAULT_SNOOZE_PRESETS`, what a server without the setting offers. */
val DEFAULT_SNOOZE_PRESETS: List<SnoozePresetSetting> =
    listOf(
        SnoozePresetSetting.After(60),
        SnoozePresetSetting.After(180),
        SnoozePresetSetting.At("today", 18, 0),
        SnoozePresetSetting.At("tomorrow", 9, 0),
        SnoozePresetSetting.At("monday", 9, 0),
    )

private const val MAX_SNOOZE_PRESET_MINUTES = 30 * 24 * 60
private val SNOOZE_PRESET_TIME = Regex("""^([01]\d|2[0-3]):([0-5]\d)$""")
private val WEEKDAYS = DayOfWeek.entries.associateBy { it.name.lowercase() }

/**
 * The server's `settings.snoozePresets`; null (an older server) means the
 * defaults. Entries this client cannot read are skipped rather than failing
 * the whole menu.
 */
fun parseSnoozePresetSettings(raw: JsonArray?): List<SnoozePresetSetting> {
    raw ?: return DEFAULT_SNOOZE_PRESETS
    return raw.mapNotNull { element ->
        val entry = element as? JsonObject ?: return@mapNotNull null
        when (entry["kind"]?.jsonPrimitive?.contentOrNull) {
            "after" ->
                entry["minutes"]?.jsonPrimitive?.intOrNull
                    ?.takeIf { it in 1..MAX_SNOOZE_PRESET_MINUTES }
                    ?.let(SnoozePresetSetting::After)
            "at" -> {
                val day = entry["day"]?.jsonPrimitive?.contentOrNull
                    ?.takeIf { it == "today" || it == "tomorrow" || it in WEEKDAYS }
                val time = entry["time"]?.jsonPrimitive?.contentOrNull?.let(SNOOZE_PRESET_TIME::matchEntire)
                if (day == null || time == null) null
                else SnoozePresetSetting.At(day, time.groupValues[1].toInt(), time.groupValues[2].toInt())
            }
            else -> null
        }
    }
}

fun snoozePresetId(setting: SnoozePresetSetting): String =
    when (setting) {
        is SnoozePresetSetting.After -> "after:${setting.minutes}"
        is SnoozePresetSetting.At -> "at:${setting.day}:%02d:%02d".format(setting.hour, setting.minute)
    }

private fun pluralize(count: Int, unit: String) = "$count $unit${if (count == 1) "" else "s"}"

/** "3 hours", "90 minutes", "2 days": the largest unit that divides evenly. */
private fun formatSnoozeDuration(minutes: Int): String =
    when {
        minutes % (24 * 60) == 0 -> pluralize(minutes / (24 * 60), "day")
        minutes % 60 == 0 -> pluralize(minutes / 60, "hour")
        else -> pluralize(minutes, "minute")
    }

private fun daysAhead(setting: SnoozePresetSetting.At, now: ZonedDateTime): Int =
    when (setting.day) {
        "today" -> 0
        "tomorrow" -> 1
        else -> ((WEEKDAYS.getValue(setting.day).value - now.dayOfWeek.value + 7) % 7).takeIf { it != 0 } ?: 7
    }

/** Labels follow the actual day gap, so a Monday option on a Sunday reads "Tomorrow". */
private fun snoozePresetLabel(setting: SnoozePresetSetting, daysAhead: Int): String {
    if (setting is SnoozePresetSetting.After) return "In ${formatSnoozeDuration(setting.minutes)}"
    setting as SnoozePresetSetting.At
    val weekday = setting.day.replaceFirstChar(Char::uppercase)
    return when {
        daysAhead == 0 ->
            when {
                setting.hour < 12 -> "This morning"
                setting.hour < 17 -> "This afternoon"
                else -> "This evening"
            }
        daysAhead == 1 -> "Tomorrow"
        setting.day == "monday" -> "Next week"
        daysAhead == 7 -> "Next $weekday"
        else -> weekday
    }
}

private fun defaultSnoozeTime(time: ZonedDateTime): String =
    time.format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT))

/** `resolveSnoozePreset`; null for a today option whose time has passed. */
fun resolveSnoozePreset(
    setting: SnoozePresetSetting,
    now: ZonedDateTime,
    formatTime: (ZonedDateTime) -> String = ::defaultSnoozeTime,
): SnoozePreset? {
    val ahead: Int
    val wake: ZonedDateTime
    when (setting) {
        is SnoozePresetSetting.After -> {
            ahead = 0
            wake = now.plusMinutes(setting.minutes.toLong())
        }
        is SnoozePresetSetting.At -> {
            ahead = daysAhead(setting, now)
            // Calendar days, not 24-hour steps, so a DST change keeps the wall time.
            wake = now.toLocalDate().plusDays(ahead.toLong()).atTime(setting.hour, setting.minute).atZone(now.zone)
            if (!wake.isAfter(now)) return null
        }
    }
    val label = snoozePresetLabel(setting, ahead)
    // Durations and "Next week" don't name the wake day, so a wake on another day shows it.
    val labelNamesDay = setting is SnoozePresetSetting.At && label != "Next week"
    val time = formatTime(wake)
    return SnoozePreset(
        id = snoozePresetId(setting),
        label = label,
        whenLabel =
            if (labelNamesDay || wake.toLocalDate() == now.toLocalDate()) time
            else "${wake.format(DateTimeFormatter.ofPattern("EEE", Locale.getDefault()))} $time",
        snoozedUntilIso = wake.toInstant().toString(),
    )
}

/**
 * `resolveSnoozePresets`: the snooze menu, soonest first, one row per wake
 * minute. By minute so a duration (which keeps the clock's seconds) and a set
 * time in the same minute tie; the stable sort keeps the one listed first.
 */
fun resolveSnoozePresets(
    now: ZonedDateTime = ZonedDateTime.now(),
    presets: List<SnoozePresetSetting> = DEFAULT_SNOOZE_PRESETS,
    formatTime: (ZonedDateTime) -> String = ::defaultSnoozeTime,
): List<SnoozePreset> =
    presets
        .mapNotNull { resolveSnoozePreset(it, now, formatTime) }
        .sortedBy { wakeMinute(it) }
        .distinctBy { wakeMinute(it) }

private fun wakeMinute(preset: SnoozePreset): Long =
    Instant.parse(preset.snoozedUntilIso).truncatedTo(ChronoUnit.MINUTES).epochSecond

private const val SNOOZE_AFTER_ACTION = "snooze:after:"

/**
 * The row menu's action for [preset]. A duration is resolved again when picked
 * (see [resolveSnoozeMenuAction]); a set time carries the wake the menu showed.
 */
fun snoozeMenuAction(preset: SnoozePreset): String =
    if (preset.id.startsWith("after:")) "snooze:${preset.id}" else "snooze:${preset.snoozedUntilIso}"

/**
 * `pickSnoozePreset` for a menu action: a duration starts at selection, a set
 * time keeps the shown wake, which a day change would otherwise shift. Null when
 * that wake has passed while the menu was open; other actions pass through.
 */
fun resolveSnoozeMenuAction(action: String, now: Instant = Instant.now()): String? {
    if (action.startsWith(SNOOZE_AFTER_ACTION)) {
        val minutes = action.removePrefix(SNOOZE_AFTER_ACTION).toLongOrNull() ?: return null
        return "snooze:${now.plus(minutes, ChronoUnit.MINUTES)}"
    }
    if (!action.startsWith("snooze:") || action == "snooze:custom") return action
    val until = runCatching { Instant.parse(action.removePrefix("snooze:")) }.getOrNull() ?: return action
    return action.takeIf { until.isAfter(now) }
}
