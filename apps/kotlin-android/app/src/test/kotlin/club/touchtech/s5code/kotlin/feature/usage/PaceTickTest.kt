package club.touchtech.s5code.kotlin.feature.usage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PaceTickTest {

    @Test
    fun `the tick sits at the share of the window left`() {
        assertEquals(40f, paceTickX(width = 100f, timeLeftPercent = 40, halfGap = 3f)!!, 0.001f)
    }

    @Test
    fun `a window just started or about to reset keeps its tick on the bar`() {
        assertEquals(97f, paceTickX(width = 100f, timeLeftPercent = 100, halfGap = 3f)!!, 0.001f)
        assertEquals(3f, paceTickX(width = 100f, timeLeftPercent = 0, halfGap = 3f)!!, 0.001f)
    }

    @Test
    fun `a bar narrower than the gap has no tick`() {
        assertNull(paceTickX(width = 5f, timeLeftPercent = 50, halfGap = 3f))
    }
}
