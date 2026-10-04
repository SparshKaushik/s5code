package club.touchtech.s5code.kotlin.feature.settings

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.model.*
import club.touchtech.s5code.kotlin.transport.v2String
import club.touchtech.s5code.kotlin.transport.v2Bool
import club.touchtech.s5code.kotlin.transport.v2Objects
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import kotlinx.serialization.json.*

@Composable
fun ProviderAccountsScreen(store: AppStore, onBack: () -> Unit) {
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val targets = environments.filter { it.state == ConnectionState.Connected }
    var selected by remember { mutableStateOf<EnvironmentId?>(null) }
    val env = targets.firstOrNull { it.id == selected } ?: targets.firstOrNull()
    var providers by remember(env?.id) { mutableStateOf(emptyList<JsonObject>()) }
    var failure by remember(env?.id) { mutableStateOf<String?>(null) }
    LaunchedEffect(env?.id) {
        val target = env ?: return@LaunchedEffect
        try { store.workspace.environmentStream(target.id, "subscribeServerConfig", buildJsonObject {
            put("usageLimitSources", true)
        }).collect { frame ->
            val snapshot = frame["config"] as? JsonObject
            val payload = frame["payload"] as? JsonObject
            (snapshot ?: payload)?.let { data -> if (data["providers"] is JsonArray) {
                providers = data.v2Objects("providers").filter { (it["setup"] as? JsonObject)?.v2Bool("canAuthenticate") == true || it.v2String("driver") == "acpRegistry" }
            } }
        } } catch (cancelled: CancellationException) { throw cancelled }
        catch (error: Exception) { failure = error.message }
    }
    S5Screen(title = "Provider accounts", onBack = onBack) { padding ->
        LazyColumn(Modifier.fillMaxSize().padding(padding), contentPadding = PaddingValues(16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            item { EnvironmentChoices(targets, env?.id) { selected = it } }
            if (env == null) item { Text("Select a connected environment.") }
            failure?.let { item { Text(it, color = MaterialTheme.colorScheme.error) } }
            if (env != null && providers.isEmpty()) item { Text("Configure a provider with in-app sign-in in web or desktop Settings.") }
            providers.forEach { provider -> item(key = provider.v2String("instanceId")) {
                ProviderAccountCard(store, env!!.id, provider)
            } }
        }
    }
}

@Composable
private fun ProviderAccountCard(store: AppStore, env: EnvironmentId, provider: JsonObject) {
    val instanceId = provider.v2String("instanceId")!!
    val uriHandler = LocalUriHandler.current
    val scope = rememberCoroutineScope()
    var state by remember(env, instanceId) { mutableStateOf<JsonObject?>(null) }
    var failure by remember(env, instanceId) { mutableStateOf<String?>(null) }
    var busy by remember(env, instanceId) { mutableStateOf(false) }
    var methodMenu by remember { mutableStateOf(false) }
    var logoutConfirm by remember { mutableStateOf(false) }
    var emailVisible by remember { mutableStateOf(false) }
    val interaction = state?.get("interaction") as? JsonObject
    val flowId = state?.v2String("flowId")
    var values by remember(flowId, interaction?.v2String("id")) { mutableStateOf(emptyMap<String, String>()) }
    val active = state?.v2String("phase") in setOf("starting", "waiting", "verifying")
    val auth = provider["auth"] as? JsonObject
    val signedIn = auth?.v2String("status") == "authenticated" ||
        (auth?.v2String("status") == "unknown" && state?.v2String("phase") == "succeeded")
    LaunchedEffect(env, instanceId) {
        try { store.workspace.environmentStream(env, "provider.auth.subscribe", buildJsonObject { put("instanceId", instanceId) })
            .collect { state = it } }
        catch (cancelled: CancellationException) { throw cancelled }
        catch (error: Exception) { failure = error.message }
    }
    fun action(method: String, extra: JsonObject = JsonObject(emptyMap()), after: () -> Unit = {}) {
        if (busy) return
        scope.launch {
            busy = true; failure = null
            try {
                store.workspace.environmentRequest(env, method, JsonObject(extra + ("instanceId" to JsonPrimitive(instanceId))))
                after()
            } catch (error: Exception) { failure = error.message ?: "Could not update sign-in." }
            finally { busy = false }
        }
    }
    fun respond(response: JsonObject, after: () -> Unit = { values = emptyMap() }) {
        val flow = flowId ?: return
        val interactionId = interaction?.v2String("id") ?: return
        action("provider.auth.respond", buildJsonObject {
            put("flowId", flow); put("interactionId", interactionId); put("response", response)
        }, after)
    }
    Card { Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(provider.v2String("displayName") ?: provider.v2String("driver").orEmpty(), style = MaterialTheme.typography.titleMedium)
        Text(state?.v2String("message") ?: if (signedIn) "Signed in" else "Connect this provider", style = MaterialTheme.typography.bodySmall)
        auth?.v2String("email")?.let { email -> TextButton(onClick = { emailVisible = !emailVisible }) { Text(if (emailVisible) email else "••••••@••••••") } }
        val kind = interaction?.v2String("type")
        if (kind == "deviceCode") Text("Enter code ${interaction?.v2String("userCode")}")
        if (kind == "terminal") {
            Text(interaction?.v2String("output").orEmpty().replace(Regex("\u001B\\[[0-?]*[ -/]*[@-~]"), "").takeLast(16000), style = MaterialTheme.typography.bodySmall)
            OutlinedTextField(values["input"].orEmpty(), { values = values + ("input" to it.take(4095)) }, label = { Text("Terminal response") },
                visualTransformation = PasswordVisualTransformation(), enabled = !busy)
            TextButton(enabled = !busy, onClick = { respond(buildJsonObject { put("type", "terminal"); put("data", values["input"].orEmpty() + "\r") }) }) { Text("Send response") }
        }
        if (kind == "credentials") {
            interaction?.v2Objects("fields").orEmpty().forEach { field ->
                val name = field.v2String("name")!!
                OutlinedTextField(values[name].orEmpty(), { values = values + (name to it.take(16384)) }, label = { Text(field.v2String("label").orEmpty()) },
                    visualTransformation = if (field.v2Bool("secret")) PasswordVisualTransformation() else VisualTransformation.None, enabled = !busy)
            }
            TextButton(enabled = !busy, onClick = { respond(buildJsonObject {
                put("type", "credentials"); put("values", JsonObject(values.mapValues { JsonPrimitive(it.value) }))
            }) }) { Text("Connect") }
        }
        val url = interaction?.v2String("url") ?: state?.v2String("authorizationUrl")
        if (url != null) {
            TextButton(enabled = !busy, onClick = {
                val open = { runCatching { uriHandler.openUri(url) }.onFailure { failure = "Could not open sign-in page." }; Unit }
                if (interaction?.v2Bool("requiresConsent") == true) respond(buildJsonObject { put("type", "browser"); put("action", "accept") }, open)
                else open()
            }) { Text("Open sign-in page") }
            if (interaction == null || interaction.v2Bool("acceptsCallback")) {
                OutlinedTextField(values["callback"].orEmpty(), { values = values + ("callback" to it.take(16384)) }, label = { Text("Final localhost URL") }, enabled = !busy)
                TextButton(enabled = !busy && !values["callback"].isNullOrBlank(), onClick = {
                    action("provider.auth.complete", buildJsonObject { put("flowId", flowId); put("callbackUrl", values["callback"]) }) { values = emptyMap() }
                }) { Text("Continue") }
            }
        }
        failure?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        if (active) TextButton(enabled = !busy, onClick = { action("provider.auth.cancel", buildJsonObject { put("flowId", flowId) }) }) { Text("Cancel sign-in") }
        else {
            val methods = state?.v2Objects("methods").orEmpty()
            Box {
                TextButton(enabled = !busy && state != null && provider.v2Bool("enabled") && provider.v2Bool("installed"), onClick = {
                    if (methods.size > 1) methodMenu = true else action("provider.auth.start")
                }) { Text(if (signedIn) "Change account" else "Sign in") }
                DropdownMenu(methodMenu, { methodMenu = false }) { methods.forEach { method ->
                    DropdownMenuItem(text = { Text(method.v2String("name").orEmpty()) }, onClick = {
                        methodMenu = false; action("provider.auth.start", buildJsonObject { put("methodId", method.v2String("id")) })
                    })
                } }
            }
            if (signedIn) TextButton(enabled = !busy, onClick = { logoutConfirm = true }) { Text("Sign out") }
        }
    } }
    if (logoutConfirm) AlertDialog(onDismissRequest = { logoutConfirm = false }, title = { Text("Sign out?") },
        text = { Text("Running threads sharing this sign-in will stop. Thread history is kept.") },
        dismissButton = { TextButton(onClick = { logoutConfirm = false }) { Text("Cancel") } },
        confirmButton = { TextButton(onClick = { logoutConfirm = false; action("provider.auth.logout") }) { Text("Sign out") } })
}
