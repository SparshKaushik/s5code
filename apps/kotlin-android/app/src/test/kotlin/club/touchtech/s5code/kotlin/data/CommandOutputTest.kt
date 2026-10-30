package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.CommandOutputFrameDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Cases ported from `packages/shared/src/terminalOutput.test.ts`. */
class CommandOutputTest {

    private val esc = "\u001b"

    private fun feed(chunks: List<String>, maxChars: Int = TERMINAL_OUTPUT_TAIL_CHARS) =
        chunks.fold(EMPTY_TERMINAL_OUTPUT) { state, chunk -> appendTerminalOutput(state, chunk, maxChars) }

    private fun plain(input: String) = terminalOutputPlainText(feed(listOf(input)).text)

    @Test
    fun `colours become canonical codes that reset at each line end`() {
        assertEquals("$esc[0;1;32mPASS$esc[0m ok\n", feed(listOf("$esc[1;32mPASS$esc[0m ok$esc]0;title\u0007\n")).text)
        assertEquals("$esc[0;31mone$esc[0m\n$esc[0;31mtwo$esc[0m", feed(listOf("$esc[31mone\ntwo")).text)
    }

    @Test
    fun `progress bars and cursor moves redraw in place`() {
        assertEquals("[3/3] done\nnext", feed(listOf("[1/3]", "\r[2/3]", "\r[3/3] done\nnext")).text)
        val frames =
            listOf(
                "layer a: 10%\nlayer b: 10%\n",
                "$esc[2A$esc[2Klayer a: 60%\n$esc[2Klayer b: 40%\n",
                "$esc[2A$esc[2Klayer a: done\n$esc[2Klayer b: done\n",
            )
        assertEquals("layer a: done\nlayer b: done\n", feed(frames).text)
        assertEquals("status 2\n", plain("status 1\ndetail 1\n$esc[2A$esc[Jstatus 2\n"))
        assertEquals("done", plain("progress 100%\rdone$esc[K"))
        assertEquals("aXc", plain("abc\b\bX"))
        assertEquals("12X45", plain("12345$esc[3GX"))
    }

    @Test
    fun `split sequences and CRLF wait for the next chunk`() {
        val crlf = feed(listOf("one\r", "\ntwo\r\n"))
        assertEquals("one\ntwo\n", crlf.text)
        assertEquals("", crlf.pending)
        assertEquals("red: ", appendTerminalOutput(EMPTY_TERMINAL_OUTPUT, "red: $esc[3").text)
        assertEquals("red: $esc[0;31mR$esc[0m", feed(listOf("red: $esc[3", "1mR$esc[0m")).text)
    }

    @Test
    fun `the tail is bounded, starts on a fresh line, and ignores where chunks split`() {
        val lines = (0 until 2_000).joinToString("\n") { "line $it" }
        val state = feed(listOf(lines), 1_000)
        assertTrue(state.truncated)
        assertTrue(state.text.length <= 1_000)
        assertTrue(state.text.startsWith("line "))
        assertTrue(state.text.endsWith("line 1999"))

        val input = "abcdef\rX$esc[31mred$esc[0m\nnext line\r$esc[2Kdone\n"
        val whole = feed(listOf(input), 3).text
        for (cut in 1 until input.length) {
            assertEquals(whole, feed(listOf(input.substring(0, cut), input.substring(cut)), 3).text)
        }
    }

    @Test
    fun `spans resolve colours and keep badge text readable`() {
        val text = feed(listOf("$esc[31mred$esc[0m $esc[1;38;5;196mhot$esc[0m $esc[38;2;1;2;3mrgb$esc[0m")).text
        val spans = terminalOutputSpans(text, TERMINAL_PALETTE_DARK)
        assertEquals(listOf("red", " ", "hot", " ", "rgb"), spans.map { it.text })
        assertEquals(TERMINAL_PALETTE_DARK[1], spans[0].style?.color)
        assertNull(spans[1].style)
        assertEquals(0xFFFF0000.toInt(), spans[2].style?.color)
        assertTrue(spans[2].style?.bold == true)
        assertEquals(0xFF010203.toInt(), spans[4].style?.color)

        val badges = feed(listOf("$esc[30;42m PASS $esc[0m$esc[1;37;41m FAIL $esc[0m")).text
        val (pass, fail) = terminalOutputSpans(badges, TERMINAL_PALETTE_LIGHT)
        assertEquals(0xFFFFFFFF.toInt(), pass.style?.color)
        assertEquals(0xFFFFFFFF.toInt(), fail.style?.color)

        val many = feed(listOf("$esc[31mx$esc[32my".repeat(50))).text
        val capped = terminalOutputSpans(many, TERMINAL_PALETTE_LIGHT, 10)
        assertEquals(11, capped.size)
        assertNull(capped.last().style)
        assertEquals("xy".repeat(50), capped.joinToString("") { it.text })
    }

    @Test
    fun `frames append, replace the whole tail, and report when the command settled`() {
        var view = CommandOutputView()
        view = applyCommandOutputFrame(view, CommandOutputFrameDto(kind = "replace", text = "one\n"))
        view = applyCommandOutputFrame(view, CommandOutputFrameDto(kind = "append", text = "two\n"))
        assertEquals("one\ntwo\n", view.output.text)
        assertTrue(view.running)
        view = applyCommandOutputFrame(view, CommandOutputFrameDto(kind = "replace", text = "final\n", truncated = true, running = false))
        assertEquals("final\n", view.output.text)
        assertTrue(view.output.truncated)
        assertFalse(view.running)
    }
}
