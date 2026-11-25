package club.touchtech.s5code.kotlin.data

import org.junit.Assert.assertEquals
import org.junit.Test

class ModelShortNameTest {

    @Test
    fun `matches the upstream short name cases`() {
        val cases =
            listOf(
                Triple("Claude Opus 5.5", listOf("Claude "), "Opus 5.5"),
                Triple("GPT-6.1-Sol", listOf("GPT-"), "6.1 Sol"),
                Triple("GPT 6.0 Astra", listOf("GPT-", "GPT "), "6.0 Astra"),
                Triple("Grok 4 Pro", emptyList(), null),
                Triple("Claude Opus 5.5", emptyList(), "Opus 5.5"),
                Triple("NVIDIA: Nemotron 3 Super", emptyList(), "Nemotron 3 Super"),
                Triple("GPT-5.5", listOf("GPT-"), null),
                Triple("Gemini 3.1 Pro", listOf("Gemini "), "3.1 Pro"),
            )
        cases.forEach { (name, families, expected) ->
            assertEquals(name, expected, modelShortName(name, families))
        }
    }

    @Test
    fun `the server's short name wins and the driver picks the native families`() {
        assertEquals("Sol", resolveModelShortName("GPT-6.1-Sol", "Sol", "codex"))
        assertEquals("4 Pro", resolveModelShortName("Grok 4 Pro", null, "grok"))
        assertEquals(null, resolveModelShortName("Grok 4 Pro", null, "opencode"))
        assertEquals(null, resolveModelShortName("Grok 4 Pro", null, null))
    }
}
