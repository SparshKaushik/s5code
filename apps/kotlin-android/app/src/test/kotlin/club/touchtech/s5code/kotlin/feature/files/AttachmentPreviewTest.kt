package club.touchtech.s5code.kotlin.feature.files

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The classification and decode rules are ported from `filePreviewKind` /
 * `decodeFilePreviewText` in `packages/shared`; these cases pin the precedence
 * that matters — mime first, extension only when the mime is generic.
 */
class AttachmentPreviewTest {
    @Test
    fun `a definite mime beats a misleading extension`() {
        assertEquals(
            AttachmentPreviewKind.Pdf,
            attachmentPreviewKind("recording.mp4", "application/pdf"),
        )
        assertEquals(
            AttachmentPreviewKind.Image,
            attachmentPreviewKind("notes.txt", "image/png"),
        )
    }

    @Test
    fun `generic mimes fall back to the extension`() {
        assertEquals(
            AttachmentPreviewKind.Video,
            attachmentPreviewKind("clip.MP4", "application/octet-stream"),
        )
        assertEquals(
            AttachmentPreviewKind.Markdown,
            attachmentPreviewKind("README.md", "text/plain"),
        )
        assertEquals(
            AttachmentPreviewKind.Text,
            attachmentPreviewKind("Dockerfile", "application/octet-stream"),
        )
        assertEquals(
            AttachmentPreviewKind.Audio,
            attachmentPreviewKind("voice.m4a", ""),
        )
    }

    @Test
    fun `structured application types read as text`() {
        assertEquals(
            AttachmentPreviewKind.Text,
            attachmentPreviewKind("payload", "application/json"),
        )
        assertEquals(
            AttachmentPreviewKind.Text,
            attachmentPreviewKind("feed", "application/atom+xml"),
        )
        assertEquals(
            AttachmentPreviewKind.Text,
            attachmentPreviewKind("page", "text/css"),
        )
    }

    @Test
    fun `html and unknown types hand off to the system`() {
        assertEquals(
            AttachmentPreviewKind.Html,
            attachmentPreviewKind("page.html", "text/html"),
        )
        assertEquals(
            AttachmentPreviewKind.Unsupported,
            attachmentPreviewKind("archive.zip", "application/zip"),
        )
    }

    @Test
    fun `text decodes and reports the truncation flag`() {
        val preview = decodeAttachmentPreviewText("hello".encodeToByteArray(), truncated = true)
        assertEquals("hello", preview.text)
        assertTrue(preview.truncated)
        assertFalse(
            decodeAttachmentPreviewText("hello".encodeToByteArray(), truncated = false).truncated
        )
    }

    @Test
    fun `binary bytes are rejected rather than rendered`() {
        assertThrows(IllegalArgumentException::class.java) {
            decodeAttachmentPreviewText(byteArrayOf(0x50, 0x4B, 0x00, 0x00), truncated = false)
        }
        assertThrows(IllegalArgumentException::class.java) {
            // A lone continuation byte is malformed UTF-8.
            decodeAttachmentPreviewText(byteArrayOf(0x80.toByte()), truncated = false)
        }
    }

    @Test
    fun `size labels match the RN formatter`() {
        assertEquals("1 KB", attachmentSizeLabel(0))
        assertEquals("2 KB", attachmentSizeLabel(1536))
        assertEquals("1.5 MB", attachmentSizeLabel(1536 * 1024))
        assertEquals("10.0 MB", attachmentSizeLabel(10L * 1024 * 1024))
    }
}
