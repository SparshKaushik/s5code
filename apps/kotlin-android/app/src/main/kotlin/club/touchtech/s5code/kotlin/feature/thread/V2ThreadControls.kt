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
    working: Boolean, followUp: String, onFollowUp: (String) -> Unit, onOpenThread: (String) -> Unit) {
    val scope = rememberCoroutineScope()
    var queueOpen by remember(id) { mutableStateOf(false) }
    var agentsOpen by remember(id) { mutableStateOf(false) }
    var editing by remember(id) { mutableStateOf<QueuedRun?>(null) }
    var editText by remember(id) { mutableStateOf("") }
    var busy by remember(id) { mutableStateOf(false) }
    fun action(block: suspend () -> Unit) {
        if (busy) return
        busy = true
        scope.launch {
            try { block() } catch (cancelled: kotlinx.coroutines.CancellationException) { throw cancelled }
            catch (error: Exception) { store.showError(error.message ?: "Operation failed.") }
            finally { busy = false }
        }
    }
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (detail.queuedRuns.isNotEmpty() || detail.queueHeld) TextButton(onClick = { queueOpen = true }) {
                Text(if (detail.queueHeld) "Queue paused · ${detail.queuedRuns.size}" else "${detail.queuedRuns.size} queued")
            }
            if (detail.relationships.isNotEmpty()) TextButton(onClick = { agentsOpen = true }) { Text("Threads & agents") }
            if (working && detail.canSteer && !detail.providerNativeSubagent) {
                TextButton(onClick = { onFollowUp(if (followUp == "queue") "steer" else "queue") }) {
                    Text("Follow-up: ${if (followUp == "queue") "Queue" else "Steer"}")
                }
            }
        }
        if (detail.summary.status == ThreadStatus.Waiting && !detail.providerNativeSubagent) {
            TextButton(enabled = !busy, onClick = { action { store.workspace.cancelTurn(env, id) } }) { Text("Stop background work") }
        }
        if (detail.usageLimitReached) {
            val resetAt = detail.usageLimitResetAt
            val runId = detail.latestTurn?.turnId
            Text(if (resetAt != null) "Usage limit reached · resets ${club.touchtech.s5code.kotlin.data.absoluteLabel(resetAt)}"
                else "Usage limit reached. The provider did not report a reset time; retry when the limit is available.", style = MaterialTheme.typography.bodySmall)
            if (resetAt != null && runId != null) {
            Row {
                TextButton(enabled = !busy, onClick = { action {
                    store.workspace.updateLimitRecovery(env, id, runId, resetAt, autoResume = !detail.limitRecoveryAutoResume)
                } }) { Text(if (detail.limitRecoveryAutoResume) "Disable auto-resume" else "Resume after reset") }
                TextButton(enabled = !busy && (detail.limitRecoverySnoozed ||
                    (club.touchtech.s5code.kotlin.data.parseInstant(resetAt) ?: 0) > System.currentTimeMillis()),
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
                    Text(run.text.take(300), style = MaterialTheme.typography.bodyMedium)
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
                    if (detail.canPromoteQueued) TextButton(enabled = !busy, onClick = { action {
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
        text = { OutlinedTextField(value = editText, onValueChange = { editText = it }, minLines = 3) },
        dismissButton = { TextButton(onClick = { editing = null }) { Text("Cancel") } },
        confirmButton = { TextButton(enabled = !busy && editText.isNotBlank(), onClick = { action {
            store.workspace.queueAction(env, id, "queued-run.edit", run.id, editText)
            editing = null
        } }) { Text("Save") } }) }
}
