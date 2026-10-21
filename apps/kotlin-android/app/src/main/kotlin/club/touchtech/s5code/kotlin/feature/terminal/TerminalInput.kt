package club.touchtech.s5code.kotlin.feature.terminal

import club.touchtech.s5code.kotlin.data.DEFAULT_TERMINAL_ID
import club.touchtech.s5code.kotlin.model.TerminalSession
import club.touchtech.s5code.kotlin.model.TerminalStatus
import club.touchtech.s5code.kotlin.model.TerminalSummary

/**
 * Terminal input encoding and session-menu logic, ported from
 * `apps/mobile/src/features/terminal/terminalInput.ts`, `terminalMenu.ts`, and
 * `packages/shared/src/terminalLabels.ts`. Kept android-free so it is testable
 * on the JVM.
 */

/** Upper bound of `TerminalWriteInput.data`; longer writes are rejected by the server. */
const val TERMINAL_WRITE_MAX_LENGTH = 65_536

enum class PendingModifier {
    Ctrl,
    Meta,
}

enum class HostPlatform {
    Mac,
    Linux,
    Windows,
    Unknown,
}

sealed interface ModifiedTerminalInput {
    data class Write(val data: String) : ModifiedTerminalInput
    /** The toolbar chord asked for the device clipboard, not a control byte. */
    data object Paste : ModifiedTerminalInput
}

// C0 controls other than tab, LF, and CR, plus DEL.
private val UNSAFE_PASTE_BYTES = Regex("[\\u0000-\\u0008\\u000b\\u000c\\u000e-\\u001f\\u007f]")

private const val ESC = "\u001b"

/**
 * Encodes a key pressed while the toolbar's one-shot ctrl modifier is armed
 * into the control byte a terminal expects. Ported from `applyCtrlModifier`.
 */
fun applyCtrlModifier(input: String): String {
    val first = input.firstOrNull() ?: return input
    val lower = first.lowercaseChar()
    if (lower in 'a'..'z') {
        return (lower.code - 96).toChar().toString()
    }
    return when (first) {
        '@' -> "\u0000"
        '[' -> ESC
        '\\' -> "\u001c"
        ']' -> "\u001d"
        '^' -> "\u001e"
        '_' -> "\u001f"
        '?' -> "\u007f"
        else -> input
    }
}

/**
 * Resolves what a keypress means once a toolbar modifier is armed. The host's
 * paste chord (cmd+v on a macOS host, ctrl+v elsewhere) pastes the device
 * clipboard instead of reaching the remote shell as a raw control byte, which
 * matches what the web terminal does with the same chord. Ported from
 * `resolveModifiedTerminalInput`.
 */
fun resolveModifiedTerminalInput(
    data: String,
    modifier: PendingModifier,
    hostPlatform: HostPlatform,
): ModifiedTerminalInput {
    val pasteModifier = if (hostPlatform == HostPlatform.Mac) PendingModifier.Meta else PendingModifier.Ctrl
    if (modifier == pasteModifier && data.lowercase() == "v") {
        return ModifiedTerminalInput.Paste
    }
    return ModifiedTerminalInput.Write(
        if (modifier == PendingModifier.Ctrl) applyCtrlModifier(data) else ESC + data,
    )
}

/**
 * Encodes clipboard text for the remote pty the way the web terminal does when
 * bracketed paste is off: unsafe control bytes become spaces (which also
 * defuses an embedded bracketed-paste end marker, since its ESC goes too) and
 * line breaks become carriage returns. Ported from `encodeTerminalPaste`.
 */
fun encodeTerminalPaste(text: String): String =
    text.replace(UNSAFE_PASTE_BYTES, " ").replace("\r\n", "\r").replace("\n", "\r")

/**
 * Splits terminal input into writes the wire contract accepts, never cutting
 * through a surrogate pair so every chunk stays valid UTF-16.
 */
fun chunkTerminalWrite(data: String): List<String> {
    val chunks = mutableListOf<String>()
    var start = 0
    while (start < data.length) {
        var end = minOf(start + TERMINAL_WRITE_MAX_LENGTH, data.length)
        if (end < data.length && data[end - 1].isHighSurrogate()) {
            end -= 1
        }
        chunks.add(data.substring(start, end))
        start = end
    }
    return chunks
}

/**
 * Maps the environment's reported OS (or, failing that, its label) onto the
 * toolbar's host layout. `hostPlatformFromOs` + `inferHostPlatform` in the RN
 * screen; Unknown falls back to the non-mac modifier pair, same as RN.
 */
fun hostPlatformOf(os: String?, environmentLabel: String?): HostPlatform {
    when (os) {
        "darwin" -> return HostPlatform.Mac
        "linux" -> return HostPlatform.Linux
        "windows" -> return HostPlatform.Windows
    }
    val value = environmentLabel?.lowercase().orEmpty()
    return when {
        value.contains("mac") || value.contains("darwin") || value.contains("imac") ->
            HostPlatform.Mac
        value.contains("windows") || value.contains("win") -> HostPlatform.Windows
        value.contains("linux") || value.contains("ubuntu") || value.contains("debian") ->
            HostPlatform.Linux
        else -> HostPlatform.Unknown
    }
}

/** `getTerminalLabel`: human-readable label for a terminal tab. */
fun terminalLabel(terminalId: String): String {
    val numericSuffix = Regex("^term(?:inal)?-(\\d+)$", RegexOption.IGNORE_CASE)
        .find(terminalId)
        ?.groupValues
        ?.get(1)
    return if (numericSuffix != null) "Terminal $numericSuffix" else terminalId
}

/** `resolveTerminalSessionLabel`: the server's label wins over the id-derived one. */
fun terminalSessionLabel(terminalId: String, serverLabel: String?): String =
    serverLabel?.trim()?.takeIf { it.isNotEmpty() } ?: terminalLabel(terminalId)

/**
 * Client-side terminal id allocator. Ids are always chosen by the client; the
 * lowest unused `term-N` wins. Ported from `nextTerminalId`.
 */
fun nextTerminalId(existingTerminalIds: List<String>): String {
    val used = existingTerminalIds.filter { it.isNotBlank() }.toSet()
    var index = 1
    while ("term-$index" in used) index += 1
    return "term-$index"
}

/**
 * `nextOpenTerminalId`: counts the terminal screen already mounted as occupied
 * so an empty session list on the primary route still advances to `term-2`
 * instead of re-opening the same tab.
 */
fun nextOpenTerminalId(listedTerminalIds: List<String>, activeRouteTerminalId: String?): String {
    val listed = listedTerminalIds.filter { it.isNotBlank() }
    val routeId = activeRouteTerminalId?.takeIf { it.isNotBlank() }
    if (routeId == null || routeId in listed) {
        return nextTerminalId(listed)
    }
    return nextTerminalId(listed + routeId)
}

/** The toolbar/menu row for one known session, from `TerminalMenuSession`. */
data class TerminalMenuSession(
    val terminalId: String,
    val cwd: String?,
    val status: TerminalStatus,
    val hasRunningSubprocess: Boolean,
    val displayLabel: String,
    val updatedAt: String?,
)

/** Numeric-aware ordering: `term-2` sorts before `term-10` (`numeric: true` collation). */
private val TERMINAL_ID_COMPARATOR =
    compareBy<TerminalMenuSession>({ terminalIdSortKey(it.terminalId) }, { it.terminalId })

private fun terminalIdSortKey(terminalId: String): Int =
    Regex("^(?:term(?:inal)?-)?(\\d+)$", RegexOption.IGNORE_CASE)
        .find(terminalId)
        ?.groupValues
        ?.get(1)
        ?.toIntOrNull()
        ?: Int.MAX_VALUE

fun basename(path: String?): String? {
    if (path == null) return null
    val normalized = path.trimEnd('/')
    if (normalized.isEmpty()) return "/"
    return normalized.substringAfterLast('/')
}

/** `getTerminalStatusLabel`. */
fun terminalStatusLabel(status: TerminalStatus, hasRunningSubprocess: Boolean): String =
    when (status) {
        TerminalStatus.Running -> if (hasRunningSubprocess) "Task running" else "Ready"
        TerminalStatus.Starting -> "Starting"
        TerminalStatus.Exited -> "Exited"
        TerminalStatus.Error -> "Error"
        TerminalStatus.Closed -> "Not started"
    }

/**
 * `buildTerminalMenuSessions`: only live sessions plus the one this screen is
 * showing, sorted by id. The attached [current] session fills in while the
 * metadata stream has not caught up.
 */
fun buildTerminalMenuSessions(
    knownSessions: List<TerminalSummary>,
    threadId: String,
    workspaceRoot: String?,
    current: TerminalMenuSession?,
): List<TerminalMenuSession> {
    val sessionsById = linkedMapOf<String, TerminalMenuSession>()
    for (session in knownSessions) {
        if (session.threadId != threadId) continue
        if (!session.status.live && session.terminalId != current?.terminalId) continue
        sessionsById[session.terminalId] =
            TerminalMenuSession(
                terminalId = session.terminalId,
                cwd = session.cwd.ifBlank { workspaceRoot },
                status = session.status,
                hasRunningSubprocess = session.hasRunningSubprocess,
                displayLabel = terminalSessionLabel(session.terminalId, session.label),
                updatedAt = session.updatedAt,
            )
    }
    if (current != null && current.terminalId !in sessionsById) {
        sessionsById[current.terminalId] = current
    }
    return sessionsById.values.sortedWith(TERMINAL_ID_COMPARATOR)
}

/**
 * `pickRunningTerminalSessionForBootstrap`: the session a bare `/terminal`
 * route redirects to — the default id when it runs, else the first live one.
 */
fun pickRunningTerminalSession(sessions: List<TerminalMenuSession>): TerminalMenuSession? {
    val running = sessions.filter { it.status.live }
    if (running.isEmpty()) return null
    return running.firstOrNull { it.terminalId == DEFAULT_TERMINAL_ID } ?: running.first()
}

/**
 * `previousLiveTerminalId`: the session to show after this one exits — the
 * nearest live id below, falling back to the first above. Null means no live
 * session remains and the terminal UI should be dismissed.
 */
fun previousLiveTerminalId(sessions: List<TerminalMenuSession>, exitedTerminalId: String): String? {
    val live =
        sessions
            .filter { it.terminalId != exitedTerminalId && it.status.live }
            .sortedWith(TERMINAL_ID_COMPARATOR)
    if (live.isEmpty()) return null
    val below = live.filter { terminalIdLessThan(it.terminalId, exitedTerminalId) }
    return (below.lastOrNull() ?: live.first()).terminalId
}

private fun terminalIdLessThan(left: String, right: String): Boolean {
    val leftKey = terminalIdSortKey(left)
    val rightKey = terminalIdSortKey(right)
    return if (leftKey == rightKey) left < right else leftKey < rightKey
}

/** Current-session row for [buildTerminalMenuSessions] from a live attach. */
fun TerminalSession.toMenuSession(): TerminalMenuSession =
    TerminalMenuSession(
        terminalId = id,
        cwd = cwd.ifBlank { null },
        status = status,
        hasRunningSubprocess = hasRunningSubprocess,
        // `title` falls back to "Terminal" on a blank server label, which would
        // suppress the numbered `Terminal N` the menu expects — pass the label
        // through only when it carries real content.
        displayLabel = terminalSessionLabel(id, title.takeIf { it != "Terminal" }),
        updatedAt = updatedAt,
    )
