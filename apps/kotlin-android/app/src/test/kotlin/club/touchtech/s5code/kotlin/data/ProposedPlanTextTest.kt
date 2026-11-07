package club.touchtech.s5code.kotlin.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The plan card's title and body split, ported from `proposedPlanText.ts`. */
class ProposedPlanTextTest {
    @Test
    fun `the first heading titles the plan`() {
        assertEquals("Ship the fix", proposedPlanTitle("Intro\n\n## Ship the fix\n- step"))
        assertNull(proposedPlanTitle("- step one\n- step two"))
    }

    @Test
    fun `the body drops the title and a summary heading under it`() {
        assertEquals(
            "Do the thing.\n\n## Steps\n- one",
            stripDisplayedPlanMarkdown("# Plan\n\n## Summary\n\nDo the thing.\n\n## Steps\n- one\n\n"),
        )
    }

    @Test
    fun `a plan without a leading heading keeps its first line`() {
        assertEquals("Do the thing.\n# Later", stripDisplayedPlanMarkdown("Do the thing.\n# Later"))
    }

    @Test
    fun `saved contents end in exactly one newline`() {
        assertEquals("# Plan\n", proposedPlanFileContents("# Plan  \n\n\n"))
    }
}
