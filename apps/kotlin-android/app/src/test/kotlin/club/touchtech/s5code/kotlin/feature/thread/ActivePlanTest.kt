package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.model.FeedEntry
import club.touchtech.s5code.kotlin.model.PlanStep
import club.touchtech.s5code.kotlin.model.PlanStepState
import club.touchtech.s5code.kotlin.model.ThreadStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The pinned plan bar reduces a whole transcript to one line, so what it picks
 * matters: the wrong step makes the header lie about what the agent is doing.
 */
class ActivePlanTest {

    private fun plan(id: String, vararg steps: Pair<String, PlanStepState>) =
        FeedEntry.PlanUpdate(id, steps.map { PlanStep(it.first, it.second) })

    @Test
    fun `no plan in the feed means no bar`() {
        val feed =
            listOf(
                FeedEntry.UserMessage("u1", "go", "09:00"),
                FeedEntry.AgentMessage("a1", "working", "09:01"),
            )
        assertNull(activePlan(feed))
    }

    @Test
    fun `an empty plan means no bar`() {
        assertNull(activePlan(listOf(plan("p1"))))
    }

    @Test
    fun `the active step wins`() {
        val result =
            activePlan(
                listOf(
                    plan(
                        "p1",
                        "read the tracker" to PlanStepState.Done,
                        "build the screens" to PlanStepState.Active,
                        "sign the apk" to PlanStepState.Pending,
                    )
                )
            )
        assertEquals("build the screens", result?.step?.text)
        assertEquals(1, result?.done)
        assertEquals(3, result?.total)
    }

    @Test
    fun `with nothing active the first pending step is next up`() {
        val result =
            activePlan(
                listOf(
                    plan(
                        "p1",
                        "read the tracker" to PlanStepState.Done,
                        "build the screens" to PlanStepState.Pending,
                        "sign the apk" to PlanStepState.Pending,
                    )
                )
            )
        assertEquals("build the screens", result?.step?.text)
    }

    @Test
    fun `a finished plan shows its last step rather than disappearing`() {
        val result =
            activePlan(
                listOf(plan("p1", "a" to PlanStepState.Done, "b" to PlanStepState.Done))
            )
        assertEquals("b", result?.step?.text)
        assertEquals(2, result?.done)
        assertEquals(2, result?.total)
    }

    @Test
    fun `the newest plan supersedes earlier copies of itself`() {
        // A provider republishes the whole plan on every change, so an earlier
        // PlanUpdate is a stale snapshot and must not win.
        val feed =
            listOf(
                plan("p1", "one" to PlanStepState.Active, "two" to PlanStepState.Pending),
                FeedEntry.AgentMessage("a1", "done with one", "09:02"),
                plan("p2", "one" to PlanStepState.Done, "two" to PlanStepState.Active),
            )
        val result = activePlan(feed)
        assertEquals("two", result?.step?.text)
        assertEquals(1, result?.done)
    }
}

/**
 * The tall header belongs to the top of the thread, and the header's own height
 * feeds back into what is visible. Without hysteresis that loop oscillates, so the
 * dead zone is the behavior under test.
 */
class HistoryTopTest {

    @Test
    fun `the oldest entry in view is the top`() {
        assertTrue(atHistoryTop(lastVisibleIndex = 9, lastIndex = 9, wasAtTop = false))
    }

    @Test
    fun `the live tail of a long thread is not the top`() {
        // Reversed list: index 0 is the newest entry, so a full screen at the
        // bottom sees only the low indices.
        assertFalse(atHistoryTop(lastVisibleIndex = 3, lastIndex = 40, wasAtTop = false))
    }

    @Test
    fun `an empty transcript has no top to be at`() {
        assertFalse(atHistoryTop(lastVisibleIndex = -1, lastIndex = -1, wasAtTop = true))
    }

    @Test
    fun `a thread short enough to fit is at the top immediately`() {
        assertTrue(atHistoryTop(lastVisibleIndex = 2, lastIndex = 2, wasAtTop = false))
    }

    @Test
    fun `one item of slack keeps the expanded header still`() {
        // Expanding the header shortens the viewport, which can push the oldest
        // entry out of view. That must not immediately collapse it again.
        assertTrue(atHistoryTop(lastVisibleIndex = 8, lastIndex = 9, wasAtTop = true))
    }

    @Test
    fun `re-entering needs the oldest entry fully back in view`() {
        assertFalse(atHistoryTop(lastVisibleIndex = 8, lastIndex = 9, wasAtTop = false))
        assertTrue(atHistoryTop(lastVisibleIndex = 9, lastIndex = 9, wasAtTop = false))
    }

    @Test
    fun `scrolling well away leaves the top whatever it was`() {
        assertFalse(atHistoryTop(lastVisibleIndex = 4, lastIndex = 40, wasAtTop = true))
    }
}

/**
 * The plan strip claims the agent is doing something right now, so the gate is
 * about honesty rather than layout: a finished turn has no current step.
 */
class PlanBarAppliesTest {

    @Test
    fun `a live turn shows the strip`() {
        assertTrue(planBarApplies(ThreadStatus.Working))
        assertTrue(planBarApplies(ThreadStatus.Queued))
    }

    @Test
    fun `a blocked turn is still mid-plan`() {
        assertTrue(planBarApplies(ThreadStatus.AwaitingApproval))
        assertTrue(planBarApplies(ThreadStatus.AwaitingInput))
    }

    @Test
    fun `a finished turn hides it`() {
        assertFalse(planBarApplies(ThreadStatus.Idle))
        assertFalse(planBarApplies(ThreadStatus.Settled))
        assertFalse(planBarApplies(ThreadStatus.Snoozed))
        assertFalse(planBarApplies(ThreadStatus.Failed))
    }
}
