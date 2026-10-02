package club.touchtech.s5code.kotlin.feature.devices

import android.annotation.SuppressLint
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.DeviceToolUpdateOwnership
import club.touchtech.s5code.kotlin.data.deviceToolUpdatePolicy
import club.touchtech.s5code.kotlin.data.deviceToolVersionLabels
import club.touchtech.s5code.kotlin.data.selectedThreadDevicePreview
import club.touchtech.s5code.kotlin.data.threadDevicePreviews
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.DeviceHubAccess
import club.touchtech.s5code.kotlin.model.ThreadDevicePreview
import kotlinx.coroutines.launch
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive

/**
 * The thread's open devices, streamed through the environment's device hub —
 * `DevicePreviewRouteScreen` on RN. The stream itself is the same
 * `T3DeviceStream` bundle the React Native WebView runs, generated into
 * `assets/device-stream.js`; only the native shell (toolbar, bridge, lifecycle)
 * is Kotlin.
 */

private val StreamJson = Json { ignoreUnknownKeys = true }

private fun hexOf(color: Color): String =
    String.format("#%02X%02X%02X", (color.red * 255).toInt(), (color.green * 255).toInt(), (color.blue * 255).toInt())

/** `deviceStreamDocument`: the page the stream bundle boots in. */
private fun deviceStreamDocument(configuration: String, script: String): String {
    // Tickets and device names are data, including any HTML delimiter characters.
    val safeConfiguration = configuration.replace("<", "\\u003c")
    val safeScript = script.replace(Regex("</script", RegexOption.IGNORE_CASE), "<\\/script")
    val failure =
        "window.ReactNativeWebView.postMessage(" +
            "JSON.stringify({type:\"status\",status:\"error\"," +
            "detail:\"Device viewer stopped unexpectedly.\"}));"
    return "<!doctype html><html><head>" +
        "<meta name=\"viewport\" content=\"width=device-width,initial-scale=1," +
        "maximum-scale=1,user-scalable=no\"></head><body><script>" +
        "window.addEventListener(\"error\",function(){$failure});" +
        "window.addEventListener(\"unhandledrejection\",function(){$failure});\n" +
        safeScript +
        "\ntry{T3DeviceStream.start($safeConfiguration);}catch{$failure}" +
        "</script></body></html>"
}

/** `deviceStreamMessage`: parses one bridge frame. */
private sealed interface StreamMessage {
    data object Unauthorized : StreamMessage
    data object Retry : StreamMessage
    data class Input(val connected: Boolean) : StreamMessage
    data class Status(val status: String, val detail: String?) : StreamMessage
}

private fun deviceStreamMessage(data: String): StreamMessage? =
    runCatching {
            // A malformed frame is noise, never a crash on the JS interface thread.
            val message =
                StreamJson.parseToJsonElement(data) as? JsonObject ?: return@runCatching null
            when (message["type"]?.jsonPrimitive?.content) {
                "unauthorized" -> StreamMessage.Unauthorized
                "retry" -> StreamMessage.Retry
                "input" ->
                    message["connected"]?.jsonPrimitive?.booleanOrNull?.let {
                        StreamMessage.Input(it)
                    }
                "status" -> {
                    val status = message["status"]?.jsonPrimitive?.content
                    if (status in listOf("connecting", "streaming", "error")) {
                        StreamMessage.Status(
                            status!!,
                            message["detail"]?.jsonPrimitive?.contentOrNull,
                        )
                    } else null
                }
                else -> null
            }
        }
        .getOrNull()

/** The bundle's `window.ReactNativeWebView.postMessage` target. */
private class StreamBridge(val onMessage: (String) -> Unit) {
    @JavascriptInterface
    fun postMessage(data: String) = onMessage(data)
}

@Composable
fun DevicePreviewScreen(
    store: AppStore,
    environmentId: String,
    threadId: String,
    onBack: () -> Unit,
) {
    val env = EnvironmentId(environmentId)
    val state by store.workspace.deviceState(env).collectAsStateWithLifecycle()
    val previews = remember(state, threadId) { threadDevicePreviews(state, threadId) }
    var selectedKey by remember { mutableStateOf<String?>(null) }
    val preview = remember(previews, selectedKey) { selectedThreadDevicePreview(previews, selectedKey) }
    val scope = rememberCoroutineScope()

    var inputConnected by remember { mutableStateOf(false) }
    var streamAttempt by remember { mutableIntStateOf(0) }
    var accessNonce by remember { mutableIntStateOf(0) }
    var access by remember { mutableStateOf<DeviceHubAccess?>(null) }
    var accessError by remember { mutableStateOf<String?>(null) }
    var shuttingDown by remember { mutableStateOf(false) }
    var hostDetailDialog by remember { mutableStateOf<String?>(null) }
    var menuOpen by remember { mutableStateOf(false) }
    var webView by remember { mutableStateOf<WebView?>(null) }

    // The WebView stops drawing in the background; streaming resumes on return.
    var foreground by remember { mutableStateOf(true) }
    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner) {
        val observer =
            LifecycleEventObserver { _, event ->
                foreground =
                    when (event) {
                        Lifecycle.Event.ON_RESUME -> true
                        Lifecycle.Event.ON_PAUSE, Lifecycle.Event.ON_STOP -> false
                        else -> foreground
                    }
            }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    // The sheet answers "no devices open" by closing, like RN's focused check.
    LaunchedEffect(state, previews.size) {
        if (state != null && previews.isEmpty()) onBack()
    }

    // Fresh ticket per open and per unauthorized/retry pass — hub media cannot
    // send headers, so the ticket is the only credential the stream carries.
    LaunchedEffect(preview?.session?.hostId, accessNonce, streamAttempt) {
        val hostId = preview?.session?.hostId ?: return@LaunchedEffect
        accessError = null
        runCatching { store.workspace.deviceHubAccess(env, hostId) }
            .onSuccess { access = it }
            .onFailure {
                access = null
                accessError = it.message ?: "Could not reach the device hub."
            }
    }

    val streamCommand: (String) -> Unit = { button ->
        webView?.evaluateJavascript(
            "window.T3DeviceStream?.command(${jsonString(button)}); true;",
            null,
        )
    }

    DisposableEffect(preview?.key, streamAttempt) {
        onDispose {
            webView?.evaluateJavascript("window.T3DeviceStream?.stop(); true;", null)
            inputConnected = false
        }
    }

    S5Screen(
        title = preview?.name ?: "Device",
        subtitle = preview?.description?.takeIf { it.isNotEmpty() },
        onBack = onBack,
        actions = {
            IconButton(onClick = { menuOpen = true }) {
                Icon(Icons.Rounded.MoreVert, contentDescription = "Device options")
            }
            DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                if (previews.size > 1) {
                    previews.forEach { device ->
                        DropdownMenuItem(
                            text = {
                                Column {
                                    Text(device.name)
                                    if (device.description.isNotEmpty()) {
                                        Text(
                                            device.description,
                                            style = MaterialTheme.typography.labelSmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                    }
                                }
                            },
                            onClick = {
                                selectedKey = device.key
                                menuOpen = false
                            },
                            trailingIcon = if (device.key == preview?.key) ({ Text("•") }) else null,
                        )
                    }
                }
                // Failed hosts retry one at a time, like the RN header menu.
                state?.hosts
                    ?.filter { host ->
                        state?.supportsHostRetry == true &&
                            state?.hostStatuses?.get(host.id)?.status == "failed"
                    }
                    ?.forEach { host ->
                        DropdownMenuItem(
                            text = { Text("Retry ${host.label}") },
                            onClick = {
                                menuOpen = false
                                scope.launch {
                                    runCatching { store.workspace.refreshDevices(env, host.id) }
                                        .onFailure { store.showError(it.message ?: "Retry failed.") }
                                }
                            },
                        )
                    }
                if (state?.supportsToolInspection == true) {
                    DropdownMenuItem(
                        text = { Text("Check device tool versions") },
                        onClick = {
                            menuOpen = false
                            scope.launch {
                                runCatching { store.workspace.inspectDeviceTools(env) }
                                    .onFailure { store.showError(it.message ?: "Check failed.") }
                            }
                        },
                    )
                }
                DropdownMenuItem(
                    text = { Text("Device tool versions") },
                    onClick = {
                        menuOpen = false
                        val host = state?.hosts?.firstOrNull { it.id == preview?.session?.hostId }
                        hostDetailDialog =
                            buildString {
                                append(DeviceToolUpdateOwnership)
                                append("\n\n")
                                append(deviceToolUpdatePolicy(host?.tools))
                                append("\n\n")
                                append(deviceToolVersionLabels(host?.tools).joinToString("\n"))
                                val detail =
                                    host?.toolInspectionError
                                        ?: state?.hostStatuses?.get(preview?.session?.hostId)?.detail
                                detail?.let { append("\n").append(it) }
                            }
                    },
                )
                DropdownMenuItem(
                    text = { Text("Reload stream") },
                    enabled = preview != null && !shuttingDown,
                    onClick = {
                        menuOpen = false
                        inputConnected = false
                        streamAttempt += 1
                    },
                )
                if (preview?.session?.platform == "android") {
                    DropdownMenuItem(
                        text = { Text("Back") },
                        enabled = inputConnected,
                        onClick = {
                            menuOpen = false
                            streamCommand("back")
                        },
                    )
                }
                DropdownMenuItem(
                    text = { Text("App switcher") },
                    enabled = inputConnected,
                    onClick = {
                        menuOpen = false
                        streamCommand("appSwitcher")
                    },
                )
                if (preview?.session?.platform == "ios") {
                    DropdownMenuItem(
                        text = { Text("Rotate device") },
                        enabled = inputConnected,
                        onClick = {
                            menuOpen = false
                            streamCommand("rotate")
                        },
                    )
                }
                DropdownMenuItem(
                    text = { Text("Home") },
                    enabled = inputConnected,
                    onClick = {
                        menuOpen = false
                        streamCommand("home")
                    },
                )
                preview?.let { current ->
                    DropdownMenuItem(
                        text = { Text("Shut down device") },
                        enabled = !shuttingDown,
                        onClick = {
                            menuOpen = false
                            shuttingDown = true
                            scope.launch {
                                runCatching {
                                        store.workspace.shutdownDevice(
                                            env,
                                            current.session.hostId,
                                            current.session.deviceId,
                                            current.session.platform,
                                        )
                                    }
                                    .onFailure {
                                        store.showError(
                                            "Could not shut down device: " +
                                                (it.message ?: "unknown error")
                                        )
                                    }
                                shuttingDown = false
                            }
                        },
                    )
                }
            }
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            val currentPreview = preview
            if (currentPreview == null || !foreground) {
                // RN shows the device list's empty state here; Kotlin shows a
                // spinner — the auto-close effect above handles true empties.
                Column(
                    Modifier.fillMaxSize().padding(S5Theme.spacing.gutter),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    CircularProgressIndicator()
                }
            } else {
                val currentAccess = access
                if (currentAccess == null) {
                    Column(
                        Modifier.fillMaxSize().padding(S5Theme.spacing.gutter),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.Center,
                    ) {
                        if (accessError != null) {
                            Text(
                                accessError.orEmpty(),
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            TextButton(onClick = { accessNonce += 1 }) { Text("Retry") }
                        } else {
                            CircularProgressIndicator()
                            Text(
                                "Connecting to device...",
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                } else {
                    DeviceStream(
                        access = currentAccess,
                        platform = currentPreview.session.platform,
                        deviceId = currentPreview.session.deviceId,
                        attempt = streamAttempt,
                        onBridgeMessage = { message ->
                            when (message) {
                                is StreamMessage.Unauthorized -> accessNonce += 1
                                is StreamMessage.Retry -> {
                                    accessNonce += 1
                                    streamAttempt += 1
                                }
                                is StreamMessage.Input -> inputConnected = message.connected
                                is StreamMessage.Status -> Unit // handled inside DeviceStream
                            }
                        },
                        onUnauthorized = { accessNonce += 1 },
                        webViewRef = { webView = it },
                    )
                }
            }
        }
    }

    hostDetailDialog?.let { detail ->
        androidx.compose.material3.AlertDialog(
            onDismissRequest = { hostDetailDialog = null },
            confirmButton = { TextButton(onClick = { hostDetailDialog = null }) { Text("OK") } },
            title = { Text("Device tool versions") },
            text = { Text(detail) },
        )
    }
}

/** The stream card: page hosting, bridge, and the connecting/error overlay. */
@SuppressLint("SetJavaScriptEnabled", "JavascriptInterface")
@Composable
private fun DeviceStream(
    access: DeviceHubAccess,
    platform: String,
    deviceId: String,
    attempt: Int,
    onBridgeMessage: (StreamMessage) -> Unit,
    onUnauthorized: () -> Unit,
    webViewRef: (WebView?) -> Unit,
) {
    val context = LocalContext.current
    var status by remember(attempt) { mutableStateOf("connecting") }
    var error by remember(attempt) { mutableStateOf<String?>(null) }

    // `remember`'s lambda cannot call @Composable MaterialTheme, so the color
    // scheme is read during composition and only hex strings enter the cache.
    val scheme = MaterialTheme.colorScheme
    val configuration =
        remember(access, platform, deviceId, scheme) {
            val colors =
                mapOf(
                    "background" to hexOf(scheme.surface),
                    "foreground" to hexOf(scheme.onSurface),
                    "muted" to hexOf(scheme.onSurfaceVariant),
                    "buttonBackground" to hexOf(scheme.secondaryContainer),
                    "buttonForeground" to hexOf(scheme.onSecondaryContainer),
                    "buttonBorder" to hexOf(scheme.outlineVariant),
                )
            // Plain JSON: configuration is opaque to the bridge.
            buildString {
                append("{\"access\":{\"httpBase\":")
                append(jsonString(access.httpBase))
                append(",\"wsBase\":")
                append(jsonString(access.wsBase))
                append(",\"query\":")
                append(
                    access.query.entries.joinToString(",", "{", "}") {
                        jsonString(it.key) + ":" + jsonString(it.value)
                    },
                )
                append(",\"credentials\":false}")
                append(",\"platform\":")
                append(jsonString(platform))
                append(",\"deviceId\":")
                append(jsonString(deviceId))
                append(",\"colors\":")
                append(colors.entries.joinToString(",", "{", "}") { jsonString(it.key) + ":" + jsonString(it.value) })
                append("}")
            }
        }
    val document =
        remember(configuration) {
            val script =
                runCatching {
                        context.assets.open("device-stream.js").bufferedReader().use { it.readText() }
                    }
                    .getOrElse {
                        // A missing bundle fails like a crashed page, so the
                        // overlay path carries it rather than a blank view.
                        ""
                    }
            deviceStreamDocument(configuration, script)
        }

    DisposableEffect(Unit) { onDispose { webViewRef(null) } }

    // `key(attempt)`/`key(configuration)` on RN — the stream remounts on retry
    // or a fresh ticket rather than re-navigating the same view.
    androidx.compose.runtime.key(attempt, configuration) {
    AndroidView(
        modifier = Modifier.fillMaxSize(),
        factory = { viewContext ->
            WebView(viewContext).apply {
                settings.javaScriptEnabled = true
                settings.domStorageEnabled = true
                settings.mediaPlaybackRequiresUserGesture = false
                // The page is loaded under a placeholder origin; hub traffic is
                // absolute, and RN allows the same cross-origin reach.
                settings.allowUniversalAccessFromFileURLs = true
                webViewClient =
                    object : android.webkit.WebViewClient() {
                        override fun shouldOverrideUrlLoading(
                            view: WebView,
                            request: android.webkit.WebResourceRequest,
                        ): Boolean = request.url.toString() != StreamBaseUrl

                        override fun onRenderProcessGone(
                            view: WebView,
                            detail: android.webkit.RenderProcessGoneDetail,
                        ): Boolean {
                            // One automatic recovery, matching RN's processRetried.
                            error = "Device viewer stopped. Reconnect to try again."
                            status = "error"
                            return true
                        }
                    }
                addJavascriptInterface(
                    StreamBridge { data ->
                        val message = deviceStreamMessage(data) ?: return@StreamBridge
                        when (message) {
                            is StreamMessage.Unauthorized -> onUnauthorized()
                            is StreamMessage.Input -> onBridgeMessage(message)
                            is StreamMessage.Retry -> onBridgeMessage(message)
                            is StreamMessage.Status -> {
                                when (message.status) {
                                    "error" -> {
                                        error = message.detail ?: "Device stream failed."
                                        status = "error"
                                    }
                                    else -> status = message.status
                                }
                            }
                        }
                    },
                    "ReactNativeWebView",
                )
                loadDataWithBaseURL(StreamBaseUrl, document, "text/html", "utf-8", null)
                webViewRef(this)
            }
        },
    )
    }

    if (status != "streaming") {
        Column(
            Modifier.fillMaxSize().padding(S5Theme.spacing.gutter),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            if (status == "connecting") CircularProgressIndicator()
            Text(
                if (status == "error") error.orEmpty() else "Connecting to device...",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (status == "error") {
                TextButton(onClick = { onBridgeMessage(StreamMessage.Retry) }) { Text("Reconnect") }
            }
        }
    }
}

/** The page's own origin; navigation away from it is blocked. */
private const val StreamBaseUrl = "https://device-stream.t3.invalid/"

private fun jsonString(value: String): String =
    buildString {
        append('"')
        for (char in value) {
            when (char) {
                '"', '\\' -> append('\\').append(char)
                '\n' -> append("\\n")
                '\r' -> append("\\r")
                '\t' -> append("\\t")
                else ->
                    if (char < ' ') append(String.format("\\u%04x", char.code)) else append(char)
            }
        }
        append('"')
    }
