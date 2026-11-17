package club.touchtech.s5code.kotlin.platform.updates

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SemVerTest {

    @Test
    fun `parses clean and prefixed semantic versions`() {
        val v1 = SemVer.parse("0.1.0")
        assertNotNull(v1)
        assertEquals(0, v1?.major)
        assertEquals(1, v1?.minor)
        assertEquals(0, v1?.patch)
        assertNull(v1?.preRelease)

        val v2 = SemVer.parse("v1.2.3")
        assertNotNull(v2)
        assertEquals(1, v2?.major)
        assertEquals(2, v2?.minor)
        assertEquals(3, v2?.patch)

        val v3 = SemVer.parse("0.1.0-alpha.1")
        assertNotNull(v3)
        assertEquals("alpha.1", v3?.preRelease)

        val v4 = SemVer.parse("kotlin-android-v0.1.0-alpha.1-debug")
        assertNotNull(v4)
        assertEquals(0, v4?.major)
        assertEquals(1, v4?.minor)
        assertEquals(0, v4?.patch)
        assertEquals("alpha.1", v4?.preRelease)
    }

    @Test
    fun `compares major minor and patch versions`() {
        assertTrue(SemVer(0, 1, 0) < SemVer(0, 1, 1))
        assertTrue(SemVer(0, 1, 9) < SemVer(0, 2, 0))
        assertTrue(SemVer(0, 9, 9) < SemVer(1, 0, 0))
        assertEquals(0, SemVer(1, 2, 3).compareTo(SemVer(1, 2, 3)))
    }

    @Test
    fun `pre-release versions have lower precedence than normal releases`() {
        assertTrue(SemVer(0, 1, 0, "alpha.1") < SemVer(0, 1, 0))
        assertTrue(SemVer(0, 1, 0, "beta.2") < SemVer(0, 1, 0))
        assertTrue(SemVer(1, 0, 0, "rc.1") < SemVer(1, 0, 0))
    }

    @Test
    fun `compares pre-release segments numerically and lexicographically`() {
        assertTrue(SemVer(0, 1, 0, "alpha.1") < SemVer(0, 1, 0, "alpha.2"))
        assertTrue(SemVer(0, 1, 0, "alpha.9") < SemVer(0, 1, 0, "alpha.10"))
        assertTrue(SemVer(0, 1, 0, "alpha.1") < SemVer(0, 1, 0, "beta.1"))
    }

    @Test
    fun `rejects invalid version strings`() {
        assertNull(SemVer.parse(""))
        assertNull(SemVer.parse("not-a-version"))
        assertNull(SemVer.parse("1.2"))
    }
}
