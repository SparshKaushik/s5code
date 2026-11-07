package club.touchtech.s5code.kotlin.platform.terminal

import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class TerminalFrameTest {

    @Test
    fun `decodes minimal 30-byte header for empty frame`() {
        val bytes = ByteArray(30)
        val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
        buffer.putInt(0x54563354) // MAGIC "T3VT"
        buffer.putShort(1.toShort()) // VERSION
        buffer.putShort(0.toShort()) // cols = 0
        buffer.putShort(0.toShort()) // rows = 0
        buffer.putShort(0.toShort()) // cursorX = 0
        buffer.putShort(0.toShort()) // cursorY = 0
        buffer.put(0.toByte()) // cursorVisible = false
        buffer.put(0.toByte()) // cursorStyle = 0
        buffer.put(0.toByte()) // cursorBlinking = false
        buffer.put(0.toByte()) // padding = 0
        buffer.putInt(0xFFFFFFFF.toInt()) // foreground
        buffer.putInt(0xFF000000.toInt()) // background
        buffer.putInt(0xFF009FFF.toInt()) // cursorColor

        val frame = TerminalFrame.decode(bytes)
        assertNotNull(frame)
        assertEquals(0, frame?.cols)
        assertEquals(0, frame?.rows)
        assertEquals(false, frame?.cursorVisible)
        assertEquals(0, frame?.cursorX)
        assertEquals(0, frame?.cursorY)
    }

    @Test
    fun `rejects frame shorter than 30-byte header`() {
        val truncated = ByteArray(29)
        assertNull(TerminalFrame.decode(truncated))
    }

    @Test
    fun `rejects invalid magic or version`() {
        val bytes = ByteArray(30)
        val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
        buffer.putInt(0x12345678) // Invalid magic
        buffer.putShort(1.toShort())
        assertNull(TerminalFrame.decode(bytes))
    }
}
