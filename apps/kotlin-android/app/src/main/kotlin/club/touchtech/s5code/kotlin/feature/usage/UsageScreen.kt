package club.touchtech.s5code.kotlin.feature.usage

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.drawscope.Fill
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.Remote
import club.touchtech.s5code.kotlin.data.rememberRetryableRemote
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5ConnectedButtonGroup
import club.touchtech.s5code.kotlin.design.component.S5ErrorState
import club.touchtech.s5code.kotlin.design.component.S5LoadingState
import club.touchtech.s5code.kotlin.design.component.S5ProviderAvatar
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SectionHeader
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.Usage
import club.touchtech.s5code.kotlin.model.UsageWindow

/** Cost/token mode for the usage view. */
private enum class UsageMode(val label: String) {
    Cost("Cost"),
    Tokens("Tokens"),
}

/** Usage totals, a daily bar chart, and per-provider/model breakdown. */
@Composable
fun UsageScreen(store: AppStore, onBack: () -> Unit) {
    var window by remember { mutableStateOf(UsageWindow.Month) }
    var mode by remember { mutableStateOf(UsageMode.Cost) }
    // Usage is read per environment and summed, so it is a request rather than a
    // subscription: it has a loading state and it can fail. Changing the window is a
    // new request, which is why it keys the read.
    val (state, retry) = rememberRetryableRemote(window) { store.workspace.usage(window) }
    val remote = state.value

    S5Screen(
        title = "Usage",
        subtitle = remote.valueOrNull?.environments?.joinToString(" · ").orEmpty(),
        prominence = S5TopBarProminence.Hero,
        onBack = onBack,
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            // Outside the loading branch on purpose: the window control must stay put
            // while the new window loads, or every switch collapses the screen to a
            // spinner and the user loses the row they were aiming at.
            Box(
                Modifier.padding(
                    horizontal = S5Theme.spacing.gutter,
                    vertical = S5Theme.spacing.small,
                )
            ) {
                S5ConnectedButtonGroup(
                    options = UsageWindow.entries,
                    selected = window,
                    onSelect = { window = it },
                    label = { it.label },
                )
            }
            when (remote) {
                is Remote.Loading -> S5LoadingState("Adding up usage…")
                is Remote.Failed ->
                    Box(Modifier.padding(S5Theme.spacing.gutter)) {
                        S5ErrorState(
                            title = "Couldn't read usage",
                            detail = remote.message,
                            onRetry = retry,
                        )
                    }
                is Remote.Loaded ->
                    UsageBody(usage = remote.value, mode = mode, onMode = { mode = it })
            }
        }
    }
}

@Composable
private fun UsageBody(usage: Usage, mode: UsageMode, onMode: (UsageMode) -> Unit) {
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                S5Card(tone = S5CardTone.Hero, modifier = Modifier.fillMaxWidth()) {
                    Column(
                        Modifier.padding(S5Theme.spacing.xLarge),
                        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
                    ) {
                        Text(
                            "$${"%.2f".format(usage.totals.costUsd)}",
                            style = MaterialTheme.typography.displaySmallEmphasized,
                        )
                        Text(
                            "${usage.totals.requests} requests · ${usage.window.label.lowercase()}",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Row(
                            Modifier.padding(top = S5Theme.spacing.small),
                            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.large),
                        ) {
                            TokenStat("Input", usage.totals.inputTokens)
                            TokenStat("Output", usage.totals.outputTokens)
                            TokenStat("Cached", usage.totals.cacheReadTokens)
                        }
                    }
                }
            }

            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                S5ConnectedButtonGroup(
                    options = UsageMode.entries,
                    selected = mode,
                    onSelect = onMode,
                    label = { it.label },
                )
            }

            S5SectionHeader(if (usage.window.hourly) "Hourly" else "Daily")
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                S5Card(tone = S5CardTone.Standard, modifier = Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(S5Theme.spacing.large)) {
                        val values =
                            usage.days.map {
                                if (mode == UsageMode.Cost) it.costUsd.toFloat() else it.tokens.toFloat()
                            }
                        val max = values.maxOrNull() ?: 1f
                        val barColor = MaterialTheme.colorScheme.primary
                        val description =
                            usage.days
                                .map { day ->
                                    val value =
                                        if (mode == UsageMode.Cost) "$${"%.2f".format(day.costUsd)}"
                                        else formatTokens(day.tokens)
                                    "${day.label}: $value"
                                }
                                .joinToString(", ")
                        // Static chart: no continuous repaint, and the whole
                        // series is exposed to TalkBack as one label.
                        Canvas(
                            Modifier.fillMaxWidth()
                                .height(140.dp)
                                .semantics {
                                    contentDescription =
                                        "${usage.window.label} usage. $description"
                                }
                        ) {
                            // Gaps scale down as the window grows: a fixed 10px gap
                            // between 90 bars leaves no bar.
                            val gap = if (values.size > 40) 2f else if (values.size > 12) 5f else 10f
                            val barWidth = (size.width - gap * (values.size - 1)) / values.size
                            values.forEachIndexed { index, value ->
                                val barHeight = if (max == 0f) 0f else size.height * (value / max)
                                drawRoundRect(
                                    color = barColor,
                                    topLeft =
                                        Offset(
                                            x = index * (barWidth + gap),
                                            y = size.height - barHeight,
                                        ),
                                    size = Size(barWidth, barHeight),
                                    cornerRadius = CornerRadius(8f, 8f),
                                    style = Fill,
                                )
                            }
                        }
                        Row(
                            Modifier.fillMaxWidth().padding(top = S5Theme.spacing.small),
                            horizontalArrangement = Arrangement.SpaceBetween,
                        ) {
                            // Thinned to about six ticks: 24 hours or 90 days of
                            // labels at this width is a grey smear.
                            val stride = (usage.days.size / 6).coerceAtLeast(1)
                            usage.days.forEachIndexed { index, day ->
                                if (index % stride == 0) {
                                    Text(day.label, style = MaterialTheme.typography.labelSmall)
                                }
                            }
                        }
                    }
                }
            }

            S5SectionHeader("Providers")
            Column(
                Modifier.padding(horizontal = S5Theme.spacing.gutter),
                verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                usage.providers.forEach { breakdown ->
                    S5Card(tone = S5CardTone.Standard, modifier = Modifier.fillMaxWidth()) {
                        Column(
                            Modifier.padding(S5Theme.spacing.large),
                            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                        ) {
                            Row(
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                            ) {
                                S5ProviderAvatar(breakdown.provider, size = 36.dp)
                                Column(Modifier.weight(1f)) {
                                    Text(
                                        breakdown.provider.label,
                                        style = MaterialTheme.typography.titleSmallEmphasized,
                                    )
                                    Text(
                                        if (mode == UsageMode.Cost) {
                                            "$${"%.2f".format(breakdown.costUsd)}"
                                        } else {
                                            formatTokens(breakdown.tokens)
                                        },
                                        style = MaterialTheme.typography.bodyMedium,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                                Text(
                                    "${(breakdown.share * 100).toInt()}%",
                                    style = MaterialTheme.typography.labelLargeEmphasized,
                                )
                            }
                            LinearProgressIndicator(
                                progress = { breakdown.share },
                                modifier = Modifier.fillMaxWidth(),
                            )
                            breakdown.models.forEach { model ->
                                Row(
                                    Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                ) {
                                    Text(model.model, style = S5Theme.code.inlineTechnical)
                                    Text(
                                        if (mode == UsageMode.Cost) {
                                            "$${"%.2f".format(model.costUsd)}"
                                        } else {
                                            formatTokens(model.tokens)
                                        },
                                        style = MaterialTheme.typography.labelSmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            }
                        }
                    }
                }
            }
            Box(Modifier.padding(bottom = S5Theme.spacing.section))
        }
}

@Composable
private fun TokenStat(label: String, value: Long) {
    Column {
        Text(
            label,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(formatTokens(value), style = MaterialTheme.typography.titleSmallEmphasized)
    }
}

private fun formatTokens(value: Long): String =
    when {
        value >= 1_000_000 -> "${"%.1f".format(value / 1_000_000.0)}M"
        value >= 1_000 -> "${"%.1f".format(value / 1_000.0)}k"
        else -> value.toString()
    }
