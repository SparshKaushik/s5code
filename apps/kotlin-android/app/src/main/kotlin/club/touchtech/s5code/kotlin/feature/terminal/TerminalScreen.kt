package club.touchtech.s5code.kotlin.feature.terminal

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.Keyboard
import androidx.compose.material.icons.rounded.KeyboardHide
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Terminal
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SmallFloatingActionButton
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
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
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.S5WaitState
import club.touchtech.s5code.kotlin.design.component.rememberClipboardWriter
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.feature.connections.showRetry
import club.touchtech.s5code.kotlin.feature.connections.waitNotice
import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.TerminalStatus
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.platform.terminal.S5TerminalView
import club.touchtech.s5code.kotlin.platform.terminal.resolveTerminalTheme
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex

/**
 * Native Ghostty terminal surface with complete VT, alternate-screen, and TUI
 * rendering, mirroring `ThreadTerminalRouteScreen.tsx` in the RN client.
 *
 * There is deliberately no command field: input goes through the terminal's own
 * hidden input view (soft keyboard, hardware keys, IME commit), and the bottom
 * bar is the same key accessory RN docks above the keyboard — esc, a one-shot
 * modifier pair for the host platform, tab, paste, clear, arrows, and the four
 * literal keys a phone keyboard makes awkward.
 */
@Composable
fun TerminalScreen(
    store: AppStore,
    environmentId: String,
    threadId: String,
    terminalId: String? = null,
    onBack: () -> Unit,
    /** RN `StackActions.replace` to another session: the dead screen leaves the stack. */
    onSwitchTerminal: (String) -> Unit,
    /** Fallback when no live session remains after an exit. */
    onExitToThread: () -> Unit,
    confirmController: S5ConfirmDialogController,
) {
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val id = remember(threadId) { ThreadId(threadId) }
    val activeTerminalId = terminalId ?: DEFAULT_TERMINAL_ID
    val scope = rememberCoroutineScope()
    val copyToClipboard = rememberClipboardWriter()
    val clipboard = LocalClipboardManager.current
    val preferences by store.preferences.collectAsStateWithLifecycle()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val threads by store.workspace.threads.collectAsStateWithLifecycle()
    val environment = environments.firstOrNull { it.id == env }
    val thread = threads.firstOrNull { it.environmentId == env && it.id == id }
    val workspaceRoot = projects.firstOrNull { it.id == thread?.projectId }?.workspaceRoot
    val hostPlatform =
        remember(environment?.platformOs, environment?.label) {
            hostPlatformOf(environment?.platformOs, environment?.label)
        }
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

    var failure by remember(threadId, activeTerminalId) { mutableStateOf<String?>(null) }
    // The cached grid sizes the attach RPC before the first measure lands, so a
    // revisited terminal opens at the right size instead of resizing a frame
    // later (`getCachedTerminalGridSize` in the RN client).
    var attachGrid by
        remember(environmentId, threadId, activeTerminalId) {
            mutableStateOf(cachedTerminalGridSizes["$environmentId/$threadId/$activeTerminalId"])
        }
    var grid by remember(threadId, activeTerminalId) { mutableStateOf<Pair<Int, Int>?>(null) }
    var menuExpanded by remember { mutableStateOf(false) }
    var nativeView by remember(threadId, activeTerminalId) { mutableStateOf<S5TerminalView?>(null) }
    // The armed toolbar modifier, keyed like RN so switching sessions drops it.
    var pendingModifier by remember(activeTerminalId) { mutableStateOf<PendingModifier?>(null) }
    // Serializes clipboard reads and writes for the currently attached pty —
    // `createTerminalPasteSession` in the RN client.
    val pasteMutex = remember(activeTerminalId) { Mutex() }

    val knownSessions by
        remember(environmentId) { store.workspace.terminalSessions(env) }
            .collectAsStateWithLifecycle(emptyList())

    val session by
        remember(environmentId, threadId, activeTerminalId, attachGrid) {
            val measured = attachGrid
            if (measured == null) {
                kotlinx.coroutines.flow.flowOf(null)
            } else {
                store.workspace.terminal(env, id, activeTerminalId, measured.first, measured.second)
            }
        }
            .collectAsStateWithLifecycle(null)

    val menuSessions =
        remember(knownSessions, session, threadId, workspaceRoot) {
            buildTerminalMenuSessions(
                knownSessions = knownSessions,
                threadId = threadId,
                workspaceRoot = workspaceRoot,
                current = session?.toMenuSession(),
            )
        }

    // A bare `/terminal` route that finds another live session follows it, the
    // same `shouldRedirectToRunningTerminal` the RN screen redirects on.
    LaunchedEffect(terminalId, menuSessions) {
        if (terminalId == null) {
            pickRunningTerminalSession(menuSessions)
                ?.takeIf { it.terminalId != activeTerminalId }
                ?.let { onSwitchTerminal(it.terminalId) }
        }
    }

    fun reportFailure(fallback: String, cause: Throwable) {
        failure = cause.message ?: fallback
        store.showError(failure!!)
    }

    /** RN `writeInput`: the pty only accepts writes while the shell runs. */
    fun write(data: String): Boolean {
        if (data.isEmpty() || session?.status != TerminalStatus.Running) return false
        failure = null
        scope.launch {
            runCatching { store.workspace.terminalWrite(env, id, activeTerminalId, data) }
                .onFailure { reportFailure("The write was refused.", it) }
        }
        return true
    }

    fun pasteFromClipboard() {
        pendingModifier = null
        scope.launch {
            // Serialized: a second paste queues behind the first rather than
            // interleaving chunks on the wire.
            pasteMutex.lock()
            try {
                val text = clipboard.getText()?.text ?: return@launch
                for (chunk in chunkTerminalWrite(encodeTerminalPaste(text))) {
                    if (!write(chunk)) break
                }
            } finally {
                pasteMutex.unlock()
            }
        }
    }

    /** Sends a key through the armed toolbar modifier, if any, and disarms it. */
    fun writeModifiedInput(data: String) {
        val modifier = pendingModifier
        if (modifier == null) {
            write(data)
            return
        }
        pendingModifier = null
        when (val resolved = resolveModifiedTerminalInput(data, modifier, hostPlatform)) {
            ModifiedTerminalInput.Paste -> pasteFromClipboard()
            is ModifiedTerminalInput.Write -> write(resolved.data)
        }
    }

    fun restart() {
        val size = grid ?: return
        scope.launch {
            runCatching {
                    store.workspace.terminalRestart(
                        env,
                        id,
                        activeTerminalId,
                        size.first,
                        size.second,
                    )
                }
                .onFailure { reportFailure("The terminal could not be restarted.", it) }
        }
    }

    fun clear() {
        pendingModifier = null
        scope.launch {
            runCatching { store.workspace.terminalClear(env, id, activeTerminalId) }
                .onFailure { reportFailure("The terminal could not be cleared.", it) }
        }
    }

    fun setFontSize(next: Float) {
        store.updatePreferences { prefs ->
            prefs.copy(terminalScale = next / S5TerminalView.DEFAULT_FONT_SIZE_SP)
        }
    }

    // RN closes the session and falls through to the previous live terminal (or
    // back to the thread) when the shell this screen attached to ends on its
    // watch — `navigateAwayAfterExit`. A session that was already dead on attach
    // does not trigger it: the user reopened that transcript on purpose.
    var wasRunning by remember(activeTerminalId) { mutableStateOf(false) }
    var exitHandled by remember(activeTerminalId) { mutableStateOf(false) }
    LaunchedEffect(session?.status, session?.id) {
        val live = session?.takeIf { it.id == activeTerminalId } ?: return@LaunchedEffect
        if (live.status == TerminalStatus.Running) {
            wasRunning = true
            exitHandled = false
            return@LaunchedEffect
        }
        if (
            wasRunning &&
                !exitHandled &&
                (live.status == TerminalStatus.Exited || live.status == TerminalStatus.Closed)
        ) {
            wasRunning = false
            exitHandled = true
            runCatching {
                store.workspace.terminalClose(env, id, activeTerminalId)
            }
            val fallback = previousLiveTerminalId(menuSessions, activeTerminalId)
            if (fallback != null) onSwitchTerminal(fallback) else onExitToThread()
        }
    }

    val environmentNotice =
        waitNotice(
            states = listOfNotNull(environment?.state),
            environmentLabel = environment?.label,
            resourceName = "terminal",
            hasContent = session != null,
            awaitingEnvironments = environment == null,
        )

    S5Screen(
        title = session?.title?.takeIf { it.isNotBlank() } ?: terminalLabel(activeTerminalId),
        subtitle = session?.cwd.orEmpty(),
        prominence = S5TopBarProminence.Compact,
        onBack = onBack,
        actions = {
            val menuOptions =
                buildList<S5MenuOption> {
                    add(
                        S5MenuOption(
                            id = "text-size",
                            label = "Text size",
                            children =
                                listOf(
                                    S5MenuOption(
                                        id = "font-decrease",
                                        label =
                                            "A- ${"%.1f".format(
                                                (fontSize - TERMINAL_FONT_SIZE_STEP).coerceAtLeast(
                                                    S5TerminalView.MIN_FONT_SIZE_SP,
                                                ),
                                            )} pt",
                                        enabled = fontSize > S5TerminalView.MIN_FONT_SIZE_SP,
                                    ),
                                    S5MenuOption(
                                        id = "font-increase",
                                        label =
                                            "A+ ${"%.1f".format(
                                                (fontSize + TERMINAL_FONT_SIZE_STEP).coerceAtMost(
                                                    S5TerminalView.MAX_FONT_SIZE_SP,
                                                ),
                                            )} pt",
                                        enabled = fontSize < S5TerminalView.MAX_FONT_SIZE_SP,
                                    ),
                                ),
                        ),
                    )
                    menuSessions.forEach { menuSession ->
                        add(
                            S5MenuOption(
                                id = "terminal-session:${menuSession.terminalId}",
                                label = menuSession.displayLabel,
                                supporting =
                                    listOfNotNull(
                                            terminalStatusLabel(
                                                menuSession.status,
                                                menuSession.hasRunningSubprocess,
                                            ),
                                            basename(menuSession.cwd),
                                        )
                                        .joinToString(" · "),
                                selected = menuSession.terminalId == activeTerminalId,
                            ),
                        )
                    }
                    add(
                        S5MenuOption(
                            id = "terminal-new",
                            label = "Open new terminal",
                            icon = Icons.Rounded.Terminal,
                            supporting =
                                "Start another shell in ${basename(workspaceRoot) ?: "this workspace"}",
                        ),
                    )
                    add(S5MenuOption("copy-all", "Copy all", Icons.Rounded.ContentCopy))
                    add(
                        S5MenuOption(
                            "restart",
                            "Restart",
                            Icons.Rounded.Refresh,
                            destructive = true,
                        ),
                    )
                }
            S5OverflowMenu(
                icon = Icons.Rounded.MoreVert,
                label = "Terminal actions",
                expanded = menuExpanded,
                onExpandedChange = { menuExpanded = it },
                options = menuOptions,
                onSelect = { action ->
                    when {
                        action == "font-decrease" ->
                            setFontSize(fontSize - TERMINAL_FONT_SIZE_STEP)
                        action == "font-increase" ->
                            setFontSize(fontSize + TERMINAL_FONT_SIZE_STEP)
                        action == "terminal-new" ->
                            onSwitchTerminal(
                                nextOpenTerminalId(
                                    listedTerminalIds = menuSessions.map { it.terminalId },
                                    activeRouteTerminalId = activeTerminalId,
                                ),
                            )
                        action.startsWith("terminal-session:") ->
                            onSwitchTerminal(action.removePrefix("terminal-session:"))
                        action == "copy-all" -> {
                            val output = nativeView?.copyAllText().orEmpty()
                            if (output.isNotEmpty()) copyToClipboard(output)
                        }
                        action == "restart" ->
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
            // The keyboard accessory, docked to the IME like RN's
            // KeyboardStickyView: it sits above the keyboard when that is open
            // and on the nav bar when it is not.
            Surface(color = Color(terminalTheme.background)) {
                Row(
                    Modifier.fillMaxWidth()
                        .imePadding()
                        .navigationBarsPadding()
                        .horizontalScroll(rememberScrollState())
                        .padding(
                            horizontal = S5Theme.spacing.small,
                            vertical = S5Theme.spacing.tiny,
                        ),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
                ) {
                    terminalToolbarActions(hostPlatform).forEach { action ->
                        val active =
                            action.kind == TerminalToolbarKind.Modifier &&
                                pendingModifier == action.modifier
                        S5Button(
                            text =
                                if (action.kind == TerminalToolbarKind.Modifier ||
                                    action.kind == TerminalToolbarKind.Clear
                                ) {
                                    action.label.uppercase()
                                } else {
                                    action.label
                                },
                            onClick = {
                                when (action.kind) {
                                    TerminalToolbarKind.Modifier ->
                                        pendingModifier =
                                            if (pendingModifier == action.modifier) null
                                            else action.modifier
                                    TerminalToolbarKind.Clear -> clear()
                                    TerminalToolbarKind.Paste -> pasteFromClipboard()
                                    TerminalToolbarKind.Send ->
                                        writeModifiedInput(action.data.orEmpty())
                                }
                            },
                            emphasis =
                                if (active) S5ActionEmphasis.Prominent else S5ActionEmphasis.Secondary,
                            style = S5ButtonStyle.Tonal,
                        )
                    }
                    S5IconButton(
                        icon = Icons.Rounded.KeyboardHide,
                        label = "Dismiss keyboard",
                        onClick = { nativeView?.hideKeyboard() },
                    )
                }
            }
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            androidx.compose.foundation.layout.Column(Modifier.fillMaxSize()) {
                (failure ?: session?.error)?.let { message ->
                    Box(
                        Modifier.padding(
                            horizontal = S5Theme.spacing.gutter,
                            vertical = S5Theme.spacing.small,
                        ),
                    ) {
                        S5ErrorState(
                            title =
                                if (failure != null) "Terminal action failed"
                                else "The shell reported an error",
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
                            view.onInput = ::writeModifiedInput
                            view.onResize = { cols, rows ->
                                val next = cols to rows
                                val previous = grid
                                if (next != previous) {
                                    grid = next
                                    cachedTerminalGridSizes[
                                        "$environmentId/$threadId/$activeTerminalId"
                                    ] = next
                                    if (attachGrid == null) {
                                        attachGrid = next
                                    } else if (previous != null && session?.status?.live == true) {
                                        scope.launch {
                                            runCatching {
                                                store.workspace.terminalResize(
                                                    env,
                                                    id,
                                                    activeTerminalId,
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
                        view.onInput = ::writeModifiedInput
                        view.fontSizeSp = fontSize
                        view.setTheme(terminalTheme)
                        session?.let { view.setReplayBuffer(it.buffer) }
                    },
                    onRelease = { it.release() },
                    modifier = Modifier.fillMaxSize(),
                )
                if (grid != null && session == null && environmentNotice == null) {
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
                environmentNotice?.let { notice ->
                    S5WaitState(
                        title = notice.title,
                        detail = notice.detail,
                        icon = Icons.Rounded.Terminal,
                        spinning = notice.spinning,
                        actionLabel = if (notice.showRetry) "Retry now" else null,
                        onAction = { store.retryEnvironment(env) },
                        modifier = Modifier.fillMaxSize(),
                    )
                }
            }

            // RN's floating show-keyboard affordance, visible whenever the soft
            // keyboard is down.
            if (!imeVisible()) {
                SmallFloatingActionButton(
                    onClick = { nativeView?.showKeyboard() },
                    modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp),
                    containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                ) {
                    androidx.compose.material3.Icon(
                        Icons.Rounded.Keyboard,
                        contentDescription = "Show keyboard",
                    )
                }
            }
        }
    }

    DisposableEffect(threadId, activeTerminalId) {
        onDispose {
            nativeView?.release()
            nativeView = null
        }
    }
}

/** Grid sizes remembered per session for the process lifetime. */
private val cachedTerminalGridSizes = mutableMapOf<String, Pair<Int, Int>>()

/** `TERMINAL_FONT_SIZE_STEP` in the RN client's appearance preferences. */
private const val TERMINAL_FONT_SIZE_STEP = 0.5f

@Composable
private fun imeVisible(): Boolean {
    val density = LocalDensity.current
    return WindowInsets.ime.getBottom(density) > 0
}

private fun terminalEmptyTitle(status: TerminalStatus) =
    when (status) {
        TerminalStatus.Exited -> "The shell exited"
        TerminalStatus.Error -> "The shell could not start"
        else -> "No output yet"
    }

/** One key accessory button, from `TerminalToolbarAction` in the RN screen. */
private enum class TerminalToolbarKind {
    Send,
    Clear,
    Paste,
    Modifier,
}

private data class TerminalToolbarAction(
    val kind: TerminalToolbarKind,
    val label: String,
    val data: String? = null,
    val modifier: PendingModifier? = null,
)

/**
 * The RN toolbar order verbatim: esc, the host's modifier pair (cmd+ctrl on a
 * mac, ctrl+alt elsewhere), tab, paste, clear, arrows, then ~ | / -.
 */
private fun terminalToolbarActions(hostPlatform: HostPlatform): List<TerminalToolbarAction> {
    val modifiers =
        if (hostPlatform == HostPlatform.Mac) {
            listOf(
                TerminalToolbarAction(TerminalToolbarKind.Modifier, "cmd", modifier = PendingModifier.Meta),
                TerminalToolbarAction(TerminalToolbarKind.Modifier, "ctrl", modifier = PendingModifier.Ctrl),
            )
        } else {
            listOf(
                TerminalToolbarAction(TerminalToolbarKind.Modifier, "ctrl", modifier = PendingModifier.Ctrl),
                TerminalToolbarAction(TerminalToolbarKind.Modifier, "alt", modifier = PendingModifier.Meta),
            )
        }
    return buildList {
        add(TerminalToolbarAction(TerminalToolbarKind.Send, "esc", ""))
        addAll(modifiers)
        add(TerminalToolbarAction(TerminalToolbarKind.Send, "tab", "\t"))
        add(TerminalToolbarAction(TerminalToolbarKind.Paste, "paste"))
        add(TerminalToolbarAction(TerminalToolbarKind.Clear, "clear"))
        add(TerminalToolbarAction(TerminalToolbarKind.Send, "↑", "[A"))
        add(TerminalToolbarAction(TerminalToolbarKind.Send, "↓", "[B"))
        add(TerminalToolbarAction(TerminalToolbarKind.Send, "←", "[D"))
        add(TerminalToolbarAction(TerminalToolbarKind.Send, "→", "[C"))
        add(TerminalToolbarAction(TerminalToolbarKind.Send, "~", "~"))
        add(TerminalToolbarAction(TerminalToolbarKind.Send, "|", "|"))
        add(TerminalToolbarAction(TerminalToolbarKind.Send, "/", "/"))
        add(TerminalToolbarAction(TerminalToolbarKind.Send, "-", "-"))
    }
}
