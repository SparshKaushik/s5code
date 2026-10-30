package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.CommandOutputFrameDto
import java.util.Collections
import java.util.IdentityHashMap
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow

/**
 * Live command output, ported from `packages/shared/src/terminalOutput.ts` and
 * `packages/client-runtime/src/state/commandOutput.ts`.
 *
 * The server streams `orchestration.subscribeCommandOutput` frames: a `replace`
 * carries the whole bounded tail plus the cursor and style escapes needed to
 * resume, an `append` carries raw terminal chunks. Both go through
 * [appendTerminalOutput], which must reach the same state the server's copy
 * does from the same input — so this is a faithful port, not a re-design.
 *
 * It models a small terminal: the last [LIVE_LINES] lines form a window cursor
 * movement can rewrite, so `\r` progress bars keep only their latest frame.
 * Older lines are fixed text. SGR styles survive as canonical `ESC[0;…m` codes,
 * every line starts from the default style and resets at its end, and only the
 * last `maxChars` survive.
 */

/** The default tail kept for one running command, in UTF-16 code units. */
const val TERMINAL_OUTPUT_TAIL_CHARS = 64 * 1024

/** Lines that cursor movement can still reach; older output is fixed. */
private const val LIVE_LINES = 64

/** Longest fragment held waiting for an escape sequence to finish. */
private const val MAX_PENDING_ESCAPE = 64

/** Past this many style changes, a line's oldest runs lose their style. */
private const val MAX_LINE_RUNS = 2_048

private const val ESC = '\u001b'

/** A styled stretch of one line. `pen` is canonical SGR parameters, "" for default. */
data class TerminalRun(val pen: String, val text: String)

/**
 * One line of the live window. Lines from a previous state are never mutated:
 * a feed copies a line before writing to it, then may extend its own copy.
 */
class TerminalLine(val runs: MutableList<TerminalRun> = mutableListOf(), var length: Int = runs.sumOf { it.text.length })

data class TerminalCursor(val row: Int, val col: Int)

data class TerminalScreen(
    /** Serialized lines, each ending in `\n`, that scrolled out of the window. */
    val settled: String = "",
    /** The live window; never empty. */
    val lines: List<TerminalLine> = listOf(TerminalLine()),
    val row: Int = 0,
    val col: Int = 0,
    val pen: String = "",
    val saved: TerminalCursor? = null,
)

data class TerminalOutputState(
    /** What to show: fixed lines, then the window. Contains only SGR escapes. */
    val text: String = "",
    /** True once anything was dropped from the front. */
    val truncated: Boolean = false,
    /** An unfinished escape sequence awaiting the next chunk. */
    val pending: String = "",
    val screen: TerminalScreen = TerminalScreen(),
)

val EMPTY_TERMINAL_OUTPUT = TerminalOutputState()

private fun sgr(pen: String): String = if (pen.isEmpty()) "$ESC[0m" else "$ESC[0;${pen}m"

/** Runs covering columns [start, end) of [line]. */
private fun sliceLine(line: TerminalLine, start: Int, end: Int): MutableList<TerminalRun> {
    val out = mutableListOf<TerminalRun>()
    var offset = 0
    for (run in line.runs) {
        val runEnd = offset + run.text.length
        if (runEnd > start && offset < end) {
            out += TerminalRun(
                run.pen,
                run.text.substring(max(0, start - offset), min(run.text.length, end - offset)),
            )
        }
        offset = runEnd
        if (offset >= end) break
    }
    return out
}

private fun pushRun(runs: MutableList<TerminalRun>, run: TerminalRun) {
    if (run.text.isEmpty()) return
    val last = runs.lastOrNull()
    if (last != null && last.pen == run.pen) runs[runs.size - 1] = TerminalRun(last.pen, last.text + run.text)
    else runs += run
}

/** Drops the first [count] columns of a line this feed owns, in place. */
private fun trimLineStart(runs: MutableList<TerminalRun>, count: Int) {
    var dropped = 0
    var remaining = count
    while (remaining > 0 && dropped < runs.size) {
        val run = runs[dropped]
        if (run.text.length <= remaining) {
            remaining -= run.text.length
            dropped += 1
        } else {
            runs[dropped] = TerminalRun(run.pen, run.text.substring(remaining))
            remaining = 0
        }
    }
    if (dropped > 0) runs.subList(0, dropped).clear()
}

/** [line] with [text] written over it from [col], as a terminal overwrites. */
private fun writeLine(line: TerminalLine, col: Int, text: String, pen: String): TerminalLine {
    val length = line.length
    if (col == length) {
        val runs = line.runs.toMutableList()
        pushRun(runs, TerminalRun(pen, text))
        return TerminalLine(runs, length + text.length)
    }
    val runs = mutableListOf<TerminalRun>()
    for (run in sliceLine(line, 0, min(col, length))) pushRun(runs, run)
    if (col > length) pushRun(runs, TerminalRun("", " ".repeat(col - length)))
    pushRun(runs, TerminalRun(pen, text))
    for (run in sliceLine(line, col + text.length, length)) pushRun(runs, run)
    return TerminalLine(runs)
}

/** Erase in line: 0 cursor to end, 1 start to cursor, 2 whole line. */
private fun eraseLine(line: TerminalLine, col: Int, mode: Int): TerminalLine {
    val length = line.length
    if (mode == 0) return TerminalLine(sliceLine(line, 0, col))
    if (mode == 1) {
        val runs = mutableListOf<TerminalRun>()
        pushRun(runs, TerminalRun("", " ".repeat(min(col + 1, length))))
        for (run in sliceLine(line, col + 1, length)) pushRun(runs, run)
        return TerminalLine(runs)
    }
    return TerminalLine()
}

private fun serializeLine(line: TerminalLine): String {
    val out = StringBuilder()
    var current = ""
    for (run in line.runs) {
        if (run.pen != current) {
            out.append(sgr(run.pen))
            current = run.pen
        }
        out.append(run.text)
    }
    if (current.isNotEmpty()) out.append(sgr(""))
    return out.toString()
}

private class Attributes(
    var bold: Boolean = false,
    var dim: Boolean = false,
    var italic: Boolean = false,
    var underline: Boolean = false,
    var strike: Boolean = false,
    var inverse: Boolean = false,
    var fg: String = "",
    var bg: String = "",
) {
    fun reset() {
        bold = false; dim = false; italic = false; underline = false
        strike = false; inverse = false; fg = ""; bg = ""
    }
}

/** A CSI parameter that did not parse as a number — JS `NaN`. */
private const val NOT_A_NUMBER = -1

private fun parseParam(part: String): Int =
    if (part.isEmpty()) 0 else part.toLongOrNull()?.coerceIn(0, Int.MAX_VALUE.toLong())?.toInt() ?: NOT_A_NUMBER

private fun parsePen(pen: String): Attributes {
    val attributes = Attributes()
    if (pen.isNotEmpty()) applySgr(attributes, pen.split(";").map(::parseParam))
    return attributes
}

private fun formatPen(attributes: Attributes): String {
    val parts = mutableListOf<String>()
    if (attributes.bold) parts += "1"
    if (attributes.dim) parts += "2"
    if (attributes.italic) parts += "3"
    if (attributes.underline) parts += "4"
    if (attributes.inverse) parts += "7"
    if (attributes.strike) parts += "9"
    if (attributes.fg.isNotEmpty()) parts += attributes.fg
    if (attributes.bg.isNotEmpty()) parts += attributes.bg
    return parts.joinToString(";")
}

/** Reads an extended colour (`5;n` or `2;r;g;b`) at [index]; returns the code and next index. */
private fun extendedColor(code: Int, params: List<Int>, index: Int): Pair<String?, Int> {
    val mode = params.getOrNull(index + 1)
    if (mode == 5) {
        val value = params.getOrNull(index + 2)
        return (if (value == null || value == NOT_A_NUMBER || value > 255) null else "$code;5;$value") to index + 3
    }
    if (mode == 2) {
        val r = params.getOrNull(index + 2)
        val g = params.getOrNull(index + 3)
        val b = params.getOrNull(index + 4)
        if (r == null || g == null || b == null) return null to index + 5
        fun channel(value: Int) = if (value == NOT_A_NUMBER) 0 else value and 255
        return "$code;2;${channel(r)};${channel(g)};${channel(b)}" to index + 5
    }
    return null to index + 2
}

private fun applySgr(attributes: Attributes, input: List<Int>) {
    val params = input.ifEmpty { listOf(0) }
    var index = 0
    while (index < params.size) {
        val code = params[index]
        when {
            code == 0 || code == NOT_A_NUMBER -> attributes.reset()
            code == 1 -> attributes.bold = true
            code == 2 -> attributes.dim = true
            code == 3 -> attributes.italic = true
            code == 4 -> attributes.underline = true
            code == 7 -> attributes.inverse = true
            code == 9 -> attributes.strike = true
            code == 22 -> { attributes.bold = false; attributes.dim = false }
            code == 23 -> attributes.italic = false
            code == 24 -> attributes.underline = false
            code == 27 -> attributes.inverse = false
            code == 29 -> attributes.strike = false
            code in 30..37 || code in 90..97 -> attributes.fg = code.toString()
            code in 40..47 || code in 100..107 -> attributes.bg = code.toString()
            code == 39 -> attributes.fg = ""
            code == 49 -> attributes.bg = ""
            code == 38 || code == 48 -> {
                val (color, next) = extendedColor(code, params, index)
                if (color != null) {
                    if (code == 38) attributes.fg = color else attributes.bg = color
                }
                index = next
                continue
            }
        }
        index += 1
    }
}

private val STRING_SEQUENCE_INTRODUCERS = setOf(']', 'P', 'X', '_', '^')

/** Length of a complete escape sequence starting at [index] (an ESC), or -1 if unfinished. */
private fun escapeLength(input: String, index: Int): Int {
    val next = input.getOrNull(index + 1) ?: return -1
    if (next == '[') {
        // CSI: parameters/intermediates, then a final byte in @-~.
        for (cursor in index + 2 until input.length) {
            if (input[cursor].code in 0x40..0x7e) return cursor - index + 1
        }
        return -1
    }
    if (next in STRING_SEQUENCE_INTRODUCERS) {
        // OSC/DCS/SOS/APC/PM end at BEL or ST (ESC \). A newline also ends one
        // (and is kept), so a stray introducer cannot hide everything after it.
        for (cursor in index + 2 until input.length) {
            if (input[cursor] == '\u0007') return cursor - index + 1
            if (input[cursor] == '\n') return cursor - index
            if (input[cursor] == ESC) {
                if (cursor + 1 >= input.length) return -1
                if (input[cursor + 1] == '\\') return cursor - index + 2
            }
        }
        return -1
    }
    // Charset designation (ESC ( B) takes three; other two-byte sequences (ESC 7) two.
    if (next == '(' || next == ')' || next == '*' || next == '+') {
        return if (index + 2 < input.length) 3 else -1
    }
    return 2
}

/** CSI parameters; colon sub-parameters drop the optional colour-space id. */
private fun csiParams(params: String): List<Int> {
    val numbers = mutableListOf<Int>()
    for (part in params.split(";")) {
        if (':' !in part) {
            numbers += parseParam(part)
            continue
        }
        val sub = part.split(":").map(::parseParam).toMutableList()
        if ((sub[0] == 38 || sub[0] == 48) && sub.getOrNull(1) == 2 && sub.size >= 6) sub.removeAt(2)
        numbers += sub
    }
    return numbers
}

/** A mutable copy of the screen, for applying one chunk. */
private class Terminal(screen: TerminalScreen, val maxChars: Int) {
    var settled = screen.settled
    var lines: MutableList<TerminalLine> = screen.lines.toMutableList()
    var row = screen.row
    var col = screen.col
    var pen = screen.pen
    var saved = screen.saved

    /** Output was discarded on purpose (scrollback cleared or window over budget). */
    var dropped = false

    /** Lines created during this feed, which it may extend in place. */
    private val owned: MutableSet<TerminalLine> = Collections.newSetFromMap(IdentityHashMap())

    fun write(text: String) {
        var line = lines[row]
        val length = line.length
        if (col == length && line in owned) {
            pushRun(line.runs, TerminalRun(pen, text))
            line.length = length + text.length
        } else {
            line = writeLine(line, col, text, pen)
            owned += line
        }
        col += text.length
        if (line.runs.size > MAX_LINE_RUNS) {
            // Fold all but the newest runs into one plain run, so trimming eats it cheaply.
            val keep = line.runs.size - MAX_LINE_RUNS / 8
            val plain = line.runs.subList(0, keep).joinToString("") { it.text }
            line = TerminalLine(
                (listOf(TerminalRun("", plain)) + line.runs.subList(keep, line.runs.size)).toMutableList(),
                line.length,
            )
            owned += line
        }
        // A line keeps only its last `maxChars` columns, trimmed on every write so
        // the result never depends on how the output was split into chunks.
        var current = line.length
        if (current > maxChars) {
            var removed = current - maxChars
            if (line !in owned) {
                line = TerminalLine(line.runs.toMutableList(), line.length)
                owned += line
            }
            trimLineStart(line.runs, removed)
            // Never start on a lone low surrogate.
            if (line.runs.firstOrNull()?.text?.firstOrNull()?.isLowSurrogate() == true) {
                trimLineStart(line.runs, 1)
                removed += 1
            }
            current -= removed
            dropped = true
            col = max(0, col - removed)
            saved?.let { if (it.row == row) saved = it.copy(col = max(0, it.col - removed)) }
        }
        line.length = current
        lines[row] = line
    }

    fun newline() {
        row += 1
        col = 0
        if (row == lines.size) lines += TerminalLine()
        while (lines.size > LIVE_LINES) scrollOut()
    }

    /** Moves the top line out of the window into scrollback; returns its length. */
    fun scrollOut(): Int {
        val line = lines.removeAt(0)
        settle(line)
        row -= 1
        saved?.let { saved = it.copy(row = max(0, it.row - 1)) }
        return line.length
    }

    /** Fixes a line that left the window, keeping scrollback near the tail size. */
    fun settle(line: TerminalLine) {
        settled += serializeLine(line) + "\n"
        if (settled.length > 2 * maxChars) {
            settled = terminalOutputTail(settled, maxChars).first
            dropped = true
        }
    }

    fun moveTo(targetRow: Int, targetCol: Int) {
        row = targetRow.coerceIn(0, lines.size - 1)
        // A huge column (ESC[999999999C) would otherwise pad a line past any limit.
        col = targetCol.coerceIn(0, TERMINAL_OUTPUT_TAIL_CHARS)
    }

    fun csi(params: String, final: Char) {
        if (params.firstOrNull() in setOf('?', '>', '=')) return // Mode switches.
        val numbers = if (params.isEmpty()) emptyList() else csiParams(params)
        val first = numbers.getOrNull(0)
        val count = if (first == null || first <= 0) 1 else first
        when (final) {
            'm' -> {
                val attributes = parsePen(pen)
                applySgr(attributes, numbers)
                pen = formatPen(attributes)
            }
            'A' -> moveTo(row - count, col)
            'B' -> moveTo(row + count, col)
            'C' -> moveTo(row, col.toLong().plus(count).coerceAtMost(Int.MAX_VALUE.toLong()).toInt())
            'D' -> moveTo(row, col - count)
            'E' -> moveTo(row + count, 0)
            'F' -> moveTo(row - count, 0)
            'G' -> moveTo(row, count - 1)
            // Rows count from the top of the window: there is no fixed screen height.
            'H', 'f' -> moveTo(count - 1, (numbers.getOrNull(1)?.takeIf { it > 0 } ?: 1) - 1)
            'K' -> lines[row] = eraseLine(lines[row], col, first ?: 0)
            'J' -> when (first ?: 0) {
                0 -> {
                    lines[row] = eraseLine(lines[row], col, 0)
                    while (lines.size > row + 1) lines.removeAt(lines.size - 1)
                }
                1 -> {
                    for (index in 0 until row) lines[index] = TerminalLine()
                    lines[row] = eraseLine(lines[row], col, 1)
                }
                3 -> {
                    // Clear scrollback: what already scrolled out goes, the window stays.
                    if (settled.isNotEmpty()) dropped = true
                    settled = ""
                }
                else -> {
                    // A cleared screen starts the window over; what scrolled out stays.
                    lines = mutableListOf(TerminalLine())
                    row = 0
                    col = 0
                }
            }
            's' -> saved = TerminalCursor(row, col)
            'u' -> saved?.let { moveTo(it.row, it.col) }
        }
    }

    fun escape(sequence: String) {
        when {
            sequence[1] == '[' -> csi(sequence.substring(2, sequence.length - 1), sequence.last())
            sequence == "${ESC}7" -> saved = TerminalCursor(row, col)
            sequence == "${ESC}8" -> saved?.let { moveTo(it.row, it.col) }
            sequence == "${ESC}M" -> moveTo(row - 1, col) // Reverse line feed.
        }
    }

    /** Applies [input]; returns the unfinished escape left at its end. */
    fun feed(input: String): String {
        var printable = 0
        fun flush(end: Int) {
            if (end > printable) write(input.substring(printable, end))
        }
        var index = 0
        while (index < input.length) {
            val code = input[index].code
            // Printable text, including tabs, is batched into one write.
            if (code == 0x09 || (code >= 0x20 && code != 0x7f && (code < 0x80 || code > 0x9f))) {
                index += 1
                continue
            }
            flush(index)
            printable = index + 1
            when (code) {
                0x0a -> newline()
                0x0d -> col = 0
                0x08 -> col = max(0, col - 1)
                0x1b -> {
                    val length = escapeLength(input, index)
                    if (length == -1) {
                        val fragment = input.substring(index)
                        if (fragment.length <= MAX_PENDING_ESCAPE) return fragment
                        // A long string sequence keeps only its introducer (and a
                        // trailing ESC that may start its ST), so the payload stays
                        // hidden until it ends.
                        if (fragment[1] in STRING_SEQUENCE_INTRODUCERS) {
                            return fragment.substring(0, 2) + if (fragment.endsWith(ESC)) ESC.toString() else ""
                        }
                        // Any other runaway sequence is noise.
                        return ""
                    }
                    escape(input.substring(index, index + length))
                    index += length - 1
                    printable = index + 1
                }
                // Other C0, DEL and C1 controls carry no visible text.
            }
            index += 1
        }
        flush(input.length)
        return ""
    }
}

/** The active style at [index] of [text] and where a cut there may safely start. */
private fun styleAt(text: String, index: Int): Pair<Int, String> {
    val lineStart = text.lastIndexOf('\n', index - 1) + 1
    var prefix = ""
    var cursor = lineStart
    while (cursor < index) {
        val found = text.indexOf("$ESC[", cursor)
        if (found == -1 || found >= index) break
        val end = text.indexOf('m', found)
        if (end == -1) break
        if (end >= index) return end + 1 to text.substring(found, end + 1)
        prefix = text.substring(found, end + 1)
        cursor = end + 1
    }
    return index to if (prefix == sgr("")) "" else prefix
}

/** Keeps the last [maxChars], preferring to start on a fresh line; second is "truncated". */
fun terminalOutputTail(text: String, maxChars: Int): Pair<String, Boolean> {
    if (text.length <= maxChars) return text to false
    var start = text.length - maxChars
    val newline = text.indexOf('\n', start)
    // Drop the partial first line unless that would discard most of the tail.
    if (newline != -1 && newline - start < maxChars / 4) return text.substring(newline + 1) to true
    // Cutting inside a line: never split an escape, and keep the colour in force.
    val (safeStart, prefix) = styleAt(text, start)
    start = safeStart
    // Never start on a lone low surrogate.
    if (text.getOrNull(start)?.isLowSurrogate() == true) start += 1
    return prefix + text.substring(min(start, text.length)) to true
}

/** Appends one decoded chunk and returns the new bounded state. */
fun appendTerminalOutput(
    state: TerminalOutputState,
    chunk: String,
    maxChars: Int = TERMINAL_OUTPUT_TAIL_CHARS,
): TerminalOutputState {
    val input = state.pending + chunk
    if (input.isEmpty()) return state
    val terminal = Terminal(state.screen, maxChars)
    val pending = terminal.feed(input)
    var truncated = state.truncated
    // The window as a whole stays within the tail too: oldest lines scroll out early.
    var windowChars = terminal.lines.sumOf { it.length }
    while (windowChars > maxChars && terminal.row > 0) windowChars -= terminal.scrollOut()
    // Lines below a cursor that moved up cannot scroll out; they keep only their ends.
    if (windowChars > maxChars) {
        var budget = maxChars
        for (row in 0..terminal.row) budget -= terminal.lines[row].length
        for (row in terminal.row + 1 until terminal.lines.size) {
            val line = terminal.lines[row]
            val length = line.length
            val keep = budget.coerceIn(0, length)
            if (keep < length) {
                truncated = true
                terminal.lines[row] = TerminalLine(sliceLine(line, length - keep, length), keep)
                terminal.saved?.let {
                    if (it.row == row) terminal.saved = it.copy(col = max(0, it.col - (length - keep)))
                }
            }
            budget -= keep
        }
    }
    if (terminal.dropped) truncated = true
    val (settled, settledTruncated) = terminalOutputTail(terminal.settled, maxChars)
    val window = terminal.lines.joinToString("\n", transform = ::serializeLine)
    val (text, tailTruncated) = terminalOutputTail(settled + window, maxChars)
    return TerminalOutputState(
        text = text,
        truncated = truncated || settledTruncated || tailTruncated,
        pending = pending,
        screen = TerminalScreen(
            settled = settled,
            lines = terminal.lines,
            row = terminal.row,
            col = terminal.col,
            pen = terminal.pen,
            saved = terminal.saved,
        ),
    )
}

/* ── Command output frames ───────────────────────────────────────────── */

/** One command row's output as this client holds it. */
data class CommandOutputView(
    val output: TerminalOutputState = EMPTY_TERMINAL_OUTPUT,
    /** False once the command settled and [output] is final. */
    val running: Boolean = true,
)

/**
 * `applyCommandOutputFrame`: a `replace` restarts from the bounded tail it
 * carries, an `append` extends what is held; truncation is sticky and
 * `running` follows the latest frame.
 */
fun applyCommandOutputFrame(view: CommandOutputView, frame: CommandOutputFrameDto): CommandOutputView {
    val base = if (frame.kind == "replace") EMPTY_TERMINAL_OUTPUT else view.output
    return CommandOutputView(
        output = appendTerminalOutput(base.copy(truncated = base.truncated || frame.truncated), frame.text),
        running = frame.running,
    )
}

/* ── Rendering ───────────────────────────────────────────────────────── */

/** A span's style; colours are ARGB, already resolved against the palette. */
data class TerminalSpanStyle(
    val color: Int? = null,
    val backgroundColor: Int? = null,
    val bold: Boolean = false,
    /** Rendered at 65% opacity. */
    val dim: Boolean = false,
    val italic: Boolean = false,
    val underline: Boolean = false,
    val strike: Boolean = false,
)

/** A stretch of shown output with one style; null style is the default text. */
data class TerminalSpan(val text: String, val style: TerminalSpanStyle?)

private fun hex(value: Int): Int = (0xFF shl 24) or value

/** The 16 standard terminal colours, chosen to stay readable on each app theme. */
val TERMINAL_PALETTE_DARK = listOf(
    0x3b4048, 0xf47067, 0x57ab5a, 0xc69026, 0x539bf5, 0xb083f0, 0x39c5cf, 0xadbac7,
    0x768390, 0xff938a, 0x6bc46d, 0xdaaa3f, 0x6cb6ff, 0xdcbdfb, 0x56d4dd, 0xcdd9e5,
).map(::hex)

val TERMINAL_PALETTE_LIGHT = listOf(
    0x24292f, 0xcf222e, 0x116329, 0x7d4e00, 0x0550ae, 0x8250df, 0x1b7c83, 0x6e7781,
    0x57606a, 0xa40e26, 0x1a7f37, 0x633c01, 0x0969da, 0x6639ba, 0x3192aa, 0x8c959f,
).map(::hex)

private fun rgb(r: Int, g: Int, b: Int): Int = (0xFF shl 24) or ((r and 255) shl 16) or ((g and 255) shl 8) or (b and 255)

private fun xtermColor(index: Int, palette: List<Int>): Int {
    if (index < 16) return palette[index]
    if (index >= 232) {
        val level = 8 + (index - 232) * 10
        return rgb(level, level, level)
    }
    val cube = index - 16
    fun channel(value: Int) = if (value == 0) 0 else 55 + value * 40
    return rgb(channel(cube / 36), channel((cube / 6) % 6), channel(cube % 6))
}

private fun resolveColor(code: String, base: Int, palette: List<Int>): Int? {
    val parts = code.split(";").map { it.toIntOrNull() ?: 0 }
    val head = parts[0]
    if (head == base + 8) {
        if (parts.getOrNull(1) == 5) return xtermColor(parts.getOrElse(2) { 0 }, palette)
        return rgb(parts.getOrElse(2) { 0 }, parts.getOrElse(3) { 0 }, parts.getOrElse(4) { 0 })
    }
    if (head in base..base + 7) return palette[head - base]
    if (head in base + 60..base + 67) return palette[head - base - 52]
    return null
}

private fun luminance(color: Int): Double {
    fun channel(value: Int): Double {
        val scaled = value / 255.0
        return if (scaled <= 0.03928) scaled / 12.92 else ((scaled + 0.055) / 1.055).pow(2.4)
    }
    return 0.2126 * channel((color shr 16) and 255) + 0.7152 * channel((color shr 8) and 255) +
        0.0722 * channel(color and 255)
}

private fun contrastRatio(a: Int, b: Int): Double {
    val first = luminance(a)
    val second = luminance(b)
    return (max(first, second) + 0.05) / (min(first, second) + 0.05)
}

private fun spanStyle(pen: String, palette: List<Int>): TerminalSpanStyle? {
    if (pen.isEmpty()) return null
    val attributes = parsePen(pen)
    var color = attributes.fg.takeIf { it.isNotEmpty() }?.let { resolveColor(it, 30, palette) }
    var backgroundColor = attributes.bg.takeIf { it.isNotEmpty() }?.let { resolveColor(it, 40, palette) }
    if (attributes.inverse) {
        val swapped = backgroundColor
        backgroundColor = color ?: palette[7]
        color = swapped
    }
    // Badges like "PASS" on green assume a terminal's own palette; keep their text readable.
    if (backgroundColor != null && (color == null || contrastRatio(color, backgroundColor) < 3)) {
        color = if (luminance(backgroundColor) > 0.35) hex(0x1f2328) else hex(0xffffff)
    }
    return TerminalSpanStyle(
        color = color,
        backgroundColor = backgroundColor,
        bold = attributes.bold,
        dim = attributes.dim,
        italic = attributes.italic,
        underline = attributes.underline,
        strike = attributes.strike,
    )
}

private val SGR_PATTERN = Regex("$ESC\\[[0-9;]*m")

/** Shown output without its colour codes. */
fun terminalOutputPlainText(text: String): String = text.replace(SGR_PATTERN, "")

/**
 * Splits shown output into styled spans. Past [maxSpans] the rest is one plain
 * span, so a pathological stream of colour changes stays cheap to render.
 */
fun terminalOutputSpans(text: String, palette: List<Int>, maxSpans: Int = 2_000): List<TerminalSpan> {
    val spans = mutableListOf<TerminalSpan>()
    val styles = HashMap<String, TerminalSpanStyle?>()
    var pen = ""
    var cursor = 0
    fun push(end: Int) {
        if (end <= cursor) return
        val style = styles.getOrPut(pen) { spanStyle(pen, palette) }
        spans += TerminalSpan(text.substring(cursor, end), style)
    }
    while (cursor < text.length) {
        val escape = text.indexOf("$ESC[", cursor)
        if (escape == -1 || spans.size >= maxSpans) break
        push(escape)
        val end = text.indexOf('m', escape)
        if (end == -1) break
        val params = text.substring(escape + 2, end)
        pen = if (params == "0") "" else params.removePrefix("0;")
        cursor = end + 1
    }
    if (cursor < text.length) {
        if (spans.size >= maxSpans) spans += TerminalSpan(terminalOutputPlainText(text.substring(cursor)), null)
        else push(text.length)
    }
    return spans
}
