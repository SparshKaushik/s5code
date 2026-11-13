package club.touchtech.s5code.kotlin.design.component

import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ProjectFaviconCacheTest {
    @Test
    fun `cache stays bounded and evicts least recently used entry`() {
        val cache = BoundedLruCache<String, String>(maximumSize = 2)
        cache["one"] = "1"
        cache["two"] = "2"
        cache["one"] // Make one most recently used.
        cache["three"] = "3"

        assertTrue("one" in cache)
        assertTrue("three" in cache)
        assertTrue("two" !in cache)
        assertNotEquals(3, cache.size)
    }
}
