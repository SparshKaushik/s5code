package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.TerminalStatus

const val DEFAULT_TERMINAL_COLS = 80
const val DEFAULT_TERMINAL_ROWS = 24

/**
 * `DEFAULT_MAX_TERMINAL_BUFFER_BYTES` in the RN client's terminal state: the
 * retained tail of PTY history. Larger scrollbacks are trimmed from the front so
 * a long-running session cannot grow memory without bound.
 */
const val MAX_TERMINAL_BUFFER_BYTES = 512 * 1024

/**
 * Drops history from the front until the builder encodes to at most
 * [maxBytes] UTF-8 bytes. Character length overestimates bytes for ASCII-heavy
 * output only mildly, but trimming on the UTF-8 boundary avoids cutting a
 * surrogate pair; the RN client trims its byte buffer the same way.
 */
fun StringBuilder.trimToUtf8Tail(maxBytes: Int) {
    // Cheap bail: a Kotlin char encodes to at most 3 UTF-8 bytes (4 for a
    // surrogate pair), so a short-enough length can't exceed the cap.
    if (length <= maxBytes / 3) return
    // Walk from the end accumulating encoded size; the retained tail is what
    // fits, so the front is dropped whole. Cheap for ASCII, exact for UTF-16.
    var bytes = 0
    var start = length
    while (start > 0) {
        val c = this[start - 1]
        val paired = c.isLowSurrogate() && start > 1 && this[start - 2].isHighSurrogate()
        val charBytes =
            when {
                paired -> 4
                c.code < 0x80 -> 1
                c.code < 0x800 -> 2
                else -> 3
            }
        // The char that does not fit is dropped with the rest of the overflow;
        // the retained tail starts after it.
        if (bytes + charBytes > maxBytes) break
        bytes += charBytes
        start -= if (paired) 2 else 1
    }
    if (start > 0) delete(0, start)
}

/**
 * How far past the cap history may grow before it is trimmed. Trimming walks
 * the retained tail, so doing it per chunk once scrollback is full would cost
 * 512 KB of work for every few bytes of output.
 */
const val TERMINAL_TRIM_SLACK_CHARS = 16 * 1024

/**
 * Where a terminal view's feed stands: `TerminalOutputCursor` in client-runtime
 * state/terminalOutput.ts. [offset] counts characters appended since the reset
 * numbered [resetVersion].
 */
data class TerminalOutputCursor(val resetVersion: Int, val offset: Long)

sealed interface TerminalOutputUpdate {
    data object None : TerminalOutputUpdate

    /** Recreate the terminal and replay [data], the whole retained history. */
    data class Reset(val data: String) : TerminalOutputUpdate

    data class Append(val data: String) : TerminalOutputUpdate
}

/**
 * `readTerminalOutputUpdate`: what a view fed up to [cursor] must do to show
 * [buffer], which ends at offset [end] of reset [resetVersion]. Output past the
 * cursor appends, so a full scrollback trimming its front does not replay
 * history into the terminal; only a reset, or a cursor that fell out of the
 * retained tail, does.
 */
fun readTerminalOutputUpdate(
    buffer: String,
    resetVersion: Int,
    end: Long,
    cursor: TerminalOutputCursor?,
): Pair<TerminalOutputUpdate, TerminalOutputCursor> {
    val next = TerminalOutputCursor(resetVersion, end)
    val start = end - buffer.length
    val update =
        when {
            cursor == null || cursor.resetVersion != resetVersion || cursor.offset !in start..end ->
                TerminalOutputUpdate.Reset(buffer)
            cursor.offset == end -> TerminalOutputUpdate.None
            else -> TerminalOutputUpdate.Append(buffer.substring((cursor.offset - start).toInt()))
        }
    return update to next
}

/**
 * Wire status to model. Unknown values read as an error rather than a plausible
 * default: the contract's statuses are closed, so a new one means this build does
 * not understand the session and should not claim it is running.
 */
fun terminalStatusOf(wire: String): TerminalStatus =
    when (wire) {
        "starting" -> TerminalStatus.Starting
        "running" -> TerminalStatus.Running
        "exited" -> TerminalStatus.Exited
        "closed" -> TerminalStatus.Closed
        else -> TerminalStatus.Error
    }

/**
 * Grid size for a monospace surface, from `terminalUiState.ts` in the RN client.
 * Clamped to the contract's own bounds so a mid-rotation measurement cannot make
 * the resize RPC fail validation.
 */
fun terminalGridSize(
    widthPx: Float,
    heightPx: Float,
    charWidthPx: Float,
    lineHeightPx: Float,
): Pair<Int, Int> {
    if (charWidthPx <= 0f || lineHeightPx <= 0f) {
        return DEFAULT_TERMINAL_COLS to DEFAULT_TERMINAL_ROWS
    }
    val cols = (widthPx / charWidthPx).toInt().coerceIn(1, 1_000)
    val rows = (heightPx / lineHeightPx).toInt().coerceIn(1, 500)
    return cols to rows
}
