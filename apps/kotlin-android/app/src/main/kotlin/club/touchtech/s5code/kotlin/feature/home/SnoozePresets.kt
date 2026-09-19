package club.touchtech.s5code.kotlin.feature.home

import java.time.DayOfWeek
import java.time.Duration
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

/** A snooze preset choice matching the desktop and web clients. */
data class SnoozePreset(
    val id: String,
    val label: String,
    val whenLabel: String,
    val snoozedUntilIso: String,
)

/**
 * Shared "snooze until" choices for every client.
 *
 * Matches `resolveSnoozePresets` in `packages/client-runtime/src/state/threadSettled.ts`:
 * - "In 1 hour": current time + 1 hour.
 * - "This evening": today at 18:00 (6:00 PM), only if more than 1 hour away.
 * - "Tomorrow": tomorrow at 09:00 (9:00 AM).
 * - "Next week": next Monday at 09:00 (9:00 AM).
 */
fun resolveSnoozePresets(now: ZonedDateTime = ZonedDateTime.now()): List<SnoozePreset> {
    val presets = mutableListOf<SnoozePreset>()
    val timeFormatter = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)

    // 1. In 1 hour
    val inAnHour = now.plusHours(1)
    presets +=
        SnoozePreset(
            id = "hour",
            label = "In 1 hour",
            whenLabel = inAnHour.format(timeFormatter),
            snoozedUntilIso = inAnHour.toInstant().toString(),
        )

    // 2. This evening (18:00), only if more than 1 hour away
    val evening = now.toLocalDate().atTime(18, 0).atZone(now.zone)
    if (Duration.between(now, evening).toMinutes() > 60) {
        presets +=
            SnoozePreset(
                id = "evening",
                label = "This evening",
                whenLabel = evening.format(timeFormatter),
                snoozedUntilIso = evening.toInstant().toString(),
            )
    }

    // 3. Tomorrow morning (09:00)
    val tomorrow = now.toLocalDate().plusDays(1).atTime(9, 0).atZone(now.zone)
    presets +=
        SnoozePreset(
            id = "tomorrow",
            label = "Tomorrow",
            whenLabel = tomorrow.format(timeFormatter),
            snoozedUntilIso = tomorrow.toInstant().toString(),
        )

    // 4. Next week (next Monday at 09:00)
    val daysUntilMonday = (DayOfWeek.MONDAY.value - now.dayOfWeek.value + 7) % 7
    val daysToAdd = if (daysUntilMonday == 0) 7L else daysUntilMonday.toLong()
    val nextMonday = now.toLocalDate().plusDays(daysToAdd).atTime(9, 0).atZone(now.zone)
    val weekdayFormatter = DateTimeFormatter.ofPattern("EEE", Locale.getDefault())
    presets +=
        SnoozePreset(
            id = "next-week",
            label = "Next week",
            whenLabel = "${nextMonday.format(weekdayFormatter)} ${nextMonday.format(timeFormatter)}",
            snoozedUntilIso = nextMonday.toInstant().toString(),
        )

    return presets
}
