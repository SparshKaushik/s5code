package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.TerminalStatus

const val DEFAULT_TERMINAL_COLS = 80
const val DEFAULT_TERMINAL_ROWS = 24

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
