package club.touchtech.s5code.kotlin.feature.thread

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The composer stays one row until the draft would wrap, and returns once it fits. */
class ComposerRowTest {
    @Test
    fun `a one-line draft that fits the row stays in the row`() {
        assertFalse(draftOverflowsRow(lineCount = 1, oneLineWidth = 180f, rowTextWidth = 200f))
    }

    @Test
    fun `a draft that wrapped where it is laid out leaves the row`() {
        assertTrue(draftOverflowsRow(lineCount = 2, oneLineWidth = 0f, rowTextWidth = 200f))
    }

    @Test
    fun `a draft laid out in the wider card stays there until it fits the row`() {
        assertTrue(draftOverflowsRow(lineCount = 1, oneLineWidth = 260f, rowTextWidth = 200f))
        assertFalse(draftOverflowsRow(lineCount = 1, oneLineWidth = 150f, rowTextWidth = 200f))
    }

    @Test
    fun `an unmeasured row only goes by line count`() {
        assertFalse(draftOverflowsRow(lineCount = 1, oneLineWidth = 500f, rowTextWidth = 0f))
    }
}
