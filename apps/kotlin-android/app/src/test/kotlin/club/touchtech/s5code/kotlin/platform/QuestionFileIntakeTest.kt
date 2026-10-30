package club.touchtech.s5code.kotlin.platform

import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import club.touchtech.s5code.kotlin.model.QuestionAttachmentKind
import org.junit.Assert.assertEquals
import org.junit.Test

/** Question-picker sizing, after mobile's "oversized document-picked image uploads as a file" test. */
class QuestionFileIntakeTest {
    private val imageCap = ComposerAttachmentLimits.MAX_IMAGE_BYTES
    private val fileCap = 50L * 1024 * 1024

    @Test
    fun `an image over the image cap uploads as a file when the server takes files`() {
        assertEquals(fileCap, questionFileMaxBytes(isImage = true, maxFileBytes = fileCap))
        assertEquals(QuestionAttachmentKind.File, questionFileKind(isImage = true, sizeBytes = imageCap + 1))
        assertEquals(QuestionAttachmentKind.Image, questionFileKind(isImage = true, sizeBytes = imageCap))
    }

    @Test
    fun `an image-only server keeps the image cap`() {
        assertEquals(imageCap, questionFileMaxBytes(isImage = true, maxFileBytes = null))
        assertEquals(fileCap, questionFileMaxBytes(isImage = false, maxFileBytes = fileCap))
        assertEquals(QuestionAttachmentKind.File, questionFileKind(isImage = false, sizeBytes = 3))
    }
}
