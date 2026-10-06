package club.touchtech.s5code.kotlin.feature.settings

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.Commands
import club.touchtech.s5code.kotlin.data.providerInstanceForId
import club.touchtech.s5code.kotlin.data.providerOptionSelections
import club.touchtech.s5code.kotlin.data.relativeLabel
import club.touchtech.s5code.kotlin.data.rememberRetryableRemote
import club.touchtech.s5code.kotlin.data.Remote
import club.touchtech.s5code.kotlin.data.toRuntimeMode
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.rememberClipboardWriter
import club.touchtech.s5code.kotlin.model.*
import club.touchtech.s5code.kotlin.transport.v2String
import club.touchtech.s5code.kotlin.transport.v2Objects
import club.touchtech.s5code.kotlin.transport.v2Bool
import club.touchtech.s5code.kotlin.transport.v2Long
import kotlinx.coroutines.launch
import kotlinx.coroutines.CancellationException
import kotlinx.serialization.json.*

/** `MAX_WEBHOOK_DELIVERY_AGE_MINUTES` in the contracts — how long the relay holds a request. */
private const val MAX_WEBHOOK_DELIVERY_AGE_MINUTES = 24 * 60

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
    var deliveriesTask by remember(env?.id) { mutableStateOf<JsonObject?>(null) }
    var busy by remember { mutableStateOf(false) }
    var rotatingTask by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val copy = rememberClipboardWriter()
    LaunchedEffect(env?.id) {
        val target = env ?: return@LaunchedEffect
        try { store.workspace.environmentStream(target.id, "scheduledTasks.subscribe").collect {
            tasks = it.v2Objects("tasks"); loading = false
        } } catch (cancelled: CancellationException) { throw cancelled }
        catch (error: Exception) { failure = error.message; loading = false }
    }
    fun action(method: String, task: JsonObject, enabled: Boolean? = null) {
        val target = env ?: return
        if (busy) return
        busy = true
        scope.launch {
            try { store.workspace.environmentRequest(target.id, method, buildJsonObject {
                put("id", task.v2String("id")); enabled?.let { put("enabled", it) }
            }) } catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) { store.showError(error.message ?: "Could not update scheduled task.") }
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
            val schedule = task["schedule"] as? JsonObject
            val isWebhook = schedule?.v2String("type") == "webhook"
            val webhook = task["webhook"] as? JsonObject
            val address = webhook?.let { webhookAddress(it, webhookBaseUrl(env)) }
            Card { Column(Modifier.padding(16.dp)) {
                Row { Text(task.v2String("title").orEmpty(), modifier = Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
                    Switch(checked = task.v2Bool("enabled"), enabled = !busy,
                        onCheckedChange = { action("scheduledTasks.setEnabled", task, it) }) }
                Text(task.v2String("prompt").orEmpty().take(300))
                Text(scheduleLabel(schedule), style = MaterialTheme.typography.bodySmall)
                // A webhook task has no next run to show; "Listening" is its live state.
                Text(if (isWebhook) {
                        (if (task.v2Bool("enabled")) "Listening" else "Paused") +
                            " · ${task.v2String("lastRunStatus")}"
                    } else "Next: ${task.v2String("nextRunAt") ?: "Paused"} · ${task.v2String("lastRunStatus")}",
                    style = MaterialTheme.typography.bodySmall)
                task.v2String("lastRunError")?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                if (isWebhook && webhook != null && address != null) {
                    // The address a sender calls, plus who can reach it. A bare
                    // path is not callable, so the copy affordance stays off.
                    androidx.compose.foundation.text.selection.SelectionContainer {
                        Text(
                            if (address.copyable) "Webhook URL: ${address.address}" else "Webhook path: ${address.address}",
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                    address.note?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
                    (schedule?.get("signature") as? JsonObject)?.let { signature ->
                        val header = signature.v2String("header").orEmpty()
                        val prefix = signature.v2String("prefix").orEmpty()
                        val encoding = signature.v2String("encoding").orEmpty()
                        Text("Signature check: $header ($prefix$encoding)" +
                            if (webhook.v2Bool("hasSecret")) " · secret set" else "",
                            style = MaterialTheme.typography.bodySmall)
                        Text("Signature check configured on desktop/web.",
                            style = MaterialTheme.typography.bodySmall)
                    }
                    (schedule?.get("maxDeliveryAgeMinutes") as? JsonPrimitive)?.intOrNull?.let {
                        Text("Skip requests older than $it minutes", style = MaterialTheme.typography.bodySmall)
                    }
                }
                Row {
                    TextButton(enabled = !busy, onClick = { editing = task; editorOpen = true }) { Text("Edit") }
                    if (isWebhook) {
                        TextButton(enabled = !busy, onClick = { deliveriesTask = task }) { Text("Deliveries") }
                        TextButton(enabled = !busy && rotatingTask == null, onClick = {
                            val target = env ?: return@TextButton
                            val taskId = task.v2String("id") ?: return@TextButton
                            rotatingTask = taskId
                            scope.launch {
                                try { store.workspace.rotateScheduledTaskWebhookToken(target.id, taskId) }
                                catch (cancelled: CancellationException) { throw cancelled }
                                catch (error: Exception) { store.showError(error.message ?: "Could not rotate URL.") }
                                finally { rotatingTask = null }
                            }
                        }) { Text(if (rotatingTask == task.v2String("id")) "Rotating…" else "Rotate URL") }
                    } else {
                        TextButton(enabled = !busy, onClick = { action("scheduledTasks.runNow", task) }) { Text("Run now") }
                    }
                    TextButton(enabled = !busy, onClick = { action("scheduledTasks.delete", task) }) { Text("Delete") }
                }
            } }
        }
    } }
    deliveriesTask?.v2String("id")?.let { taskId ->
        if (env != null) WebhookDeliveriesDialog(store, env.id, taskId) { deliveriesTask = null }
    }
    if (editorOpen && env != null) ScheduledTaskEditor(store, env, editing,
        projects.filter { it.environmentId == env.id }, catalogs[env.id].orEmpty(),
        copy = copy,
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

/** `describeSchedule` in the RN client; a type this build does not know still gets a row. */
private fun scheduleLabel(schedule: JsonObject?): String =
    when (schedule?.v2String("type")) {
        "interval" -> "Every ${(schedule.v2Long("everyMs") ?: 0) / 60000} minutes"
        "webhook" -> "On webhook"
        else ->
            "At ${schedule?.v2String("timeOfDay")} · " +
                ((schedule?.get("weekdays") as? JsonArray)?.joinToString() ?: "every day")
    }

/** Resolved webhook address, following `webhookAddress` in the client-runtime package. */
private data class WebhookAddress(
    val address: String,
    /** Whether [address] is a full URL a sender can call. */
    val copyable: Boolean,
    /** One line on who can reach [address]; null for a T3 Connect URL. */
    val note: String?,
)

/**
 * The origin a webhook path resolves against. Direct environments keep their
 * saved `httpBaseUrl` in [Environment.baseUrl]; relay-managed rows blank it,
 * so `https://` + the reported host stands in (relay endpoints are always TLS).
 */
private fun webhookBaseUrl(env: Environment?): String? =
    env?.baseUrl?.takeIf { it.isNotBlank() }
        ?: env?.host?.takeIf { it.isNotBlank() }?.let { "https://$it" }

private fun isLocalLoopbackHost(host: String): Boolean =
    host == "localhost" || host == "::1" || host.startsWith("127.")

private fun webhookAddress(webhook: JsonObject, httpBaseUrl: String?): WebhookAddress {
    val path = webhook.v2String("path").orEmpty()
    webhook.v2String("url")?.let { return WebhookAddress(it, copyable = true, note = null) }
    if (httpBaseUrl == null) {
        return WebhookAddress(
            address = path,
            copyable = false,
            note = "Link this environment to T3 Connect for a public URL.",
        )
    }
    val url = runCatching { java.net.URI(httpBaseUrl.trimEnd('/') + "/" + path.trimStart('/')) }
        .getOrNull() ?: return WebhookAddress(path, copyable = false, note = null)
    return WebhookAddress(
        address = url.toString(),
        copyable = true,
        note =
            if (isLocalLoopbackHost(url.host.orEmpty())) {
                "Only this computer can call this address. Link T3 Connect for a public URL."
            } else {
                "Works wherever this environment's address is reachable, for example over Tailscale or your own proxy. Link T3 Connect for a public URL."
            },
    )
}

/** `DELIVERY_OUTCOME_LABELS` in the web settings; an unknown outcome keeps its raw name. */
private fun deliveryOutcomeLabel(outcome: String): String =
    when (outcome) {
        "accepted" -> "Ran"
        "dispatch_failed" -> "Run failed"
        "rejected_signature" -> "Bad signature"
        "disabled" -> "Task paused"
        "rate_limited" -> "Rate limited"
        "expired" -> "Too old"
        else -> outcome
    }

/** Recent webhook deliveries for one task — web's `WebhookDeliveriesDialog`. */
@Composable
private fun WebhookDeliveriesDialog(
    store: AppStore,
    env: EnvironmentId,
    taskId: String,
    onClose: () -> Unit,
) {
    val (deliveries, reloadDeliveries) = rememberRetryableRemote(env.value, taskId) {
        store.workspace.listScheduledTaskWebhookDeliveries(env, taskId)
    }
    var selectedId by remember(taskId) { mutableStateOf<String?>(null) }
    val (detail, _) = rememberRetryableRemote(env.value, taskId, selectedId) {
        selectedId?.let { store.workspace.scheduledTaskWebhookDelivery(env, taskId, it) }
    }
    val detailValue = detail.value.valueOrNull
    AlertDialog(
        onDismissRequest = onClose,
        title = { Text("Deliveries") },
        text = {
            LazyColumn(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                if (selectedId != null) {
                    when (val remote = detail.value) {
                        is Remote.Loading -> item { Text("Loading delivery…") }
                        is Remote.Failed ->
                            item { Text(remote.message, color = MaterialTheme.colorScheme.error) }
                        is Remote.Loaded -> {
                            val delivery = remote.value
                            if (delivery == null) {
                                item { Text("That delivery is no longer listed.") }
                            } else {
                                item {
                                    Text(
                                        "${deliveryOutcomeLabel(delivery.outcome)} · ${delivery.method} · " +
                                            relativeLabel(delivery.receivedAt, System.currentTimeMillis()) +
                                            (if (delivery.signatureVerified) " · Signature verified" else ""),
                                        style = MaterialTheme.typography.bodyMedium,
                                    )
                                }
                                delivery.error?.let {
                                    item { Text(it, color = MaterialTheme.colorScheme.error) }
                                }
                                item {
                                    Text(
                                        "Prompt sent to the agent",
                                        style = MaterialTheme.typography.labelMedium,
                                    )
                                    androidx.compose.foundation.text.selection.SelectionContainer {
                                        Text(
                                            delivery.renderedPrompt ?: "No run was started for this request.",
                                            style = MaterialTheme.typography.bodySmall,
                                        )
                                    }
                                }
                                if (delivery.missingFields.isNotEmpty()) {
                                    item {
                                        Text(
                                            "Empty placeholders: ${delivery.missingFields.joinToString(", ")}",
                                            style = MaterialTheme.typography.bodySmall,
                                        )
                                    }
                                }
                                if (delivery.headers.isNotEmpty()) {
                                    item {
                                        Text("Headers", style = MaterialTheme.typography.labelMedium)
                                        androidx.compose.foundation.text.selection.SelectionContainer {
                                            Text(
                                                delivery.headers.entries.joinToString("\n") {
                                                    "${it.key}: ${it.value}"
                                                },
                                                style = MaterialTheme.typography.bodySmall,
                                            )
                                        }
                                    }
                                }
                                if (delivery.query.isNotEmpty()) {
                                    item {
                                        Text("Query", style = MaterialTheme.typography.labelMedium)
                                        androidx.compose.foundation.text.selection.SelectionContainer {
                                            Text(delivery.query, style = MaterialTheme.typography.bodySmall)
                                        }
                                    }
                                }
                                item {
                                    Text(
                                        if (delivery.bodyTruncated) "Body (truncated)" else "Body",
                                        style = MaterialTheme.typography.labelMedium,
                                    )
                                    androidx.compose.foundation.text.selection.SelectionContainer {
                                        Text(
                                            delivery.body.ifEmpty { "(empty)" },
                                            style = MaterialTheme.typography.bodySmall,
                                        )
                                    }
                                }
                            }
                        }
                    }
                } else {
                    when (val remote = deliveries.value) {
                        is Remote.Loading -> item { Text("Loading deliveries…") }
                        is Remote.Failed ->
                            item { Text(remote.message, color = MaterialTheme.colorScheme.error) }
                        is Remote.Loaded -> {
                            val rows = remote.value
                            if (rows.isEmpty()) {
                                item { Text("No requests yet.") }
                            } else {
                                items(rows, key = { it.id }) { delivery ->
                                    TextButton(
                                        onClick = { selectedId = delivery.id },
                                        modifier = Modifier.fillMaxWidth(),
                                    ) {
                                        Text(
                                            "${deliveryOutcomeLabel(delivery.outcome)} · ${delivery.method} · " +
                                                relativeLabel(delivery.receivedAt, System.currentTimeMillis()) +
                                                if (delivery.missingFields.isNotEmpty()) {
                                                    " · ${delivery.missingFields.size} empty placeholder" +
                                                        (if (delivery.missingFields.size == 1) "" else "s")
                                                } else "",
                                            modifier = Modifier.weight(1f),
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
        dismissButton = {
            if (selectedId != null) {
                TextButton(onClick = { selectedId = null }) { Text("Back") }
            } else {
                TextButton(onClick = reloadDeliveries) { Text("Refresh") }
            }
        },
        confirmButton = { TextButton(onClick = onClose) { Text("Done") } },
    )
}

@Composable
private fun ScheduledTaskEditor(store: AppStore, env: Environment, existing: JsonObject?, projects: List<Project>,
    catalog: List<ProviderCatalogEntry>, copy: (String) -> Unit, onClose: () -> Unit) {
    var title by remember { mutableStateOf(existing?.v2String("title").orEmpty()) }
    var prompt by remember { mutableStateOf(existing?.v2String("prompt").orEmpty()) }
    val schedule = existing?.get("schedule") as? JsonObject
    val isWebhook = schedule?.v2String("type") == "webhook"
    var daily by remember { mutableStateOf(schedule?.v2String("type") == "fixed_time") }
    var time by remember { mutableStateOf(schedule?.v2String("timeOfDay") ?: "09:00") }
    var minutes by remember { mutableStateOf(((schedule?.v2Long("everyMs") ?: 3600000) / 60000).toString()) }
    // "Skip requests older than (minutes)" — blank runs every request, matching
    // `parseMaxDeliveryAge`: 1..1440 whole minutes or the write is refused.
    var maxAge by remember {
        mutableStateOf(
            (schedule?.get("maxDeliveryAgeMinutes") as? JsonPrimitive)?.intOrNull?.toString()
                .orEmpty(),
        )
    }
    var weekdays by remember { mutableStateOf((schedule?.get("weekdays") as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.intOrNull }?.toSet() ?: (0..6).toSet()) }
    var projectId by remember { mutableStateOf(existing?.v2String("projectId") ?: projects.firstOrNull()?.id?.value.orEmpty()) }
    var projectMenu by remember { mutableStateOf(false) }
    var bound by remember { mutableStateOf(existing?.v2String("threadId").orEmpty()) }
    val originalWorkspace = existing?.get("workspaceStrategy") as? JsonObject
    var workspace by remember { mutableStateOf(originalWorkspace?.v2String("type") ?: "root") }
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
        (if (isWebhook) {
            maxAge.trim().isEmpty() ||
                (maxAge.trim().toIntOrNull() ?: 0) in 1..MAX_WEBHOOK_DELIVERY_AGE_MINUTES
        } else if (daily) {
            Regex("^([01]?\\d|2[0-3]):[0-5]\\d$").matches(time) && weekdays.isNotEmpty()
        } else (minutes.toLongOrNull() ?: 0) in 1..525600) &&
        (workspace != "existing_worktree" || path.isNotBlank()) && (workspace != "worktree" || ref.isNotBlank())
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
            // Webhook schedules are created on desktop/web; this editor keeps the
            // trigger read-only (signature included, sent back unchanged on save)
            // and only exposes the delivery-age limit it writes.
            if (isWebhook) {
                item { Text(scheduleLabel(schedule)) }
                item {
                    Text(
                        "The prompt can use {{body.a.b}}, {{headers.name}}, {{query.name}}, {{body}} and {{request}}. The filled-in prompt is all the agent sees.",
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                item {
                    val webhook = existing?.get("webhook") as? JsonObject
                    val address = webhook?.let { webhookAddress(it, webhookBaseUrl(env)) }
                    Text(
                        when {
                            webhook == null || address == null -> "Save the task to get its webhook URL."
                            else -> if (address.copyable) "Webhook URL: ${address.address}" else "Webhook path: ${address.address}"
                        },
                    )
                    if (address?.copyable == true) {
                        TextButton(onClick = { copy(address.address) }) { Text("Copy") }
                    }
                    address?.note?.let { Text(it) }
                    (schedule?.get("signature") as? JsonObject)?.let {
                        Text(
                            "Signature check configured on desktop/web.",
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                }
                item {
                    OutlinedTextField(
                        value = maxAge,
                        onValueChange = { maxAge = it },
                        label = { Text("Skip requests older than (minutes)") },
                        placeholder = { Text("Run every request") },
                        singleLine = true,
                    )
                }
            } else {
                item { TextButton(onClick = { daily = !daily }) { Text(if (daily) "Fixed local time" else "Interval") } }
                item { if (daily) OutlinedTextField(time, { time = it }, label = { Text("Time (HH:MM)") })
                    else OutlinedTextField(minutes, { minutes = it }, label = { Text("Interval in minutes") }) }
                if (daily) item { Column { listOf("Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday").forEachIndexed { day, label ->
                    Row { Checkbox(day in weekdays, { weekdays = if (it) weekdays + day else weekdays - day }); Text(label) }
                } } }
            }
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
                    if (isWebhook) {
                        // The live schedule's signature rides back unchanged —
                        // "omit to keep the stored secret" — and this client edits
                        // nothing else on the trigger but the age limit.
                        put("schedule", JsonObject((schedule ?: JsonObject(emptyMap())).toMutableMap().apply {
                            put(
                                "maxDeliveryAgeMinutes",
                                maxAge.trim().toIntOrNull()?.let(::JsonPrimitive) ?: JsonNull,
                            )
                        }))
                    } else {
                        putJsonObject("schedule") {
                            put("type", if (daily) "fixed_time" else "interval")
                            if (daily) { put("timeOfDay", time); putJsonArray("weekdays") { weekdays.sorted().forEach { add(it) } } }
                            else put("everyMs", minutes.toLong() * 60000)
                        }
                    }
                    putJsonObject("workspaceStrategy") {
                        put("type", workspace)
                        if (workspace == "worktree") put("baseRef", ref)
                        if (workspace == "existing_worktree") put("worktreePath", path)
                        if (originalWorkspace?.v2String("type") == workspace) {
                            originalWorkspace["branch"]?.let { put("branch", it) }
                            if (workspace == "worktree") originalWorkspace["startFromOrigin"]?.let { put("startFromOrigin", it) }
                        }
                    }
                    put("modelSelection", Commands.updateMeta("", instanceId = settings.provider.instanceId, model = settings.model, options = settings.options).getValue("modelSelection"))
                    put("runtimeMode", settings.approvalPolicy.toRuntimeMode())
                    put("interactionMode", if (settings.runtimeMode == RuntimeMode.Plan) "plan" else "default")
                }
                store.workspace.environmentRequest(env.id, "scheduledTasks.upsert", payload)
                onClose()
            } catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) { failure = error.message ?: "Could not save scheduled task." }
            finally { busy = false }
        } }) { Text("Save") } })
    if (modelOpen) TaskSettingsSheet(settings = settings, onDismiss = { modelOpen = false },
        onSettingsChange = { settings = it }, catalog = catalog, modelsFor = { provider ->
            catalog.firstOrNull { it.instance.instanceId == provider.instanceId }?.models.orEmpty()
        })
}
