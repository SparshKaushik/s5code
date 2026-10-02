package club.touchtech.s5code.kotlin.design.component

import androidx.compose.ui.graphics.Color
import org.junit.Assert.assertEquals
import org.junit.Test

class HighlightedTextTest {

    @Test
    fun `highlights every case-insensitive occurrence without changing source text`() {
        val result =
            highlightedText(
                text = "Kotlin kotlin KOTLIN",
                query = "koTLin",
                normalColor = Color.Gray,
                highlightColor = Color.Black,
            )

        assertEquals("Kotlin kotlin KOTLIN", result.text)
        assertEquals(
            listOf(0 until 6, 7 until 13, 14 until 20),
            result.spanStyles
                .filter { it.item.fontWeight != null }
                .map { it.start until it.end },
        )
    }

    @Test
    fun `blank query leaves text unstyled`() {
        val result = highlightedText("Thread title", "   ", Color.Gray, Color.Black)
        assertEquals("Thread title", result.text)
        assertEquals(emptyList<Any>(), result.spanStyles)
    }

    @Test
    fun `missing query keeps the full normal span`() {
        val result = highlightedText("Thread title", "branch", Color.Gray, Color.Black)
        assertEquals("Thread title", result.text)
        assertEquals(1, result.spanStyles.size)
        assertEquals(0 until result.length, result.spanStyles.single().start until result.spanStyles.single().end)
    }
}
