package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ComposerAttachment
import org.junit.Assert.assertEquals
import org.junit.Test

class ThreadTitleTest {

    private val image = "![pasted-image.png](t3-context://v1/image/aadd9cc6-1234-5678-9012-abcdef123456)"

    private fun attachment(type: String, name: String) =
        ComposerAttachment(id = name, name = name, mimeType = "", sizeBytes = 1, uri = "", type = type)

    @Test
    fun `image-only prompts get a readable title`() {
        assertEquals("Attached image", deriveThreadTitleSeed(image, emptyList()))
        assertEquals(
            "Attached images",
            deriveThreadTitleSeed("$image\n![other.png](t3-context://v1/image/other)", emptyList()),
        )
    }

    @Test
    fun `context labels and prose win over attachments`() {
        assertEquals(
            "src/app.ts",
            deriveThreadTitleSeed("$image\n[src/app.ts](t3-context://v1/mention/source)", listOf(attachment("image", "a.png"))),
        )
        assertEquals(
            "Fix src/app.ts using Build output",
            deriveThreadTitleSeed(
                "  Fix $image [src/app.ts](t3-context://v1/mention/source)\n  using [Build output](t3-context://v1/terminal/build)  ",
                listOf(attachment("image", "login-error.png")),
            ),
        )
    }

    @Test
    fun `empty text falls back to attachments, then New thread`() {
        assertEquals("Attached image", deriveThreadTitleSeed("", listOf(attachment("image", "login-error.png"))))
        assertEquals("File: report.pdf", deriveThreadTitleSeed("  ", listOf(attachment("file", "report.pdf"))))
        assertEquals("New thread", deriveThreadTitleSeed("", emptyList()))
    }

    @Test
    fun `long prose truncates at fifty characters`() {
        val title = deriveThreadTitleSeed("x".repeat(60), emptyList())
        assertEquals("x".repeat(50) + "...", title)
    }
}
