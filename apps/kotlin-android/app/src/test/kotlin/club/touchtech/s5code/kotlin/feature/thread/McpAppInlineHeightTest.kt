package club.touchtech.s5code.kotlin.feature.thread

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** An MCP app's reported height, clamped for the inline card. */
class McpAppInlineHeightTest {

    @Test
    fun `heights are rounded and clamped to the inline bounds`() {
        assertEquals(80, mcpAppInlineHeight(-10.0))
        assertEquals(80, mcpAppInlineHeight(0.0))
        assertEquals(150, mcpAppInlineHeight(149.6))
        assertEquals(MCP_APP_INLINE_MAX_HEIGHT, mcpAppInlineHeight(420.0))
        assertEquals(MCP_APP_INLINE_MAX_HEIGHT, mcpAppInlineHeight(2_000.0))
    }

    @Test
    fun `an unusable height keeps the current size`() {
        assertNull(mcpAppInlineHeight(null))
        assertNull(mcpAppInlineHeight(Double.NaN))
        assertNull(mcpAppInlineHeight(Double.POSITIVE_INFINITY))
    }
}
