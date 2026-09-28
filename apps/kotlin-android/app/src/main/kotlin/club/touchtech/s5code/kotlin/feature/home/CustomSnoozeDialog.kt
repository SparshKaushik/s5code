package club.touchtech.s5code.kotlin.feature.home

import android.app.DatePickerDialog
import android.app.TimePickerDialog
import android.text.format.DateFormat
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import java.time.Instant
import java.time.LocalDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

/**
 * `CustomSnoozeSheet` on RN: a custom wake time, either as a calendar
 * date+time or a duration. Uses the platform pickers (`DatePickerDialog` /
 * `TimePickerDialog`) like the RN sheet's Compose `DateTimePicker` does.
 */

/** `localSnoozeDate`/`localSnoozeTime`: the `yyyy-MM-dd`/`HH:mm` wire pieces. */
private fun localDateString(dateTime: LocalDateTime): String =
    "%04d-%02d-%02d".format(dateTime.year, dateTime.monthValue, dateTime.dayOfMonth)

private fun localTimeString(dateTime: LocalDateTime): String =
    "%02d:%02d".format(dateTime.hour, dateTime.minute)

/**
 * `resolveCustomSnooze`: rejects past wakes and impossible local times (the
 * DST-folded or skipped hour the picker can hand back), returning the ISO
 * instant the command carries.
 */
private fun resolveCustomSnoozeIso(
    mode: String,
    dateTime: LocalDateTime,
    amount: Int,
    unit: String,
    zone: ZoneId,
    now: Instant,
): String? {
    val wake =
        if (mode == "duration") {
            if (amount <= 0) return null
            val unitMinutes =
                when (unit) {
                    "minutes" -> 1L
                    "hours" -> 60L
                    else -> 24L * 60L
                }
            Instant.ofEpochMilli(now.toEpochMilli() + amount * unitMinutes * 60_000L)
        } else {
            val zoned = dateTime.atZone(zone)
            // A local time that doesn't exist resolves forward; reject the
            // rollover like RN's same-string check does.
            val roundTrip = zoned.toLocalDateTime()
            if (
                localDateString(roundTrip) != localDateString(dateTime) ||
                localTimeString(roundTrip) != localTimeString(dateTime)
            ) {
                return null
            }
            zoned.toInstant()
        }
    return if (wake.isAfter(now)) wake.toString() else null
}

@Composable
fun CustomSnoozeDialog(
    onDismiss: () -> Unit,
    onSnooze: (untilIso: String) -> Unit,
) {
    val context = LocalContext.current
    val zone = remember { ZoneId.systemDefault() }
    var mode by remember { mutableStateOf("date") }
    // RN seeds the pickers an hour out so a tapped OK almost always lands in
    // the future.
    var dateTime by remember { mutableStateOf(LocalDateTime.now(zone).plusHours(1)) }
    var amount by remember { mutableIntStateOf(2) }
    var unit by remember { mutableStateOf("hours") }
    var error by remember { mutableStateOf<String?>(null) }

    val is24Hour = DateFormat.is24HourFormat(context)
    val dateFormatter =
        remember { DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(Locale.getDefault()) }
    val timeFormatter =
        remember(is24Hour) {
            DateTimeFormatter.ofPattern(if (is24Hour) "HH:mm" else "h:mm a")
                .withLocale(Locale.getDefault())
        }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Custom snooze") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium)) {
                SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                    listOf("date" to "Date and time", "duration" to "Duration")
                        .forEachIndexed { index, (value, label) ->
                            SegmentedButton(
                                selected = mode == value,
                                onClick = {
                                    mode = value
                                    error = null
                                },
                                shape =
                                    SegmentedButtonDefaults.itemShape(
                                        index = index,
                                        count = 2,
                                    ),
                            ) {
                                Text(label)
                            }
                        }
                }
                if (mode == "date") {
                    Row(
                        Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceEvenly,
                    ) {
                        TextButton(
                            onClick = {
                                DatePickerDialog(
                                        context,
                                        { _, year, month, day ->
                                            dateTime =
                                                dateTime
                                                    .withYear(year)
                                                    .withMonth(month + 1)
                                                    .withDayOfMonth(day)
                                            error = null
                                        },
                                        dateTime.year,
                                        dateTime.monthValue - 1,
                                        dateTime.dayOfMonth,
                                    )
                                    .show()
                            }
                        ) {
                            Text(dateTime.toLocalDate().format(dateFormatter))
                        }
                        TextButton(
                            onClick = {
                                TimePickerDialog(
                                        context,
                                        { _, hour, minute ->
                                            dateTime =
                                                dateTime.withHour(hour).withMinute(minute)
                                                    .withSecond(0)
                                                    .withNano(0)
                                            error = null
                                        },
                                        dateTime.hour,
                                        dateTime.minute,
                                        is24Hour,
                                    )
                                    .show()
                            }
                        ) {
                            Text(dateTime.toLocalTime().format(timeFormatter))
                        }
                    }
                } else {
                    Row(
                        Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceEvenly,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        FilledTonalIconButton(
                            enabled = amount > 1,
                            onClick = { amount = (amount - 1).coerceAtLeast(1) },
                            modifier = Modifier.size(48.dp),
                        ) {
                            Text("−", style = MaterialTheme.typography.titleLarge)
                        }
                        Text("$amount", style = MaterialTheme.typography.titleLarge)
                        FilledTonalIconButton(
                            enabled = amount < 99,
                            onClick = { amount = (amount + 1).coerceAtMost(99) },
                            modifier = Modifier.size(48.dp),
                        ) {
                            Text("+", style = MaterialTheme.typography.titleLarge)
                        }
                    }
                    SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                        listOf("minutes" to "Minutes", "hours" to "Hours", "days" to "Days")
                            .forEachIndexed { index, (value, label) ->
                                SegmentedButton(
                                    selected = unit == value,
                                    onClick = { unit = value },
                                    shape =
                                        SegmentedButtonDefaults.itemShape(
                                            index = index,
                                            count = 3,
                                        ),
                                ) {
                                    Text(label)
                                }
                            }
                    }
                }
                error?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    val until =
                        resolveCustomSnoozeIso(
                            mode = mode,
                            dateTime = dateTime,
                            amount = amount,
                            unit = unit,
                            zone = zone,
                            now = Instant.now(),
                        )
                    if (until == null) {
                        error =
                            if (mode == "date") "Choose a date and time in the future."
                            else "Enter a positive duration."
                    } else {
                        onSnooze(until)
                    }
                }
            ) {
                Text("Snooze")
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}
