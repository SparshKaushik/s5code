package club.touchtech.s5code.kotlin.feature.settings

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.Commands
import club.touchtech.s5code.kotlin.data.providerInstanceForId
import club.touchtech.s5code.kotlin.data.providerOptionSelections
import club.touchtech.s5code.kotlin.data.toRuntimeMode
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.model.*
import club.touchtech.s5code.kotlin.transport.v2String
import club.touchtech.s5code.kotlin.transport.v2Objects
import club.touchtech.s5code.kotlin.transport.v2Bool
import club.touchtech.s5code.kotlin.transport.v2Long
import kotlinx.coroutines.launch
import kotlinx.coroutines.CancellationException
import kotlinx.serialization.json.*

@Composable
fun ScheduledTasksScreen(store: AppStore, onBack: () -> Unit) {
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val catalogs by store.workspace.providerCatalogs.collectAsStateWithLifecycle()
    val targets = environments.filter { it.state == ConnectionState.Connected }
    var selected by remember { mutableStateOf<EnvironmentId?>(null) }
    val env = targets.firstOrNull { it.id == selected } ?: targets.firstOrNull()
    var tasks by remember(env?.id) { mutableStateOf(emptyList<JsonObject>()) }
    var loading by remember(env?.id) { mutableStateOf(true) }
    var failure by remember(env?.id) { mutableStateOf<String?>(null) }
    var editing by remember(env?.id) { mutableStateOf<JsonObject?>(null) }
    var editorOpen by remember(env?.id) { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(env?.id) {
        val target = env ?: return@LaunchedEffect
        try { store.workspace.environmentStream(target.id, "scheduledTasks.subscribe").collect {
            tasks = it.v2Objects("tasks"); loading = false
        } } catch (cancelled: CancellationException) { throw cancelled }
        catch (error: Exception) { failure = error.message; loading = false }
    }
    fun action(method: String, task: JsonObject, enabled: Boolean? = null) {
        val target = env ?: return
        scope.launch {
            busy = true
            try { store.workspace.environmentRequest(target.id, method, buildJsonObject {
                put("id", task.v2String("id")); enabled?.let { put("enabled", it) }
            }) } catch (error: Exception) { store.showError(error.message ?: "Could not update scheduled task.") }
            finally { busy = false }
        }
    }
    S5Screen(title = "Scheduled tasks", onBack = onBack, actions = {
        TextButton(enabled = env != null, onClick = { editing = null; editorOpen = true }) { Text("New task") }
    }) { padding -> LazyColumn(Modifier.fillMaxSize().padding(padding), contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item { EnvironmentChoices(targets, env?.id) { selected = it } }
        if (env == null) item { Text("Connect an environment to manage scheduled tasks.") }
        else if (loading) item { LinearProgressIndicator(Modifier.fillMaxWidth()) }
        failure?.let { item { Text(it, color = MaterialTheme.colorScheme.error) } }
        if (!loading && tasks.isEmpty() && env != null) item { Text("No scheduled tasks on this environment.") }
        items(tasks, key = { it.v2String("id")!! }) { task ->
            Card { Column(Modifier.padding(16.dp)) {
                Row { Text(task.v2String("title").orEmpty(), modifier = Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
                    Switch(checked = task.v2Bool("enabled"), enabled = !busy,
                        onCheckedChange = { action("scheduledTasks.setEnabled", task, it) }) }
                Text(task.v2String("prompt").orEmpty().take(300))
                val schedule = task["schedule"] as? JsonObject
                Text(if (schedule?.v2String("type") == "interval") "Every ${(schedule.v2Long("everyMs") ?: 0) / 60000} minutes"
                    else "At ${schedule?.v2String("timeOfDay")} · ${(schedule?.get("weekdays") as? JsonArray)?.joinToString() ?: "every day"}",
                    style = MaterialTheme.typography.bodySmall)
                Text("Next: ${task.v2String("nextRunAt") ?: "Paused"} · ${task.v2String("lastRunStatus")}", style = MaterialTheme.typography.bodySmall)
                task.v2String("lastRunError")?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                Row {
                    TextButton(enabled = !busy, onClick = { editing = task; editorOpen = true }) { Text("Edit") }
                    TextButton(enabled = !busy, onClick = { action("scheduledTasks.runNow", task) }) { Text("Run now") }
                    TextButton(enabled = !busy, onClick = { action("scheduledTasks.delete", task) }) { Text("Delete") }
                }
            } }
        }
    } }
    if (editorOpen && env != null) ScheduledTaskEditor(store, env.id, editing,
        projects.filter { it.environmentId == env.id }, catalogs[env.id].orEmpty(),
        onClose = { editorOpen = false })
}

@Composable
internal fun EnvironmentChoices(targets: List<Environment>, selected: EnvironmentId?, onSelect: (EnvironmentId) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box { TextButton(onClick = { open = true }) { Text(targets.firstOrNull { it.id == selected }?.label ?: "Environment") }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            targets.forEach { env -> DropdownMenuItem(text = { Text(env.label) }, onClick = { onSelect(env.id); open = false }) }
        }
    }
}

@Composable
private fun ScheduledTaskEditor(store: AppStore, env: EnvironmentId, existing: JsonObject?, projects: List<Project>,
    catalog: List<ProviderCatalogEntry>, onClose: () -> Unit) {
    var title by remember { mutableStateOf(existing?.v2String("title").orEmpty()) }
    var prompt by remember { mutableStateOf(existing?.v2String("prompt").orEmpty()) }
    val schedule = existing?.get("schedule") as? JsonObject
    var daily by remember { mutableStateOf(schedule?.v2String("type") == "fixed_time") }
    var time by remember { mutableStateOf(schedule?.v2String("timeOfDay") ?: "09:00") }
    var minutes by remember { mutableStateOf(((schedule?.v2Long("everyMs") ?: 3600000) / 60000).toString()) }
    var weekdays by remember { mutableStateOf((schedule?.get("weekdays") as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.intOrNull }?.toSet() ?: (0..6).toSet()) }
    var projectId by remember { mutableStateOf(existing?.v2String("projectId") ?: projects.firstOrNull()?.id?.value.orEmpty()) }
    var projectMenu by remember { mutableStateOf(false) }
    var bound by remember { mutableStateOf(existing?.v2String("threadId").orEmpty()) }
    var workspace by remember { mutableStateOf((existing?.get("workspaceStrategy") as? JsonObject)?.v2String("type") ?: "root") }
    var ref by remember { mutableStateOf((existing?.get("workspaceStrategy") as? JsonObject)?.v2String("baseRef") ?: "HEAD") }
    var path by remember { mutableStateOf((existing?.get("workspaceStrategy") as? JsonObject)?.v2String("worktreePath").orEmpty()) }
    val selection = existing?.get("modelSelection") as? JsonObject
    var settings by remember { mutableStateOf(ThreadSettings(
        provider = selection?.v2String("instanceId")?.let(::providerInstanceForId) ?: catalog.firstOrNull()?.instance ?: ProviderInstance.Default,
        model = selection?.v2String("model") ?: catalog.firstOrNull()?.models?.firstOrNull().orEmpty(),
        options = providerOptionSelections(selection?.get("options")),
        runtimeMode = if (existing?.v2String("interactionMode") == "plan") RuntimeMode.Plan else RuntimeMode.Default,
        approvalPolicy = when (existing?.v2String("runtimeMode")) { "approval-required" -> ApprovalPolicy.Ask; "auto-accept-edits" -> ApprovalPolicy.AutoEdit; else -> ApprovalPolicy.Full },
    )) }
    var modelOpen by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val valid = title.isNotBlank() && prompt.isNotBlank() && projectId.isNotBlank() && settings.model.isNotBlank() &&
        (if (daily) Regex("^([01]?\\d|2[0-3]):[0-5]\\d$").matches(time) && weekdays.isNotEmpty() else (minutes.toLongOrNull() ?: 0) in 1..525600) &&
        (workspace != "existing_worktree" || path.isNotBlank())
    AlertDialog(onDismissRequest = onClose, title = { Text(if (existing == null) "New scheduled task" else "Edit scheduled task") },
        text = { LazyColumn(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            item { OutlinedTextField(title, { title = it }, label = { Text("Title") }) }
            item { OutlinedTextField(prompt, { prompt = it }, label = { Text("Prompt") }, minLines = 3) }
            item { Box {
                TextButton(onClick = { projectMenu = true }) { Text(projects.firstOrNull { it.id.value == projectId }?.title ?: "Project") }
                DropdownMenu(projectMenu, { projectMenu = false }) { projects.forEach { project -> DropdownMenuItem(
                    text = { Text(project.title) }, onClick = { projectId = project.id.value; bound = ""; projectMenu = false }) } }
            } }
            item { OutlinedTextField(bound, { bound = it }, label = { Text("Existing thread ID (optional)") }) }
            item { TextButton(onClick = { daily = !daily }) { Text(if (daily) "Fixed local time" else "Interval") } }
            item { if (daily) OutlinedTextField(time, { time = it }, label = { Text("Time (HH:MM)") })
                else OutlinedTextField(minutes, { minutes = it }, label = { Text("Interval in minutes") }) }
            if (daily) item { Column { listOf("Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday").forEachIndexed { day, label ->
                Row { Checkbox(day in weekdays, { weekdays = if (it) weekdays + day else weekdays - day }); Text(label) }
            } } }
            item { Row { listOf("root", "worktree", "existing_worktree").forEach { choice ->
                TextButton(onClick = { workspace = choice }) { Text(when (choice) { "root" -> "Local"; "worktree" -> "New worktree"; else -> "Existing" }) }
            } } }
            if (workspace == "worktree") item { OutlinedTextField(ref, { ref = it }, label = { Text("Base branch") }) }
            if (workspace == "existing_worktree") item { OutlinedTextField(path, { path = it }, label = { Text("Worktree path") }) }
            item { TextButton(onClick = { modelOpen = true }) { Text("${settings.provider.label} · ${settings.model}") } }
            failure?.let { item { Text(it, color = MaterialTheme.colorScheme.error) } }
        } }, dismissButton = { TextButton(onClick = onClose) { Text("Cancel") } },
        confirmButton = { TextButton(enabled = valid && !busy, onClick = { scope.launch {
            busy = true
            try {
                val payload = buildJsonObject {
                    existing?.v2String("id")?.let { put("id", it); put("requireExisting", true) }
                    put("commandId", java.util.UUID.randomUUID().toString())
                    put("title", title.trim()); put("prompt", prompt.trim())
                    put("enabled", existing?.v2Bool("enabled") ?: true)
                    put("projectId", projectId); put("threadId", bound.takeIf { it.isNotBlank() })
                    put("createdBy", "user"); put("creationSource", "mobile")
                    putJsonObject("schedule") {
                        put("type", if (daily) "fixed_time" else "interval")
                        if (daily) { put("timeOfDay", time); putJsonArray("weekdays") { weekdays.sorted().forEach { add(it) } } }
                        else put("everyMs", minutes.toLong() * 60000)
                    }
                    putJsonObject("workspaceStrategy") {
                        put("type", workspace)
                        if (workspace == "worktree") put("baseRef", ref)
                        if (workspace == "existing_worktree") put("worktreePath", path)
                    }
                    put("modelSelection", Commands.updateMeta("", instanceId = settings.provider.instanceId, model = settings.model, options = settings.options).getValue("modelSelection"))
                    put("runtimeMode", settings.approvalPolicy.toRuntimeMode())
                    put("interactionMode", if (settings.runtimeMode == RuntimeMode.Plan) "plan" else "default")
                }
                store.workspace.environmentRequest(env, "scheduledTasks.upsert", payload)
                onClose()
            } catch (error: Exception) { failure = error.message ?: "Could not save scheduled task." }
            finally { busy = false }
        } }) { Text("Save") } })
    if (modelOpen) TaskSettingsSheet(settings = settings, onDismiss = { modelOpen = false },
        onSettingsChange = { settings = it }, catalog = catalog, modelsFor = { provider ->
            catalog.firstOrNull { it.instance.instanceId == provider.instanceId }?.models.orEmpty()
        })
}
