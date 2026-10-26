package club.touchtech.s5code.kotlin.feature.thread

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.model.*
import kotlinx.coroutines.launch
import kotlinx.serialization.json.*

/** Server queues are shared between clients; local outbox messages remain delivery state. */
@Composable
internal fun V2ThreadControls(store: AppStore, env: EnvironmentId, id: ThreadId, detail: ThreadDetail,
    working: Boolean, followUp: String, onFollowUp: (String) -> Unit, onOpenThread: (String) -> Unit,
    /** [ThreadDetail.canSteer] unless the turn is compacting, which queues follow-ups instead. */
    canSteer: Boolean = detail.canSteer) {
    val scope = rememberCoroutineScope()
    var queueOpen by remember(id) { mutableStateOf(false) }
    var agentsOpen by remember(id) { mutableStateOf(false) }
    var editing by remember(id) { mutableStateOf<QueuedRun?>(null) }
    var editText by remember(id) { mutableStateOf("") }
    var busy by remember(id) { mutableStateOf(false) }
    // The edited run started or was cancelled elsewhere; a save in flight settles it instead.
    val editedRunQueued = editing?.let { run -> detail.queuedRuns.any { it.id == run.id } } ?: true
    LaunchedEffect(editing, editedRunQueued, busy) {
        val run = editing ?: return@LaunchedEffect
        if (editedRunQueued || busy) return@LaunchedEffect
        editing = null
        val draft = store.threadDraft(env.value, id.value)
        val exit = queuedEditExit(run.text, editText, draft.text.isBlank() && draft.attachments.isEmpty())
        if (exit == QueuedEditExit.Kept) store.setThreadDraft(env.value, id.value, editText)
        if (exit != QueuedEditExit.Clean) store.workspace.setThreadComposerError(
            "${env.value}:${id.value}",
            if (exit == QueuedEditExit.Kept) "That message already started. Your edit is back in the composer."
            else "That message already started, so the edit was discarded.",
        )
    }
    val queuedRunIds by rememberUpdatedState(detail.queuedRuns.map { it.id })
    fun action(block: suspend () -> Unit) {
        if (busy) return
        busy = true
        scope.launch {
            try { block() } catch (cancelled: kotlinx.coroutines.CancellationException) { throw cancelled }
            catch (error: Exception) {
                // A save that lost its run to a start is reported by the edit's exit.
                if (editing?.id?.let { it !in queuedRunIds } != true) store.showError(error.message ?: "Operation failed.")
            }
            finally { busy = false }
        }
    }
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (detail.queuedRuns.isNotEmpty() || detail.queueHeld) TextButton(onClick = { queueOpen = true }) {
                Text(if (detail.queueHeld) "Queue paused · ${detail.queuedRuns.size}" else "${detail.queuedRuns.size} queued")
            }
            if (detail.relationships.isNotEmpty()) TextButton(onClick = { agentsOpen = true }) { Text("Threads & agents") }
            if (working && canSteer && !detail.providerNativeSubagent) {
                TextButton(onClick = { onFollowUp(if (followUp == "queue") "steer" else "queue") }) {
                    Text("Follow-up: ${if (followUp == "queue") "Queue" else "Steer"}")
                }
            }
        }
        if (detail.usageLimitReached) {
            val resetAt = detail.usageLimitResetAt
            val runId = detail.latestTurn?.turnId
            Text(if (resetAt != null) "Usage limit reached · resets ${club.touchtech.s5code.kotlin.data.absoluteLabel(resetAt)}"
                else "Usage limit reached. The provider did not report a reset time; retry when the limit is available.", style = MaterialTheme.typography.bodySmall)
            if (resetAt != null && runId != null) {
            // Snooze stops making sense at the reset, which can pass without any redraw
            // (RN's `UsageLimitRecoveryCard` waits on the clock for the same reason).
            val resetAtMillis = club.touchtech.s5code.kotlin.data.parseInstant(resetAt) ?: 0
            var beforeReset by remember(resetAtMillis) { mutableStateOf(resetAtMillis > System.currentTimeMillis()) }
            LaunchedEffect(resetAtMillis) {
                val remaining = resetAtMillis - System.currentTimeMillis()
                if (remaining > 0) kotlinx.coroutines.delay(remaining)
                beforeReset = false
            }
            Row {
                TextButton(enabled = !busy, onClick = { action {
                    store.workspace.updateLimitRecovery(env, id, runId, resetAt, autoResume = !detail.limitRecoveryAutoResume)
                } }) { Text(if (detail.limitRecoveryAutoResume) "Disable auto-resume" else "Resume after reset") }
                TextButton(enabled = !busy && (detail.limitRecoverySnoozed || beforeReset),
                    onClick = { action { store.workspace.updateLimitRecovery(env, id, runId, resetAt, snooze = !detail.limitRecoverySnoozed) } }) {
                    Text(if (detail.limitRecoverySnoozed) "Wake now" else "Snooze until reset")
                }
            }
            }
        }
    }
    if (agentsOpen) AlertDialog(onDismissRequest = { agentsOpen = false }, title = { Text("Threads & agents") },
        text = { Column { detail.relationships.forEach { relation ->
            TextButton(onClick = { agentsOpen = false; onOpenThread(relation.threadId) }) { Text(relation.label) }
        } } }, confirmButton = { TextButton(onClick = { agentsOpen = false }) { Text("Close") } })
    if (queueOpen) AlertDialog(onDismissRequest = { queueOpen = false }, title = { Text("Queued messages") },
        text = { androidx.compose.foundation.lazy.LazyColumn {
            items(detail.queuedRuns.size, key = { detail.queuedRuns[it].id }) { index ->
                val run = detail.queuedRuns[index]
                Column {
                    // Edit and Steer keep the raw text so quoted citations stay linked.
                    val displayText = remember(run.text) {
                        club.touchtech.s5code.kotlin.data.assistantCitationsToPlainText(run.text).take(300)
                    }
                    Text(displayText, style = MaterialTheme.typography.bodyMedium)
                    Row {
                        TextButton(enabled = !busy, onClick = { editing = run; editText = run.text }) { Text("Edit") }
                        TextButton(enabled = !busy, onClick = { action { store.workspace.queueAction(env, id, "queued-run.cancel", run.id) } }) { Text("Cancel") }
                        if (index > 0 && detail.canReorderQueue) TextButton(enabled = !busy, onClick = { action {
                            store.workspace.queueAction(env, id, "queued-run.reorder", run.id, beforeRunId = detail.queuedRuns[index - 1].id)
                        } }) { Text("Move up") }
                        if (index < detail.queuedRuns.lastIndex && detail.canReorderQueue) TextButton(enabled = !busy, onClick = { action {
                            store.workspace.queueAction(env, id, "queued-run.reorder", run.id, beforeRunId = detail.queuedRuns.getOrNull(index + 2)?.id)
                        } }) { Text("Move down") }
                    }
                    // A stopped thread has nothing to steer, so the held message is sent on
                    // its own instead (RN's `canSendNow`).
                    if (detail.canSendNow) TextButton(enabled = !busy, onClick = { action {
                        store.workspace.queueAction(env, id, "queued-run.start", run.id)
                    } }) { Text("Send now") }
                    else if (detail.canPromoteQueued) TextButton(enabled = !busy, onClick = { action {
                        store.workspace.queueAction(env, id, "queued-message.promote-to-steer", run.id, targetRunId = detail.latestTurn?.turnId)
                    } }) { Text("Steer now") }
                    HorizontalDivider()
                }
            }
        } }, confirmButton = {
            Row {
                if (detail.queueHeld) TextButton(enabled = !busy, onClick = { action {
                    store.workspace.queueAction(env, id, "queue.resume")
                } }) { Text("Resume queue") }
                TextButton(onClick = { queueOpen = false }) { Text("Close") }
            }
        })
    editing?.let { run -> AlertDialog(onDismissRequest = { editing = null }, title = { Text("Edit queued message") },
        text = { OutlinedTextField(value = editText, onValueChange = { editText = it }, minLines = 3, readOnly = busy) },
        dismissButton = { TextButton(onClick = { editing = null }) { Text("Cancel") } },
        confirmButton = { TextButton(enabled = !busy && editText.isNotBlank(), onClick = { action {
            store.workspace.queueAction(env, id, "queued-run.edit", run.id, editText)
            editing = null
        } }) { Text("Save") } }) }
}

internal enum class QueuedEditExit { Clean, Kept, Discarded }

/**
 * RN `resolveQueuedEditExit`: an unchanged edit just closes; a changed one moves
 * into the thread's composer when that is empty, else it is dropped so it never
 * overwrites other work. A cleared edit has nothing to put back.
 */
internal fun queuedEditExit(originalText: String, editText: String, threadDraftEmpty: Boolean): QueuedEditExit =
    when {
        editText == originalText -> QueuedEditExit.Clean
        editText.isNotBlank() && threadDraftEmpty -> QueuedEditExit.Kept
        else -> QueuedEditExit.Discarded
    }
