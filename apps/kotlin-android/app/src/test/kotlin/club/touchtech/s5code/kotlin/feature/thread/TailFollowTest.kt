package club.touchtech.s5code.kotlin.feature.thread

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Live-follow decisions for the transcript.
 *
 * Indices are as a reversed lazy list reports them, so 0 is the newest row.
 */
class TailFollowTest {

    @Test
    fun `sitting on the newest row follows`() {
        assertTrue(shouldFollowTail(0, 0, wasFollowing = true))
        assertTrue(shouldFollowTail(0, 0, wasFollowing = false))
    }

    @Test
    fun `scrolling past the newest row stops the follow`() {
        assertFalse(shouldFollowTail(3, 0, wasFollowing = true))
    }

    @Test
    fun `coming back to the newest row resumes the follow`() {
        assertTrue(shouldFollowTail(0, 10, wasFollowing = false))
    }

    @Test
    fun `a small drift while following does not drop it`() {
        // This is the case a single threshold gets wrong: an appended row nudges the
        // offset, and follow would flicker off and on.
        assertTrue(shouldFollowTail(0, 60, wasFollowing = true))
    }

    @Test
    fun `a small drift is not enough to resume a follow the user cancelled`() {
        assertFalse(shouldFollowTail(0, 60, wasFollowing = false))
    }

    @Test
    fun `a deliberate scroll away from the edge drops the follow`() {
        assertFalse(shouldFollowTail(0, 400, wasFollowing = true))
    }

    @Test
    fun `the resume threshold is tighter than the exit threshold`() {
        // Stated as a property so the two constants cannot be edited into one.
        val nudge = 60
        assertTrue(shouldFollowTail(0, nudge, wasFollowing = true))
        assertFalse(shouldFollowTail(0, nudge, wasFollowing = false))
    }
}
