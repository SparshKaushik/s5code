package club.touchtech.s5code.kotlin.feature.terminal

import android.view.KeyEvent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Send
import androidx.compose.material.icons.rounded.ClearAll
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.KeyboardHide
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Terminal
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.DEFAULT_TERMINAL_ID
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogRequest
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5ErrorState
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5LoadingState
import club.touchtech.s5code.kotlin.design.component.S5MenuOption
import club.touchtech.s5code.kotlin.design.component.S5OverflowMenu
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5TextField
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.rememberClipboardWriter
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.TerminalStatus
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.platform.terminal.S5TerminalView
import club.touchtech.s5code.kotlin.platform.terminal.resolveTerminalTheme
import kotlinx.coroutines.launch

/** Native Ghostty terminal surface with complete VT, alternate-screen, and TUI rendering. */
@Composable
fun TerminalScreen(
    store: AppStore,
    environmentId: String,
    threadId: String,
    onBack: () -> Unit,
    confirmController: S5ConfirmDialogController,
) {
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val id = remember(threadId) { ThreadId(threadId) }
    val scope = rememberCoroutineScope()
    val copyToClipboard = rememberClipboardWriter()
    val preferences by store.preferences.collectAsStateWithLifecycle()
    val isAppDark = MaterialTheme.colorScheme.background.luminance() < 0.5f
    val terminalTheme =
        remember(preferences.terminalTheme, isAppDark) {
            resolveTerminalTheme(preferences.terminalTheme.name, isAppDark)
        }
    val fontSize =
        remember(preferences.terminalScale) {
            (S5TerminalView.DEFAULT_FONT_SIZE_SP * preferences.terminalScale)
                .coerceIn(S5TerminalView.MIN_FONT_SIZE_SP, S5TerminalView.MAX_FONT_SIZE_SP)
        }

    var input by remember(threadId) { mutableStateOf("") }
    var failure by remember(threadId) { mutableStateOf<String?>(null) }
    var attachGrid by remember(threadId) { mutableStateOf<Pair<Int, Int>?>(null) }
    var grid by remember(threadId) { mutableStateOf<Pair<Int, Int>?>(null) }
    var menuExpanded by remember { mutableStateOf(false) }
    var nativeView by remember(threadId) { mutableStateOf<S5TerminalView?>(null) }

    val session by
        remember(environmentId, threadId, attachGrid) {
            val measured = attachGrid
            if (measured == null) kotlinx.coroutines.flow.flowOf(null)
            else store.workspace.terminal(env, id, DEFAULT_TERMINAL_ID, measured.first, measured.second)
        }
            .collectAsStateWithLifecycle(null)

    fun reportFailure(fallback: String, cause: Throwable) {
        failure = cause.message ?: fallback
        store.showError(failure!!)
    }

    fun write(data: String) {
        if (data.isEmpty()) return
        failure = null
        scope.launch {
            runCatching { store.workspace.terminalWrite(env, id, DEFAULT_TERMINAL_ID, data) }
                .onFailure { reportFailure("The write was refused.", it) }
        }
    }

    fun restart() {
        val size = grid ?: return
        scope.launch {
            runCatching {
                    store.workspace.terminalRestart(
                        env,
                        id,
                        DEFAULT_TERMINAL_ID,
                        size.first,
                        size.second,
                    )
                }
                .onFailure { reportFailure("The terminal could not be restarted.", it) }
        }
    }

    S5Screen(
        title = "Terminal",
        subtitle = session?.cwd.orEmpty(),
        prominence = S5TopBarProminence.Compact,
        onBack = onBack,
        actions = {
            S5OverflowMenu(
                icon = Icons.Rounded.MoreVert,
                label = "Terminal actions",
                expanded = menuExpanded,
                onExpandedChange = { menuExpanded = it },
                options =
                    listOf(
                        S5MenuOption("clear", "Clear", Icons.Rounded.ClearAll),
                        S5MenuOption("copy-all", "Copy all", Icons.Rounded.ContentCopy),
                        S5MenuOption("restart", "Restart", Icons.Rounded.Refresh, destructive = true),
                    ),
                onSelect = { action ->
                    when (action) {
                        "clear" ->
                            scope.launch {
                                runCatching {
                                        store.workspace.terminalClear(env, id, DEFAULT_TERMINAL_ID)
                                    }
                                    .onFailure {
                                        reportFailure("The terminal could not be cleared.", it)
                                    }
                            }
                        "copy-all" -> {
                            val output = nativeView?.copyAllText().orEmpty()
                            if (output.isNotEmpty()) copyToClipboard(output)
                        }
                        "restart" ->
                            confirmController.show(
                                S5ConfirmDialogRequest(
                                    title = "Restart terminal?",
                                    message = "The running shell and any foreground command will be stopped.",
                                    confirmText = "Restart",
                                    destructive = true,
                                    onConfirm = ::restart,
                                )
                            )
                    }
                },
            )
            S5IconButton(
                icon = Icons.Rounded.KeyboardHide,
                label = "Hide keyboard",
                onClick = { nativeView?.hideKeyboard() },
            )
        },
        bottomBar = {
            Surface(color = MaterialTheme.colorScheme.surfaceContainer, tonalElevation = 2.dp) {
                Column(Modifier.imePadding().navigationBarsPadding().padding(S5Theme.spacing.small)) {
                    Row(
                        Modifier.fillMaxWidth().padding(bottom = S5Theme.spacing.tiny),
                        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
                    ) {
                        TERMINAL_KEYS.forEach { key ->
                            S5Button(
                                text = key.label,
                                onClick = { nativeView?.sendKey(key.keyCode, key.metaState) },
                                emphasis = S5ActionEmphasis.Secondary,
                                style = S5ButtonStyle.Tonal,
                                enabled = session != null,
                            )
                        }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                        S5TextField(
                            value = input,
                            onValueChange = { input = it },
                            placeholder = "Command",
                            singleLine = true,
                            textStyle = S5Theme.code.terminal,
                            modifier = Modifier.weight(1f),
                        )
                        S5Button(
                            text = "Run",
                            onClick = {
                                val command = input
                                input = ""
                                write(command + "\r")
                            },
                            icon = Icons.AutoMirrored.Rounded.Send,
                            emphasis = S5ActionEmphasis.Prominent,
                            enabled = input.isNotBlank() && session != null,
                        )
                    }
                }
            }
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            (failure ?: session?.error)?.let { message ->
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.small)) {
                    S5ErrorState(
                        title = if (failure != null) "Terminal action failed" else "The shell reported an error",
                        detail = message,
                        onRetry = { failure = null },
                        retryLabel = "Dismiss",
                    )
                }
            }
            AndroidView(
                factory = { context ->
                    S5TerminalView(context).also { view ->
                        nativeView = view
                        view.onInput = ::write
                        view.onResize = { cols, rows ->
                            val next = cols to rows
                            val previous = grid
                            if (next != previous) {
                                grid = next
                                if (attachGrid == null) {
                                    attachGrid = next
                                } else if (previous != null) {
                                    scope.launch {
                                        runCatching {
                                            store.workspace.terminalResize(
                                                env,
                                                id,
                                                DEFAULT_TERMINAL_ID,
                                                cols,
                                                rows,
                                            )
                                        }
                                    }
                                }
                            }
                        }
                        view.fontSizeSp = fontSize
                        view.setTheme(terminalTheme)
                    }
                },
                update = { view ->
                    nativeView = view
                    view.onInput = ::write
                    view.fontSizeSp = fontSize
                    view.setTheme(terminalTheme)
                    session?.let { view.setReplayBuffer(it.buffer) }
                },
                onRelease = { it.release() },
                modifier = Modifier.fillMaxSize(),
            )
            if (grid != null && session == null) {
                S5LoadingState("Opening the terminal…", Modifier.fillMaxSize())
            }
            if (session?.buffer.isNullOrEmpty() && session?.status?.live == false) {
                S5EmptyState(
                    icon = Icons.Rounded.Terminal,
                    title = terminalEmptyTitle(session!!.status),
                    detail = "Restart the session to run commands on the machine.",
                    modifier = Modifier.fillMaxSize(),
                )
            }
        }
    }

    DisposableEffect(threadId) {
        onDispose {
            nativeView?.release()
            nativeView = null
        }
    }
}

private fun terminalEmptyTitle(status: TerminalStatus) =
    when (status) {
        TerminalStatus.Exited -> "The shell exited"
        TerminalStatus.Error -> "The shell could not start"
        else -> "No output yet"
    }

private data class ToolbarKey(val label: String, val keyCode: Int, val metaState: Int = 0)

private val TERMINAL_KEYS =
    listOf(
        ToolbarKey("esc", KeyEvent.KEYCODE_ESCAPE),
        ToolbarKey("tab", KeyEvent.KEYCODE_TAB),
        ToolbarKey("ctrl-c", KeyEvent.KEYCODE_C, KeyEvent.META_CTRL_ON),
        ToolbarKey("ctrl-d", KeyEvent.KEYCODE_D, KeyEvent.META_CTRL_ON),
        ToolbarKey("↑", KeyEvent.KEYCODE_DPAD_UP),
        ToolbarKey("↓", KeyEvent.KEYCODE_DPAD_DOWN),
    )
