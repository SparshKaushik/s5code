package club.touchtech.s5code.kotlin.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ImageAttachmentSizeTest {

    @Test
    fun `images within the edge limit are left alone`() {
        assertNull(fitImageAttachmentSize(2000, 1500))
        assertNull(fitImageAttachmentSize(10, 2000))
    }

    @Test
    fun `the longest edge shrinks to the limit and the other keeps the ratio`() {
        assertEquals(2000 to 1500, fitImageAttachmentSize(4000, 3000))
        assertEquals(1125 to 2000, fitImageAttachmentSize(2250, 4000))
        // Rounds half up like JS Math.round, and never collapses to zero.
        assertEquals(2000 to 1, fitImageAttachmentSize(100_000, 10))
        assertEquals(2000 to 3, fitImageAttachmentSize(4000, 5))
    }

    @Test
    fun `only png and jpeg are re-encoded`() {
        assertEquals("image/jpeg", downscalableImageMimeType("IMAGE/JPG"))
        assertEquals("image/png", downscalableImageMimeType("image/png"))
        assertNull(downscalableImageMimeType("image/gif"))
        assertNull(downscalableImageMimeType("image/webp"))
    }
}
