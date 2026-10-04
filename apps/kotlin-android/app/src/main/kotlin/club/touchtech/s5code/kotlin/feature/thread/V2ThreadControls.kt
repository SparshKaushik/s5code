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
        scope.launch {
            busy = true
            try { block() } catch (error: Exception) { store.showError(error.message ?: "Operation failed.") }
            finally { busy = false }
        }
    }
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (detail.queuedRuns.isNotEmpty()) TextButton(onClick = { queueOpen = true }) {
                Text("${detail.queuedRuns.size} queued")
            }
            if (detail.relationships.isNotEmpty()) TextButton(onClick = { agentsOpen = true }) { Text("Threads & agents") }
            if (working && detail.canSteer && !detail.providerNativeSubagent) {
                TextButton(onClick = { onFollowUp(if (followUp == "queue") "steer" else "queue") }) {
                    Text("Follow-up: ${if (followUp == "queue") "Queue" else "Steer"}")
                }
            }
        }
        detail.usageLimitResetAt?.let { resetAt ->
            Text("Usage limit reached · resets $resetAt", style = MaterialTheme.typography.bodySmall)
            Row {
                TextButton(enabled = !busy, onClick = { action {
                    store.workspace.environmentRequest(env, "orchestration.dispatchCommand", buildJsonObject {
                        put("type", "thread.metadata.update")
                        put("commandId", java.util.UUID.randomUUID().toString())
                        put("threadId", id.value)
                        putJsonObject("limitRecovery") {
                            put("runId", detail.latestTurn?.turnId)
                            put("resetAt", resetAt)
                            put("autoResume", !detail.limitRecoveryAutoResume)
                        }
                    })
                } }) { Text(if (detail.limitRecoveryAutoResume) "Disable auto-resume" else "Resume after reset") }
                TextButton(enabled = !busy, onClick = { action { store.workspace.setSnoozed(env, id, true, resetAt) } }) { Text("Snooze until reset") }
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
                        if (index > 0) TextButton(enabled = !busy, onClick = { action {
                            store.workspace.queueAction(env, id, "queued-run.reorder", run.id, beforeRunId = detail.queuedRuns[index - 1].id)
                        } }) { Text("Move up") }
                    }
                    if (working && detail.canSteer) TextButton(enabled = !busy, onClick = { action {
                        store.workspace.queueAction(env, id, "queued-message.promote-to-steer", run.id, targetRunId = detail.latestTurn?.turnId)
                    } }) { Text("Steer now") }
                    HorizontalDivider()
                }
            }
        } }, confirmButton = {
            Row {
                if (detail.queuedRuns.any { it.held }) TextButton(enabled = !busy, onClick = { action {
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
