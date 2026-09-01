package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.TerminalStatus

/**
 * Turns raw PTY bytes into lines of text.
 *
 * The server streams exactly what the shell wrote, escape sequences included, so
 * anything that renders `history` verbatim shows `[?2004h` and `[0m` instead of
 * output. The other clients hand that stream to `libghostty-vt`; this build has no
 * native VT yet, so this is a deliberately small emulator that covers what a
 * shell session actually emits: carriage returns, backspaces, tabs, cursor moves,
 * and the erase sequences prompts use to redraw themselves.
 *
 * What it does not do is the alternate screen. Full-screen programs (vim, htop,
 * less) address a fixed viewport and will look wrong here; that is the gap the
 * native VT closes. Everything is kept as a scrollback of lines rather than a
 * fixed grid, because a phone renders a shell transcript far more often than it
 * renders a TUI.
 *
 * Feeding is incremental on purpose. Re-parsing the whole buffer on every chunk
 * would be O(buffer) per keystroke on a 512 KB scrollback, which is exactly the
 * kind of quiet cost that shows up as a dropped frame while output is streaming.
 */
class TerminalEmulator(
    /**
     * Viewport height, used only to resolve absolute cursor addressing: `ESC[H`
     * means "top of the screen", which in a scrollback model is the last [rows]
     * lines. Fixed for the life of one attach, since that is the size the PTY was
     * opened with.
     */
    private val rows: Int = DEFAULT_ROWS,
    private val maxScrollback: Int = MAX_SCROLLBACK_LINES,
) {
    private val lines = ArrayDeque<StringBuilder>().apply { addLast(StringBuilder()) }
    private var row = 0
    private var column = 0

    /** A sequence split across two chunks; parsed when the rest arrives. */
    private var pending = StringBuilder()

    /** Drops everything, as a fresh snapshot does. */
    fun reset() {
        lines.clear()
        lines.addLast(StringBuilder())
        row = 0
        column = 0
        pending = StringBuilder()
    }

    fun snapshot(): List<String> = lines.map { it.toString() }

    fun feed(text: String) {
        val input = if (pending.isEmpty()) text else pending.toString() + text
        pending = StringBuilder()
        var index = 0
        while (index < input.length) {
            val char = input[index]
            when {
                char == ESC -> {
                    val consumed = escape(input, index)
                    if (consumed == INCOMPLETE) {
                        // Hold the partial sequence rather than printing it: a
                        // chunk boundary inside `ESC[0m` would otherwise leak
                        // `[0m` into the transcript forever.
                        pending.append(input, index, input.length)
                        return
                    }
                    index += consumed
                    continue
                }
                char == '\n' -> {
                    // Treated as CR+LF rather than a bare index. A PTY with ONLCR
                    // sends `\r\n` and either reading works, but persisted history
                    // and non-shell writers do emit bare LF, and a strict index
                    // staircases that output across the screen. Since this renders
                    // a scrollback rather than a grid, the staircase has no
                    // upside.
                    row += 1
                    column = 0
                    growTo(row)
                }
                char == '\r' -> column = 0
                char == '\b' -> column = (column - 1).coerceAtLeast(0)
                char == '\t' -> {
                    val next = ((column / TAB_WIDTH) + 1) * TAB_WIDTH
                    write(" ".repeat(next - column))
                    index += 1
                    continue
                }
                char == BELL -> {}
                char.code < 0x20 -> {}
                else -> {
                    // Runs of printable text are written in one go; per-character
                    // insertion into a StringBuilder is the hot path here.
                    var end = index
                    while (end < input.length && isPrintable(input[end])) end += 1
                    write(input.substring(index, end))
                    index = end
                    continue
                }
            }
            index += 1
        }
    }

    private fun isPrintable(char: Char) = char.code >= 0x20 && char != ESC

    /** Returns how many characters the sequence at [start] consumed. */
    private fun escape(input: String, start: Int): Int {
        if (start + 1 >= input.length) return INCOMPLETE
        return when (input[start + 1]) {
            '[' -> csi(input, start)
            // OSC and the other string sequences (title, hyperlinks) run until
            // BEL or ESC \, and carry nothing this renderer shows.
            ']', 'P', '^', '_' -> stringSequence(input, start)
            // Charset selection and single-character escapes.
            '(', ')', '*', '+' -> if (start + 2 >= input.length) INCOMPLETE else 3
            '7', '8', '=', '>', 'c' -> 2
            'M' -> {
                row = (row - 1).coerceAtLeast(0)
                2
            }
            'D' -> {
                row += 1
                growTo(row)
                2
            }
            else -> 2
        }
    }

    private fun stringSequence(input: String, start: Int): Int {
        var index = start + 2
        while (index < input.length) {
            if (input[index] == BELL) return index - start + 1
            if (input[index] == ESC) {
                if (index + 1 >= input.length) return INCOMPLETE
                if (input[index + 1] == '\\') return index - start + 2
            }
            index += 1
        }
        return INCOMPLETE
    }

    private fun csi(input: String, start: Int): Int {
        var index = start + 2
        val params = StringBuilder()
        while (index < input.length && input[index].code in 0x30..0x3F) {
            params.append(input[index])
            index += 1
        }
        // Intermediate bytes, e.g. the `$` in `ESC[1$p`.
        while (index < input.length && input[index].code in 0x20..0x2F) index += 1
        if (index >= input.length) return INCOMPLETE
        val final = input[index]
        apply(final, params.toString())
        return index - start + 1
    }

    private fun apply(final: Char, params: String) {
        // Private sequences (`ESC[?2004h`, cursor visibility) change modes this
        // renderer does not model.
        if (params.startsWith("?") || params.startsWith(">")) return
        val numbers = params.split(';').map { it.toIntOrNull() }
        fun param(position: Int, fallback: Int) =
            numbers.getOrNull(position)?.takeIf { it > 0 } ?: fallback

        when (final) {
            'A' -> row = (row - param(0, 1)).coerceAtLeast(0)
            'B' -> {
                row += param(0, 1)
                growTo(row)
            }
            'C' -> column += param(0, 1)
            'D' -> column = (column - param(0, 1)).coerceAtLeast(0)
            'E' -> {
                row += param(0, 1)
                column = 0
                growTo(row)
            }
            'F' -> {
                row = (row - param(0, 1)).coerceAtLeast(0)
                column = 0
            }
            'G' -> column = param(0, 1) - 1
            'H',
            'f' -> {
                row = viewportTop() + param(0, 1) - 1
                column = param(1, 1) - 1
                growTo(row)
            }
            'J' ->
                when (numbers.firstOrNull() ?: 0) {
                    // 0 and 1 erase around the cursor; 2 and 3 take the screen,
                    // which for a scrollback means dropping it and starting over.
                    0 -> {
                        truncateLine(row, column)
                        while (lines.size > row + 1) lines.removeLast()
                    }
                    1 -> blankLineStart(row, column)
                    else -> {
                        lines.clear()
                        lines.addLast(StringBuilder())
                        row = 0
                        column = 0
                    }
                }
            'K' ->
                when (numbers.firstOrNull() ?: 0) {
                    0 -> truncateLine(row, column)
                    1 -> blankLineStart(row, column)
                    else -> lineAt(row).setLength(0)
                }
            'P' -> {
                // Delete character: what a prompt uses when you press backspace
                // mid-line.
                val line = lineAt(row)
                val count = param(0, 1)
                if (column < line.length) {
                    line.delete(column, (column + count).coerceAtMost(line.length))
                }
            }
            'X' -> {
                val line = lineAt(row)
                val count = param(0, 1)
                padTo(line, column + count)
                for (offset in 0 until count) line.setCharAt(column + offset, ' ')
            }
            'd' -> {
                row = param(0, 1) - 1
                growTo(row)
            }
            else -> {}
        }
    }

    /**
     * Where absolute cursor addressing starts. Anchoring it to the tail of the
     * scrollback is what keeps a prompt that repaints itself with `ESC[H` from
     * overwriting the top of the session.
     */
    private fun viewportTop() = (lines.size - rows).coerceAtLeast(0)

    private fun write(text: String) {
        val line = lineAt(row)
        padTo(line, column)
        val end = column + text.length
        if (end <= line.length) {
            line.replace(column, end, text)
        } else {
            line.setLength(column)
            line.append(text)
        }
        column = end
    }

    private fun truncateLine(row: Int, from: Int) {
        val line = lineAt(row)
        if (from < line.length) line.setLength(from)
    }

    private fun blankLineStart(row: Int, through: Int) {
        val line = lineAt(row)
        padTo(line, through + 1)
        for (index in 0..through.coerceAtMost(line.length - 1)) line.setCharAt(index, ' ')
    }

    private fun padTo(line: StringBuilder, length: Int) {
        while (line.length < length) line.append(' ')
    }

    private fun lineAt(row: Int): StringBuilder {
        growTo(row)
        return lines.elementAt(row.coerceIn(0, lines.size - 1))
    }

    private fun growTo(row: Int) {
        while (lines.size <= row) lines.addLast(StringBuilder())
        trim()
    }

    /** Bounded scrollback. Dropping from the front moves the cursor with it. */
    private fun trim() {
        var dropped = 0
        while (lines.size > maxScrollback) {
            lines.removeFirst()
            dropped += 1
        }
        if (dropped > 0) row = (row - dropped).coerceAtLeast(0)
    }

    companion object {
        const val DEFAULT_COLS = 80
        const val DEFAULT_ROWS = 24

        /**
         * Scrollback ceiling. The server already caps what it keeps; this bounds
         * what a long-lived screen holds in memory and hands to LazyColumn.
         */
        const val MAX_SCROLLBACK_LINES = 4_000

        private const val TAB_WIDTH = 8
        private const val ESC = '\u001b'
        private const val BELL = '\u0007'
        private const val INCOMPLETE = -1
    }
}

/** Convenience for one-shot rendering, used by tests and snapshot-only reads. */
fun terminalLines(raw: String, rows: Int = TerminalEmulator.DEFAULT_ROWS): List<String> =
    TerminalEmulator(rows = rows).also { it.feed(raw) }.snapshot()

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
fun terminalGridSize(widthPx: Float, heightPx: Float, charWidthPx: Float, lineHeightPx: Float): Pair<Int, Int> {
    if (charWidthPx <= 0f || lineHeightPx <= 0f) {
        return TerminalEmulator.DEFAULT_COLS to TerminalEmulator.DEFAULT_ROWS
    }
    val cols = (widthPx / charWidthPx).toInt().coerceIn(1, 1_000)
    val rows = (heightPx / lineHeightPx).toInt().coerceIn(1, 500)
    return cols to rows
}
