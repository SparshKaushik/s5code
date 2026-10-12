package club.touchtech.s5code.kotlin.feature.browser

import android.webkit.JavascriptInterface
import android.webkit.WebView
import android.webkit.WebChromeClient
import android.webkit.ValueCallback
import android.net.Uri
import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.FileProvider
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.horizontalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.model.*
import club.touchtech.s5code.kotlin.transport.TransportJson
import club.touchtech.s5code.kotlin.transport.v2Bool
import club.touchtech.s5code.kotlin.transport.v2String
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.launch
import kotlinx.serialization.json.*

@Composable
fun BrowserScreen(store: AppStore, environmentId: String, threadId: String, onBack: () -> Unit) {
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val id = remember(threadId) { ThreadId(threadId) }
    val scope = rememberCoroutineScope()
    var error by remember { mutableStateOf<String?>(null) }
    val tabs by remember(env, id) {
        store.workspace.browserTabs(env, id).catch {
            if (it is CancellationException) throw it
            error = it.message ?: "Browser tabs could not be loaded."
        }
    }.collectAsStateWithLifecycle(emptyList())
    var selectedId by remember(threadId) { mutableStateOf<String?>(null) }
    val selected = tabs.firstOrNull { it.v2String("tabId") == selectedId } ?: tabs.firstOrNull()
    val tabId = selected?.v2String("tabId")
    val navigation = selected?.get("navStatus") as? JsonObject
    var address by remember(tabId) { mutableStateOf(navigation?.v2String("url").orEmpty()) }
    LaunchedEffect(navigation?.v2String("url")) { address = navigation?.v2String("url").orEmpty() }
    var attempt by remember { mutableIntStateOf(0) }
    var unauthorizedRetries by remember(tabId) { mutableIntStateOf(0) }
    var rendererStopped by remember(tabId, attempt) { mutableStateOf(false) }
    var webView by remember { mutableStateOf<WebView?>(null) }
    var control by remember(tabId) { mutableStateOf<JsonObject?>(null) }
    var chooser by remember(tabId) { mutableStateOf<JsonObject?>(null) }
    val context = LocalContext.current
    val hasControl = control?.v2String("controller") == "you"
    val canOperate = control?.v2Bool("canOperate") == true
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    var foreground by remember { mutableStateOf(lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)) }
    LaunchedEffect(lifecycle) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            foreground = true
            try { awaitCancellation() } finally { foreground = false }
        }
    }
    var access by remember(env, tabId, attempt) { mutableStateOf<DeviceHubAccess?>(null) }
    LaunchedEffect(env, tabId, attempt, foreground) {
        access = null
        if (tabId != null && foreground) {
            try { access = store.workspace.browserAccess(env) }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (failure: Exception) { error = failure.message ?: "Browser connection failed." }
        }
    }
    fun command(input: JsonObject) {
        webView?.evaluateJavascript("T3PreviewStream.command($input);true;", null)
    }
    fun action(method: String, input: JsonObject) {
        scope.launch {
            try {
                error = null
                val result = store.workspace.browserCommand(env, method, input)
                result.v2String("tabId")?.let { selectedId = it }
            } catch (cancelled: CancellationException) { throw cancelled }
            catch (failure: Exception) { error = failure.message ?: "Browser action failed." }
        }
    }
    S5Screen(title = "Browser", onBack = onBack) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            Row(Modifier.horizontalScroll(rememberScrollState())) {
                tabs.forEachIndexed { index, tab ->
                    TextButton(onClick = { selectedId = tab.v2String("tabId") }) {
                        Text((tab["navStatus"] as? JsonObject)?.v2String("title")
                            ?: "Tab ${index + 1}", maxLines = 1)
                    }
                }
                TextButton(onClick = { action("preview.open", buildJsonObject {
                    put("threadId", threadId); put("runtime", "server")
                }) }) { Text("New tab") }
                if (tabId != null) TextButton(onClick = { action("preview.close", buildJsonObject {
                    put("threadId", threadId); put("tabId", tabId)
                }) }) { Text("Close tab") }
            }
            Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp)) {
                OutlinedTextField(value = address, onValueChange = { address = it }, singleLine = true,
                    label = { Text("Address") }, modifier = Modifier.weight(1f))
                TextButton(enabled = hasControl && address.isNotBlank(), onClick = {
                    command(buildJsonObject { put("type", "navigate"); put("url", browserAddress(address)) })
                }) { Text("Go") }
            }
            Row(Modifier.horizontalScroll(rememberScrollState())) {
                TextButton(enabled = hasControl && selected?.v2Bool("canGoBack") == true,
                    onClick = { command(buildJsonObject { put("type", "history"); put("delta", -1) }) }) { Text("Back") }
                TextButton(enabled = hasControl && selected?.v2Bool("canGoForward") == true,
                    onClick = { command(buildJsonObject { put("type", "history"); put("delta", 1) }) }) { Text("Forward") }
                TextButton(enabled = hasControl, onClick = { command(buildJsonObject { put("type", "reload") }) }) { Text("Reload") }
                TextButton(enabled = canOperate, onClick = {
                    command(buildJsonObject { put("type", if (hasControl) "releaseControl" else "takeControl") })
                }) { Text(if (hasControl) "Release control" else "Take control") }
                TextButton(onClick = { error = null; unauthorizedRetries = 0; attempt += 1 }) { Text("Reconnect") }
                chooser?.let { upload -> TextButton(enabled = hasControl, onClick = {
                    webView?.evaluateJavascript("T3PreviewStream.selectFiles($upload);true;", null)
                }) { Text("Upload files") } }
            }
            Text(when {
                control == null -> "Connecting…"
                !canOperate -> "Read-only"
                hasControl -> "You have control"
                control?.v2String("controller") == "agent" -> "Agent has control"
                else -> "Watching"
            }, style = MaterialTheme.typography.labelMedium, modifier = Modifier.padding(8.dp))
            error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(8.dp)) }
            val currentAccess = access
            if (tabId != null && currentAccess != null && foreground && !rendererStopped) {
                key(tabId, attempt, currentAccess) {
                    BrowserStreamView(currentAccess, threadId, tabId, Modifier.weight(1f).fillMaxWidth(),
                        onView = { webView = it }, onMessage = { message ->
                            when (message.v2String("type")) {
                                "control" -> control = message
                                "rendererGone" -> {
                                    rendererStopped = true
                                    control = null
                                    error = "The browser viewer stopped. Reconnect to continue."
                                }
                                "status" -> when (message.v2String("status")) {
                                    "error" -> error = message.v2String("detail")
                                    "streaming" -> { error = null; unauthorizedRetries = 0 }
                                }
                                "hostSetup" -> error = "The browser host needs setup: ${message.v2String("command")}"
                                "uploadError" -> error = message.v2String("detail")
                                "fileChooser" -> chooser = message["chooser"] as? JsonObject
                                "download" -> scope.launch {
                                    try {
                                        val link = message.v2String("url") ?: return@launch
                                        check(link.startsWith(currentAccess.httpBase + "/download?")) { "Invalid browser download." }
                                        val name = message.v2String("fileName").orEmpty().replace(Regex("[^A-Za-z0-9._-]"), "_").takeLast(120).ifBlank { "download" }
                                        val file = java.io.File(java.io.File(context.cacheDir, "shared-attachments"), "${java.util.UUID.randomUUID()}-$name")
                                        store.workspace.downloadAsset(link, file)
                                        val uri = FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", file)
                                        context.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).apply {
                                            type = "application/octet-stream"; putExtra(Intent.EXTRA_STREAM, uri)
                                            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                                        }, null))
                                    } catch (cancelled: CancellationException) { throw cancelled }
                                    catch (failure: Exception) { error = failure.message ?: "Download failed." }
                                }
                                "unauthorized" -> {
                                    control = null
                                    if (unauthorizedRetries++ < 2) attempt += 1
                                    else error = "Browser access was refused. Reconnect or pair again."
                                }
                                "gone" -> error = "This browser tab was closed."
                            }
                        })
                }
            } else if (tabs.isEmpty()) Text("Open a tab to browse on this environment.", modifier = Modifier.padding(16.dp))
        }
    }
    val dialog = control?.get("dialog") as? JsonObject
    if (dialog != null && hasControl) {
        var prompt by remember(dialog) { mutableStateOf(dialog.v2String("defaultValue").orEmpty()) }
        AlertDialog(onDismissRequest = { command(buildJsonObject { put("type", "dialog"); put("accept", false) }) },
            title = { Text("Browser dialog") }, text = { Column {
                Text(dialog.v2String("message").orEmpty())
                if (dialog.v2String("type") == "prompt") OutlinedTextField(prompt, { prompt = it })
            } }, confirmButton = { TextButton(onClick = { command(buildJsonObject {
                put("type", "dialog"); put("accept", true); put("promptText", prompt)
            }) }) { Text("OK") } }, dismissButton = { TextButton(onClick = { command(buildJsonObject {
                put("type", "dialog"); put("accept", false)
            }) }) { Text("Cancel") } })
    }
}

internal fun browserAddress(address: String): String =
    address.trim().let { if (Regex("^[a-zA-Z][a-zA-Z0-9+.-]*:").containsMatchIn(it)) it else "https://$it" }

private class BrowserBridge(private val onMessage: (String) -> Unit) {
    @JavascriptInterface fun postMessage(message: String) = onMessage(message)
}

@Composable
@android.annotation.SuppressLint("SetJavaScriptEnabled")
private fun BrowserStreamView(access: DeviceHubAccess, threadId: String, tabId: String,
    modifier: Modifier, onView: (WebView?) -> Unit, onMessage: (JsonObject) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val callback by rememberUpdatedState(onMessage)
    var fileCallback by remember { mutableStateOf<ValueCallback<Array<Uri>>?>(null) }
    var allowMultiple by remember { mutableStateOf(false) }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        fileCallback?.onReceiveValue(uris.takeIf { it.isNotEmpty() }?.let {
            (if (allowMultiple) it else it.take(1)).toTypedArray()
        })
        fileCallback = null
    }
    val document = remember(access, threadId, tabId) {
        val config = buildJsonObject {
            put("access", buildJsonObject {
                put("httpBase", access.httpBase); put("wsBase", access.wsBase)
                put("query", buildJsonObject { access.query.forEach { (name, value) -> put(name, value) } })
                put("credentials", false)
            })
            put("threadId", threadId); put("tabId", tabId); put("interactive", true); put("background", "#111111")
        }.toString().replace("<", "\\u003c")
        val script = context.assets.open("preview-stream.js").bufferedReader().use { it.readText() }
            .replace("</script", "<\\/script", ignoreCase = true)
        """<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1,maximum-scale=1,user-scalable=no">
            <style>html,body{height:100%;overflow:hidden;margin:0}body>div{position:fixed;inset:0;width:100%;height:100%;overflow:hidden}
            canvas,video{position:absolute;inset:0;width:100%;height:100%;object-fit:contain}canvas{touch-action:none}.agent-cursor{position:absolute;width:20px;height:20px;pointer-events:none;opacity:0}
            textarea{position:fixed;left:0;top:0;width:1px;height:1px;margin:-1px;border:0;clip:rect(0,0,0,0);font-size:16px}</style></head>
            <body><script>$script
            T3PreviewStream.start($config);</script></body></html>""".trimIndent()
    }
    AndroidView(modifier = modifier, factory = { viewContext -> WebView(viewContext).apply {
        settings.javaScriptEnabled = true
        settings.allowFileAccess = false
        settings.allowContentAccess = true
        webViewClient = object : android.webkit.WebViewClient() {
            override fun shouldOverrideUrlLoading(view: WebView, request: android.webkit.WebResourceRequest) = true
            override fun onRenderProcessGone(view: WebView, detail: android.webkit.RenderProcessGoneDetail): Boolean {
                callback(buildJsonObject { put("type", "rendererGone") })
                return true
            }
        }
        webChromeClient = object : WebChromeClient() {
            override fun onShowFileChooser(view: WebView, result: ValueCallback<Array<Uri>>, params: FileChooserParams): Boolean {
                fileCallback?.onReceiveValue(null)
                fileCallback = result
                allowMultiple = params.mode == FileChooserParams.MODE_OPEN_MULTIPLE
                picker.launch(params.acceptTypes.filter { it.isNotBlank() && !it.startsWith(".") }.toTypedArray().ifEmpty { arrayOf("*/*") })
                return true
            }
        }
        addJavascriptInterface(BrowserBridge { raw -> scope.launch {
            (runCatching { TransportJson.parseToJsonElement(raw) }.getOrNull() as? JsonObject)?.let(callback)
        } }, "ReactNativeWebView")
        loadDataWithBaseURL(access.httpBase, document, "text/html", "UTF-8", null)
        onView(this)
    } }, onRelease = { view -> fileCallback?.onReceiveValue(null); fileCallback = null
        onView(null); view.removeJavascriptInterface("ReactNativeWebView"); view.destroy() })
}
