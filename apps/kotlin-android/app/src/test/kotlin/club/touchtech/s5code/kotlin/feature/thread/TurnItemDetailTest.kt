package club.touchtech.s5code.kotlin.feature.thread

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** `itemDetail.test.ts`: tool images past the preview limit keep a marker in the text. */
class TurnItemDetailTest {
    private fun image() = buildJsonObject { put("type", "image"); put("mimeType", "image/png") }

    private fun tool(output: JsonElement) = JsonObject(mapOf(
        "type" to JsonPrimitive("dynamic_tool"),
        "toolName" to JsonPrimitive("screenshots"),
        "output" to output,
    ))

    @Test
    fun `images beyond the served previews read as markers in array and content output`() {
        listOf(false, true).forEach { wrapped ->
            fun wrap(blocks: List<JsonElement>): JsonElement =
                if (wrapped) JsonObject(mapOf("content" to JsonArray(blocks))) else JsonArray(blocks)
            // Equal blocks on purpose: only the first eight are served, whatever they contain.
            val served = List(8) { image() }
            assertNull(turnItemOutputText(tool(wrap(served))))
            val overflow = wrap(served + listOf(
                buildJsonObject { put("type", "text"); put("text", "Two additional screenshots:") },
                image(),
                buildJsonObject {
                    put("type", "image")
                    put("source", buildJsonObject { put("type", "base64"); put("media_type", "image/jpeg") })
                },
            ))
            assertEquals("Two additional screenshots:\n[image]\n[image]", turnItemOutputText(tool(overflow)))
            assertEquals(8, turnItemDetailImageCount(tool(overflow)))
        }
    }
}
