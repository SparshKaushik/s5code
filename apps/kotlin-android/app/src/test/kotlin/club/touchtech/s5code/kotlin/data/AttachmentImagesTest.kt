package club.touchtech.s5code.kotlin.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

class AttachmentImagesTest {

    @Test
    fun `the cache key matches mobile's JSON-encoded pair`() {
        assertEquals("t3-attachment:[\"env-1\",\"att-1\"]", attachmentImageCacheKey("env-1", "att-1"))
    }

    @Test
    fun `the same attachment id on two environments caches separately`() {
        assertNotEquals(attachmentImageCacheKey("env-1", "att-1"), attachmentImageCacheKey("env-2", "att-1"))
    }

    @Test
    fun `ids that contain separators cannot collide`() {
        assertNotEquals(attachmentImageCacheKey("a\",\"b", "c"), attachmentImageCacheKey("a", "b\",\"c"))
    }
}
