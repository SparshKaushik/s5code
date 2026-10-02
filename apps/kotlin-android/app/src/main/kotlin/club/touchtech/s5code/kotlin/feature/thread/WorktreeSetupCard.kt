package club.touchtech.s5code.kotlin.feature.thread

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.RadioButtonUnchecked
import androidx.compose.material.icons.rounded.Remove
import androidx.compose.material.icons.rounded.Warning
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.component.S5BottomSheet
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.WorktreeSetupSnapshot
import club.touchtech.s5code.kotlin.model.WorktreeSetupStage
import kotlinx.coroutines.delay

/**
 * The bootstrap worktree's progress, matching RN's `WorktreeSetupCard`: a
 * border-bottom row (the same plain row the working header uses, not a card)
 * that lists its stages until the agent takes over, then collapses into the
 * "Working for …" header while a setup script keeps running behind it.
 *
 * Details open in a sheet with the full stage list, the script's output tail,
 * and the Cancel setup / Work locally escape hatches while setup is running.
 */
@Composable
fun WorktreeSetupCard(
    snapshot: WorktreeSetupSnapshot,
    turnStarted: Boolean,
    turnStartedAtMillis: Long?,
    /** A turn is producing work (drives the "Working for" header wording). */
    working: Boolean,
    onCancel: () -> Unit,
    /** Null when the creation payload was not retained for a local resend. */
    onWorkLocally: (() -> Unit)?,
    modifier: Modifier = Modifier,
) {
    val handedOff = turnStarted && snapshot.agentStarted
    val running = snapshot.isRunning
    val failed =
        snapshot.phase == "failed" || snapshot.stages.any { it.isFailed }
    var detailsOpen by remember(snapshot.threadId) { mutableStateOf(false) }
    val now by setupClock(running || working)
    val scriptName = snapshot.setupScript?.name ?: "Setup script"
    val label =
        when {
            handedOff && working ->
                "Working for ${elapsed(turnStartedAtMillis, null, now) ?: "0s"}"
            running -> if (handedOff) "Setup continues…" else "Setting up worktree…"
            snapshot.phase == "cancelled" -> "Worktree setup cancelled"
            snapshot.phase == "failed" -> "Worktree setup failed"
            failed -> "Setup script failed"
            else -> "Worktree ready"
        }

    Column(modifier) {
        Row(
            Modifier.fillMaxWidth().padding(vertical = S5Theme.spacing.small),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Text(
                label,
                style = MaterialTheme.typography.labelLarge,
                color =
                    if (failed && !working) MaterialTheme.colorScheme.error
                    else MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            if (!handedOff) {
                Text(
                    elapsed(snapshot.startedAtMillis, snapshot.endedAtMillis, now).orEmpty(),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Row(
                Modifier.clickable { detailsOpen = true },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
            ) {
                if (handedOff && running) {
                    S5InlineLoading(Modifier.size(12.dp))
                } else if (failed) {
                    Icon(
                        Icons.Rounded.ErrorOutline,
                        contentDescription = null,
                        modifier = Modifier.size(14.dp),
                        tint = MaterialTheme.colorScheme.error,
                    )
                }
                Text(
                    if (handedOff && running) scriptName else "Details",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        if (!handedOff) {
            snapshot.stages
                .filter { it.id != "agent" }
                .forEach { stage ->
                    SetupStageRow(
                        stage = stage,
                        scriptName = snapshot.setupScript?.name,
                        now = now,
                        compact = true,
                    )
                }
        }
    }

    if (detailsOpen) {
        WorktreeSetupDetailsSheet(
            snapshot = snapshot,
            turnStarted = turnStarted,
            now = now,
            onCancel = {
                detailsOpen = false
                onCancel()
            },
            onWorkLocally =
                onWorkLocally?.let { workLocally ->
                    {
                        detailsOpen = false
                        workLocally()
                    }
                },
            onDismiss = { detailsOpen = false },
        )
    }
}

@Composable
private fun WorktreeSetupDetailsSheet(
    snapshot: WorktreeSetupSnapshot,
    turnStarted: Boolean,
    now: Long,
    onCancel: () -> Unit,
    onWorkLocally: (() -> Unit)?,
    onDismiss: () -> Unit,
) {
    // Cancel is offered only while setup is still preparing the turn; once the
    // agent has started, stopping it is interrupting a turn, not cancelling
    // setup (`canCancel` in the RN card).
    val canCancel = snapshot.isRunning && !turnStarted
    S5BottomSheet(onDismiss = onDismiss, title = "Worktree setup") {
        Column(
            Modifier.verticalScroll(rememberScrollState()).padding(bottom = S5Theme.spacing.medium)
        ) {
            snapshot.stages
                .filter { it.id != "agent" }
                .forEach { stage ->
                    SetupStageRow(
                        stage = stage,
                        scriptName = snapshot.setupScript?.name,
                        now = now,
                    )
                    if (
                        stage.id == "setup-script" &&
                            (stage.status == "running" ||
                                stage.isFailed ||
                                stage.tail.isNotEmpty())
                    ) {
                        SetupOutputTail(stage.tail, failed = stage.isFailed)
                    }
                }
            if (snapshot.phase == "failed" && snapshot.error != null) {
                Text(
                    snapshot.error,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(start = 32.dp, top = S5Theme.spacing.small),
                )
            }
            if (canCancel) {
                Row(
                    Modifier.fillMaxWidth().padding(top = S5Theme.spacing.small),
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    Spacer(Modifier.weight(1f))
                    S5Button(
                        text = "Cancel setup",
                        onClick = onCancel,
                        emphasis = S5ActionEmphasis.Secondary,
                        style = S5ButtonStyle.Outlined,
                    )
                    if (onWorkLocally != null) {
                        S5Button(
                            text = "Work locally",
                            onClick = onWorkLocally,
                            emphasis = S5ActionEmphasis.Secondary,
                            style = S5ButtonStyle.Text,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun SetupStageRow(
    stage: WorktreeSetupStage,
    scriptName: String?,
    now: Long,
    compact: Boolean = false,
) {
    val label = if (stage.id == "setup-script") (scriptName ?: "Run setup script") else stageLabel(stage.id)
    val detail =
        when {
            stage.status == "pending" -> null
            stage.status == "skipped" -> stage.detail ?: "skipped"
            stage.id == "checkout" && stage.status == "running" && stage.percent != null ->
                "${stage.percent}%"
            else -> stage.detail
        }
    Row(
        Modifier.fillMaxWidth()
            .padding(vertical = if (compact) S5Theme.spacing.tiny else S5Theme.spacing.small),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        Box(Modifier.width(24.dp), contentAlignment = Alignment.Center) {
            when (stage.status) {
                "running" -> S5InlineLoading(Modifier.size(14.dp))
                else ->
                    Icon(
                        when (stage.status) {
                            "done" -> Icons.Rounded.Check
                            "skipped" -> Icons.Rounded.Remove
                            "failed" -> Icons.Rounded.Close
                            "warning" -> Icons.Rounded.Warning
                            else -> Icons.Rounded.RadioButtonUnchecked
                        },
                        contentDescription = null,
                        modifier = Modifier.size(14.dp),
                        tint =
                            when (stage.status) {
                                "failed" -> MaterialTheme.colorScheme.error
                                // No warning tone in the palette: approval is the
                                // attention color everywhere else in the app.
                                "warning" -> S5Theme.status.approval
                                else -> MaterialTheme.colorScheme.onSurfaceVariant
                            },
                    )
            }
        }
        Text(
            label,
            style = MaterialTheme.typography.bodySmall,
            color =
                when (stage.status) {
                    "failed" -> MaterialTheme.colorScheme.error
                    else -> MaterialTheme.colorScheme.onSurfaceVariant
                },
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        if (detail != null) {
            Text(
                detail,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (stage.status != "pending" && stage.status != "skipped") {
            Text(
                elapsed(stage.startedAtMillis, stage.endedAtMillis, now).orEmpty(),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

private const val OUTPUT_TAIL_SLOTS = 4

/** Fixed four-line output window, shown only in Details, matching the RN card. */
@Composable
private fun SetupOutputTail(lines: List<String>, failed: Boolean) {
    Column(
        Modifier.padding(start = 32.dp, bottom = S5Theme.spacing.small)
    ) {
        repeat(OUTPUT_TAIL_SLOTS) { slot ->
            val line = lines.getOrNull(lines.size - OUTPUT_TAIL_SLOTS + slot) ?: " "
            Text(
                line,
                style =
                    MaterialTheme.typography.labelSmall.copy(fontFamily = FontFamily.Monospace),
                color =
                    if (failed) MaterialTheme.colorScheme.error
                    else MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

private fun stageLabel(id: String): String =
    when (id) {
        "fetch" -> "Fetch base branch"
        "checkout" -> "Check out files"
        "submodules" -> "Init submodules"
        "setup-script" -> "Run setup script"
        "agent" -> "Start agent"
        else -> id
    }

private fun elapsed(startMillis: Long?, endMillis: Long?, nowMillis: Long): String? {
    val start = startMillis ?: return null
    return formatDuration(((endMillis ?: nowMillis) - start).coerceAtLeast(0))
}

/**
 * A once-per-second clock while setup (or the turn it handed off to) is running.
 * Stopped setups render their recorded span and never tick — a finished card has
 * no reason to repaint.
 */
@Composable
private fun setupClock(active: Boolean) =
    produceState(System.currentTimeMillis(), active) {
        while (active) {
            delay(1_000)
            value = System.currentTimeMillis()
        }
    }
