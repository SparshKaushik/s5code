package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import club.touchtech.s5code.kotlin.model.ComposerImageCandidate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Attachment validation is the only place the client enforces the send-turn
 * limits, so every rejection path is pinned here rather than discovered by a
 * server error mid-turn.
 */
class ComposerAttachmentsTest {

    private fun candidate(
        uri: String,
        mimeType: String? = "image/png",
        name: String? = null,
        sizeBytes: Long? = 1_024,
    ) = ComposerImageCandidate(uri = uri, mimeType = mimeType, name = name, sizeBytes = sizeBytes)

    private fun existing(count: Int): List<ComposerAttachment> =
        (0 until count).map {
            ComposerAttachment(
                id = "content://existing/$it",
                name = "existing-$it.png",
                mimeType = "image/png",
                sizeBytes = 512,
                uri = "content://existing/$it",
            )
        }

    @Test
    fun `a supported image is accepted with its reported metadata`() {
        val result =
            acceptComposerImages(
                emptyList(),
                listOf(candidate("content://clip/1", name = "screenshot.png", sizeBytes = 2_048)),
            )

        assertNull(result.error)
        assertEquals(1, result.attachments.size)
        val attachment = result.attachments.single()
        assertEquals("screenshot.png", attachment.name)
        assertEquals("image/png", attachment.mimeType)
        assertEquals(2_048L, attachment.sizeBytes)
        assertEquals("content://clip/1", attachment.uri)
    }

    @Test
    fun `mime type comparison ignores case`() {
        val result = acceptComposerImages(emptyList(), listOf(candidate("content://clip/1", "IMAGE/JPEG")))

        assertNull(result.error)
        assertEquals("image/jpeg", result.attachments.single().mimeType)
    }

    @Test
    fun `unsupported and unknown types are refused with a reason`() {
        val svg = acceptComposerImages(emptyList(), listOf(candidate("content://clip/1", "image/svg+xml")))
        val unknown = acceptComposerImages(emptyList(), listOf(candidate("content://clip/2", null)))

        assertTrue(svg.attachments.isEmpty())
        assertTrue(svg.error!!.contains("not a supported image type"))
        assertTrue(unknown.attachments.isEmpty())
        assertNotNull(unknown.error)
    }

    @Test
    fun `oversized and empty images are refused`() {
        val tooBig =
            acceptComposerImages(
                emptyList(),
                listOf(candidate("content://clip/1", sizeBytes = ComposerAttachmentLimits.MAX_IMAGE_BYTES + 1)),
            )
        val empty = acceptComposerImages(emptyList(), listOf(candidate("content://clip/2", sizeBytes = 0)))

        assertTrue(tooBig.attachments.isEmpty())
        assertTrue(tooBig.error!!.contains("10 MB"))
        assertTrue(empty.attachments.isEmpty())
        assertNotNull(empty.error)
    }

    @Test
    fun `an unreported size is accepted as unknown rather than refused`() {
        val result = acceptComposerImages(emptyList(), listOf(candidate("content://clip/1", sizeBytes = null)))

        assertNull(result.error)
        assertEquals(0L, result.attachments.single().sizeBytes)
    }

    @Test
    fun `the send limit truncates a paste and still keeps what fits`() {
        val result =
            acceptComposerImages(
                existing(ComposerAttachmentLimits.MAX_ATTACHMENTS - 1),
                listOf(candidate("content://clip/1"), candidate("content://clip/2")),
            )

        assertEquals(1, result.attachments.size)
        assertEquals("content://clip/1", result.attachments.single().uri)
        assertTrue(result.error!!.contains("up to ${ComposerAttachmentLimits.MAX_ATTACHMENTS} images"))
    }

    @Test
    fun `a full draft refuses the whole paste`() {
        val result =
            acceptComposerImages(
                existing(ComposerAttachmentLimits.MAX_ATTACHMENTS),
                listOf(candidate("content://clip/1")),
            )

        assertTrue(result.attachments.isEmpty())
        assertNotNull(result.error)
    }

    @Test
    fun `a bad image does not drop the good ones beside it`() {
        val result =
            acceptComposerImages(
                emptyList(),
                listOf(
                    candidate("content://clip/1", name = "ok.png"),
                    candidate("content://clip/2", "application/pdf", name = "notes.pdf"),
                    candidate("content://clip/3", "image/webp", name = "also-ok.webp"),
                ),
            )

        assertEquals(listOf("ok.png", "also-ok.webp"), result.attachments.map { it.name })
        assertTrue(result.error!!.contains("notes.pdf"))
    }

    @Test
    fun `pasting the same uri twice does not duplicate the attachment`() {
        val first = acceptComposerImages(emptyList(), listOf(candidate("content://clip/1")))
        val second = acceptComposerImages(first.attachments, listOf(candidate("content://clip/1")))
        val withinOnePaste =
            acceptComposerImages(
                emptyList(),
                listOf(candidate("content://clip/1"), candidate("content://clip/1")),
            )

        assertTrue(second.attachments.isEmpty())
        assertNull(second.error)
        assertEquals(1, withinOnePaste.attachments.size)
    }

    @Test
    fun `names fall back to the uri file name then to a mime-typed label`() {
        val fromUri =
            acceptComposerImages(emptyList(), listOf(candidate("content://clip/photo.png?size=full")))
        val fromMime = acceptComposerImages(emptyList(), listOf(candidate("content://clip/9421", "image/gif")))

        assertEquals("photo.png", fromUri.attachments.single().name)
        assertEquals("pasted-image.gif", fromMime.attachments.single().name)
    }

    @Test
    fun `an empty paste is a no-op`() {
        val result = acceptComposerImages(existing(1), emptyList())

        assertTrue(result.attachments.isEmpty())
        assertNull(result.error)
    }
}
