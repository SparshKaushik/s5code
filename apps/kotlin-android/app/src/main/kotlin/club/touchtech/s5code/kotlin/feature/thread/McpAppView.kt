package club.touchtech.s5code.kotlin.feature.thread

import android.content.Intent
import android.webkit.JavascriptInterface
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.FileProvider
import androidx.core.net.toUri
import club.touchtech.s5code.kotlin.BuildConfig
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.model.*
import club.touchtech.s5code.kotlin.transport.TransportJson
import club.touchtech.s5code.kotlin.transport.v2Bool
import club.touchtech.s5code.kotlin.transport.v2String
import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.json.*
import java.io.File
import java.util.UUID

/** Taller apps scroll inside the inline view; full screen fills its own screen. */
internal const val MCP_APP_INLINE_MAX_HEIGHT = 420
private const val MCP_APP_MIN_HEIGHT = 80

/** The inline row height for an app's reported content height, or null to keep the current one. */
internal fun mcpAppInlineHeight(height: Double?): Int? {
    if (height == null || !height.isFinite()) return null
    return minOf(MCP_APP_INLINE_MAX_HEIGHT, Math.round(maxOf(MCP_APP_MIN_HEIGHT.toDouble(), height)).toInt())
}

private data class AppApproval(val title: String, val detail: String, val reply: CompletableDeferred<Boolean>)

/** Hosts the captured document with the same MCP Apps protocol implementation as RN. */
@Composable
fun McpAppView(store: AppStore, environmentId: EnvironmentId, conversationThreadId: ThreadId,
    entry: FeedEntry.McpApp, awaitingUser: Boolean = false) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var approval by remember(entry.id) { mutableStateOf<AppApproval?>(null) }
    val approvalLock = remember(entry.id) { Mutex() }
    var closed by remember(entry.id) { mutableStateOf(false) }
    var fullscreen by remember(entry.id) { mutableStateOf(false) }
    var attempt by remember(entry.id) { mutableIntStateOf(0) }
    var error by remember(entry.id, attempt) { mutableStateOf<String?>(null) }
    var url by remember(entry.id, attempt) { mutableStateOf<String?>(null) }
    var original by remember(entry.id, entry.revision) { mutableStateOf<JsonObject?>(null) }
    var webView by remember { mutableStateOf<WebView?>(null) }
    // Inline follows the app's reported height; a new document starts at the cap.
    var inlineHeight by remember(entry.id, entry.revision, attempt) { mutableIntStateOf(MCP_APP_INLINE_MAX_HEIGHT) }
    val secret = remember(entry.id, attempt, fullscreen) { UUID.randomUUID().toString() }
    suspend fun confirm(title: String, detail: String): Boolean = approvalLock.withLock {
        val request = AppApproval(title, detail, CompletableDeferred())
        approval = request
        try { request.reply.await() } finally { if (approval === request) approval = null }
    }
    fun target(input: JsonObject) = JsonObject(input + buildJsonObject {
        put("threadId", entry.sourceThreadId); put("itemId", entry.itemId)
    })
    LaunchedEffect(awaitingUser) { if (awaitingUser) fullscreen = false }
    LaunchedEffect(entry.id, attempt) {
        try {
            url = store.workspace.attachmentUrl(environmentId, entry.app.v2String("attachmentId")!!,
                fileName = "${entry.app.v2String("tool")}.html", mimeType = "text/html", disposition = "inline",
                renderIntent = "mcp-app")
        } catch (cancelled: CancellationException) { throw cancelled }
        catch (failure: Exception) { error = failure.message ?: "The app could not be loaded." }
    }
    LaunchedEffect(entry.id, entry.revision) {
        try { original = store.workspace.turnItemDetail(environmentId, ThreadId(entry.sourceThreadId), entry.itemId, "live") }
        catch (cancelled: CancellationException) { throw cancelled }
        catch (failure: Exception) { error = failure.message ?: "The app's tool result could not be loaded." }
    }
    LaunchedEffect(original, webView) {
        original?.let { item -> webView?.evaluateJavascript(
            "window.__t3AppToolCall&&window.__t3AppToolCall(${item.toString().replace("<", "\\u003c")});true;", null) }
    }
    val onMessage: (String) -> Unit = { raw ->
        scope.launch {
            val message = withContext(Dispatchers.Default) {
                runCatching { TransportJson.parseToJsonElement(raw) }.getOrNull() as? JsonObject
            }
            if (message?.v2String("secret") == secret) {
                val requestId = message["id"]
                val view = webView
                try {
                    val input = message["input"] as? JsonObject ?: JsonObject(emptyMap())
                    val result: JsonElement = when (message.v2String("operation")) {
                        "callTool" -> {
                            val info = store.workspace.mcpAppRequest(environmentId, "mcpApps.toolInfo", target(buildJsonObject {
                                put("name", input.v2String("name"))
                            }))
                            check(info.v2Bool("callable")) { "This app cannot call that tool." }
                            if (!info.v2Bool("readOnly")) check(confirm(
                                "Allow ${entry.app.v2String("server")} to run ${info.v2String("title") ?: input.v2String("name")}?",
                                input["arguments"].toString())) { "Declined by the user." }
                            store.workspace.mcpAppRequest(environmentId, "mcpApps.callTool", target(input))
                        }
                        "readResource" -> store.workspace.mcpAppRequest(environmentId, "mcpApps.readResource", target(input))
                        "updateModelContext" -> store.workspace.mcpAppRequest(environmentId, "mcpApps.updateModelContext",
                            target(JsonObject(input + ("conversationThreadId" to JsonPrimitive(conversationThreadId.value)))))
                        "openLink" -> {
                            val link = input.v2String("url").orEmpty()
                            check(link.startsWith("https://") || link.startsWith("http://")) { "Unsupported link." }
                            check(confirm("Open a link from ${entry.app.v2String("server")}?", link)) { "Declined by the user." }
                            context.startActivity(Intent(Intent.ACTION_VIEW, link.toUri()))
                            JsonNull
                        }
                        "sendMessage" -> {
                            val text = input.v2String("text").orEmpty()
                            check(confirm("Send this message from ${entry.app.v2String("server")}?", text)) { "Declined by the user." }
                            val settings = store.workspace.thread(environmentId, conversationThreadId).value?.settings ?: ThreadSettings()
                            store.enqueueThreadMessage(environmentId.value, conversationThreadId.value, text, emptyList(), settings)
                            JsonNull
                        }
                        "requestDisplayMode" -> {
                            val mode = input.v2String("mode")
                            check(mode == "inline" || mode == "fullscreen") { "Display mode unavailable." }
                            check(mode != "fullscreen" || !awaitingUser) { "The agent is waiting for your answer." }
                            fullscreen = mode == "fullscreen"
                            JsonPrimitive(mode)
                        }
                        "downloadFile" -> {
                            val files = input["files"] as? JsonArray ?: error("No files supplied.")
                            check(confirm("Save files from ${entry.app.v2String("server")}?",
                                files.joinToString { (it as? JsonObject)?.v2String("name").orEmpty() })) { "Declined by the user." }
                            files.forEach { rawFile ->
                                val file = rawFile as? JsonObject ?: error("Invalid file.")
                                val bytes = if (file.v2String("_tag") == "embedded") {
                                    val encoded = file.v2String("base64").orEmpty()
                                    check(encoded.length <= 28 * 1024 * 1024) { "File too large." }
                                    android.util.Base64.decode(encoded, android.util.Base64.DEFAULT)
                                } else {
                                    val resource = store.workspace.mcpAppRequest(environmentId, "mcpApps.readResource", target(buildJsonObject {
                                        put("uri", file.v2String("uri"))
                                    }))
                                    val content = (resource["contents"] as? JsonArray)?.firstOrNull() as? JsonObject ?: error("Empty resource.")
                                    content.v2String("text")?.toByteArray() ?: android.util.Base64.decode(
                                        content.v2String("blob") ?: error("Resource has no bytes."), android.util.Base64.DEFAULT)
                                }
                                val safeName = file.v2String("name").orEmpty().replace(Regex("[^A-Za-z0-9._-]"), "_").takeLast(120).ifBlank { "app-file" }
                                val saved = File(File(context.cacheDir, "shared-attachments"), "${UUID.randomUUID()}-$safeName")
                                withContext(Dispatchers.IO) { saved.parentFile?.mkdirs(); saved.writeBytes(bytes) }
                                val uri = FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", saved)
                                context.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).apply {
                                    type = file.v2String("mimeType") ?: "application/octet-stream"
                                    putExtra(Intent.EXTRA_STREAM, uri); addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                                }, null))
                            }
                            JsonNull
                        }
                        "sizeChanged" -> {
                            if (!fullscreen) mcpAppInlineHeight(input["height"]?.jsonPrimitive?.doubleOrNull)?.let { inlineHeight = it }
                            JsonNull
                        }
                        "teardown" -> { closed = true; JsonNull }
                        "rendererGone" -> {
                            closed = true
                            fullscreen = false
                            error = "The app viewer stopped. Reopen it to continue."
                            JsonNull
                        }
                        "navigated" -> error("The app navigated away. Reopen it to continue.")
                        else -> error("Unsupported app request.")
                    }
                    if (requestId != null) view?.evaluateJavascript("window.__t3AppResult($requestId,${result.toString().replace("<", "\\u003c")},null);true;", null)
                } catch (cancelled: CancellationException) { throw cancelled }
                catch (failure: Exception) {
                    val reason = failure.message ?: "App request failed."
                    if (requestId != null) view?.evaluateJavascript("window.__t3AppResult($requestId,null,${JsonPrimitive(reason)});true;", null)
                    else error = reason
                }
            }
        }
    }
    val source = url
    Column(Modifier.fillMaxWidth()) {
        Row {
            Text(entry.app.v2String("tool") ?: "App", modifier = Modifier.weight(1f).padding(8.dp))
            TextButton(onClick = { closed = !closed }) { Text(if (closed) "Open app" else "Close") }
            if (!closed) TextButton(enabled = !awaitingUser, onClick = { fullscreen = true }) { Text("Full screen") }
        }
        error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        if (error != null) TextButton(onClick = { attempt += 1; closed = false }) { Text("Retry") }
        if (!closed && source != null && !fullscreen) {
            key(secret) { McpAppSurface(source, entry.app, secret, false, onMessage,
                { webView = it }, Modifier.fillMaxWidth().height(inlineHeight.dp)) }
        }
    }
    if (fullscreen && !closed && source != null) Dialog(onDismissRequest = { fullscreen = false },
        properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(Modifier.fillMaxSize()) { Column {
            TextButton(onClick = { fullscreen = false }) { Text("Back to thread") }
            key(secret) { McpAppSurface(source, entry.app, secret, true, onMessage,
                { webView = it }, Modifier.weight(1f).fillMaxWidth()) }
        } }
    }
    approval?.let { request -> AlertDialog(onDismissRequest = { request.reply.complete(false) },
        title = { Text(request.title) }, text = { Text(request.detail.take(4000)) },
        confirmButton = { TextButton(onClick = { request.reply.complete(true) }) { Text("Allow") } },
        dismissButton = { TextButton(onClick = { request.reply.complete(false) }) { Text("Cancel") } }) }
}

private class McpAppBridge(private val post: (String) -> Unit) {
    @JavascriptInterface fun postMessage(message: String) = post(message)
}

@Composable
@android.annotation.SuppressLint("SetJavaScriptEnabled")
private fun McpAppSurface(source: String, app: JsonObject, secret: String, fullscreen: Boolean,
    onMessage: (String) -> Unit, onView: (WebView?) -> Unit, modifier: Modifier) {
    val context = LocalContext.current
    val callback by rememberUpdatedState(onMessage)
    val theme = if (MaterialTheme.colorScheme.background.red < 0.5f) "dark" else "light"
    val document = remember(source, app, secret, fullscreen) {
        val script = context.assets.open("mcp-app-host.js").bufferedReader().use { it.readText() }
            .replace("</script", "<\\/script", ignoreCase = true)
        val safeSource = JsonPrimitive(source).toString().replace("<", "\\u003c")
        val safeApp = app.toString().replace("<", "\\u003c")
        """<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1">
            <style>html,body{margin:0;height:100%;background:transparent}iframe{border:0;width:100%;height:100%;display:block}</style></head>
            <body><iframe id="app" sandbox="allow-scripts allow-forms"></iframe><script>$script
            (function(){var frame=document.getElementById('app'),secret=${JsonPrimitive(secret)},pending=new Map(),next=1,live=true,loads=0;
            function post(m){window.ReactNativeWebView.postMessage(JSON.stringify(Object.assign({secret:secret},m)));}
            function call(operation,input){return new Promise(function(resolve,reject){var id=next++;pending.set(id,{resolve:resolve,reject:reject});post({id:id,operation:operation,input:input});});}
            window.__t3AppResult=function(id,value,error){var p=pending.get(id);if(!p)return;pending.delete(id);error?p.reject(new Error(error)):p.resolve(value);};
            var host=T3McpAppHost.makeMcpAppHost({app:$safeApp,hostVersion:${JsonPrimitive(BuildConfig.VERSION_NAME)},
            post:function(m){if(live&&frame.contentWindow)frame.contentWindow.postMessage(m,'*');},
            hostContext:function(){return {theme:'$theme',styles:{variables:{}},displayMode:'${if (fullscreen) "fullscreen" else "inline"}',availableDisplayModes:['inline','fullscreen'],containerDimensions:${if (fullscreen) "{width:innerWidth,height:innerHeight}" else "{width:innerWidth,maxHeight:$MCP_APP_INLINE_MAX_HEIGHT}"},platform:'mobile',deviceCapabilities:{touch:true,hover:false}};},
            callTool:function(i){return call('callTool',i);},readResource:function(i){return call('readResource',i);},
            updateModelContext:function(i){return call('updateModelContext',i);},openLink:function(url){return call('openLink',{url:url});},sendMessage:function(text){return call('sendMessage',{text:text});},
            requestDisplayMode:function(mode){return call('requestDisplayMode',{mode:mode});},downloadFile:function(files){return call('downloadFile',{files:files.map(function(f){if(f._tag!=='embedded')return f;var s='';f.bytes.forEach(function(b){s+=String.fromCharCode(b);});return {_tag:f._tag,name:f.name,mimeType:f.mimeType,base64:btoa(s)};})});},
            onRequestTeardown:function(){post({operation:'teardown'});},onSizeChanged:function(s){if(s&&typeof s.height==='number')post({operation:'sizeChanged',input:{height:s.height}});}});
            window.__t3AppToolCall=function(item){host.toolCall({arguments:item.input,result:item.output});};
            frame.addEventListener('load',function(){loads++;if(loads>1){live=false;host.dispose();post({operation:'navigated'});}});
            window.addEventListener('message',function(e){if(live&&e.source===frame.contentWindow)host.receive(e.data);});
            window.addEventListener('pagehide',function(){live=false;host.dispose();});frame.src=$safeSource;
            })();</script></body></html>""".trimIndent()
    }
    AndroidView(modifier = modifier, factory = { viewContext -> WebView(viewContext).apply {
        settings.javaScriptEnabled = true
        settings.allowFileAccess = false
        settings.allowContentAccess = false
        webViewClient = object : WebViewClient() {
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest) = request.isForMainFrame
            override fun onPageFinished(view: WebView, url: String?) { onView(view) }
            override fun onRenderProcessGone(view: WebView, detail: android.webkit.RenderProcessGoneDetail): Boolean {
                callback(buildJsonObject { put("secret", secret); put("operation", "rendererGone") }.toString())
                return true
            }
        }
        addJavascriptInterface(McpAppBridge { callback(it) }, "ReactNativeWebView")
        loadDataWithBaseURL(source, document, "text/html", "UTF-8", null)
    } }, onRelease = { view -> onView(null); view.removeJavascriptInterface("ReactNativeWebView"); view.destroy() })
}
