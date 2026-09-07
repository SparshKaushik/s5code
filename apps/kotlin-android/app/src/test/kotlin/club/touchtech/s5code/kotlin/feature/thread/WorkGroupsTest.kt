package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.model.FeedEntry
import club.touchtech.s5code.kotlin.model.ToolState
import club.touchtech.s5code.kotlin.model.TurnInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Folding runs of work.
 *
 * The rules that matter are about what must *not* fold. A subagent hidden behind
 * "+7 previous tool calls" is exactly the stall the subagent row exists to explain,
 * and a folded error is an error the user does not know about.
 */
class WorkGroupsTest {

    private fun tool(id: String, state: ToolState = ToolState.Succeeded) =
        FeedEntry.ToolCall(id = id, name = "Bash", summary = "ls", detail = "", state = state)

    private fun message(id: String) = FeedEntry.AgentMessage(id, "text", "12:00")

    private fun subagent(id: String) =
        FeedEntry.Subagent(id = id, name = "explorer", task = "read files", active = true)

    private fun keys(rows: List<FeedRow>) = rows.map { it.key }

    @Test
    fun `a short run is left alone`() {
        val feed = listOf(message("m1"), tool("t1"), message("m2"))
        assertEquals(listOf("m1", "t1", "m2"), keys(presentFeed(feed, emptySet())))
    }

    @Test
    fun `a long run folds to the newest row plus a toggle`() {
        val feed = listOf(message("m1"), tool("t1"), tool("t2"), tool("t3"), message("m2"))
        val rows = presentFeed(feed, emptySet())
        assertEquals(listOf("m1", "t3", "work-toggle:work-group:t1", "m2"), keys(rows))
        val toggle = rows.filterIsInstance<FeedRow.WorkToggle>().single()
        assertEquals(2, toggle.hiddenCount)
        assertTrue(toggle.onlyTools)
    }

    @Test
    fun `expanding a group restores every row and keeps the toggle`() {
        val feed = listOf(tool("t1"), tool("t2"), tool("t3"))
        val rows = presentFeed(feed, setOf("work-group:t1"))
        assertEquals(listOf("t1", "t2", "t3", "work-toggle:work-group:t1"), keys(rows))
        assertTrue(rows.filterIsInstance<FeedRow.WorkToggle>().single().expanded)
    }

    @Test
    fun `messages break a run so two short runs never merge`() {
        val feed = listOf(tool("t1"), message("m1"), tool("t2"))
        assertEquals(listOf("t1", "m1", "t2"), keys(presentFeed(feed, emptySet())))
    }

    @Test
    fun `a plan card is never swallowed into a fold`() {
        val feed = listOf(tool("t1"), tool("t2"), FeedEntry.PlanUpdate("p1", emptyList()), tool("t3"))
        val rows = keys(presentFeed(feed, emptySet()))
        assertTrue(rows.contains("p1"))
        // Two tools before the plan is one over the visible budget, so only that
        // run folds; the single tool after it stays.
        assertEquals(listOf("t2", "work-toggle:work-group:t1", "p1", "t3"), rows)
    }

    @Test
    fun `an error row is never folded away`() {
        val feed = listOf(tool("t1"), FeedEntry.ErrorEntry("e1", "boom"), tool("t2"), tool("t3"))
        val rows = keys(presentFeed(feed, emptySet()))
        assertTrue(rows.contains("e1"))
    }

    @Test
    fun `a subagent inside a folded run stays visible in place`() {
        val feed = listOf(tool("t1"), subagent("s1"), tool("t2"), tool("t3"))
        val rows = keys(presentFeed(feed, emptySet()))
        // s1 keeps its chronological slot rather than being pushed above the tools
        // that ran before it.
        assertEquals(listOf("s1", "t3", "work-toggle:work-group:t1"), rows)
    }

    @Test
    fun `a run with a subagent counts log entries rather than tool calls`() {
        val feed = listOf(tool("t1"), tool("t2"), subagent("s1"), tool("t3"))
        val toggle = presentFeed(feed, emptySet()).filterIsInstance<FeedRow.WorkToggle>().single()
        assertTrue(!toggle.onlyTools)
        assertEquals("+2 previous log entries", workToggleLabel(toggle))
    }

    @Test
    fun `subagents alone never fold however many there are`() {
        val feed = listOf(subagent("s1"), subagent("s2"), subagent("s3"))
        assertEquals(listOf("s1", "s2", "s3"), keys(presentFeed(feed, emptySet())))
    }

    @Test
    fun `the group key follows the first row so a growing run stays open`() {
        val open = setOf("work-group:t1")
        val before = presentFeed(listOf(tool("t1"), tool("t2"), tool("t3")), open)
        val after = presentFeed(listOf(tool("t1"), tool("t2"), tool("t3"), tool("t4")), open)
        assertTrue(before.filterIsInstance<FeedRow.WorkToggle>().single().expanded)
        assertTrue(after.filterIsInstance<FeedRow.WorkToggle>().single().expanded)
        assertEquals(4, after.filterIsInstance<FeedRow.Entry>().size)
    }

    @Test
    fun `toggle labels read correctly at one hidden row and when expanded`() {
        val one =
            FeedRow.WorkToggle(groupId = "g", hiddenCount = 1, expanded = false, onlyTools = true)
        assertEquals("+1 previous tool call", workToggleLabel(one))
        assertEquals("Show fewer tool calls", workToggleLabel(one.copy(expanded = true)))
        val mixed = one.copy(onlyTools = false)
        assertEquals("+1 previous log entry", workToggleLabel(mixed))
        assertEquals("Show fewer log entries", workToggleLabel(mixed.copy(expanded = true)))
    }

    @Test
    fun `an empty feed presents nothing`() {
        assertTrue(presentFeed(emptyList(), emptySet()).isEmpty())
    }

    @Test
    fun `a run at the very end of the feed still folds`() {
        val feed = listOf(message("m1"), tool("t1"), tool("t2"))
        assertEquals(
            listOf("m1", "t2", "work-toggle:work-group:t1"),
            keys(presentFeed(feed, emptySet())),
        )
    }
}

/**
 * Folding a whole finished turn down to its answer.
 *
 * The exclusions are the point. A turn that is still running or streaming must stay
 * open, because folding work in flight hides the only evidence anything is
 * happening; and a turn whose answer is all there is has nothing to disclose.
 */
class TurnFoldTest {

    private fun tool(id: String, turnId: String?, at: Long = 0) =
        FeedEntry.ToolCall(
            id = id,
            name = "Bash",
            summary = "ls",
            detail = "",
            state = ToolState.Succeeded,
            turnId = turnId,
            atMillis = at,
        )

    private fun answer(
        id: String,
        turnId: String?,
        at: Long = 0,
        end: Long = at,
        streaming: Boolean = false,
    ) =
        FeedEntry.AgentMessage(
            id = id,
            markdown = "done",
            timeLabel = "12:00",
            streaming = streaming,
            turnId = turnId,
            atMillis = at,
            endedAtMillis = end,
        )

    private fun prompt(id: String, at: Long = 0) =
        FeedEntry.UserMessage(id = id, text = "go", timeLabel = "12:00", atMillis = at)

    private fun keys(rows: List<FeedRow>) = rows.map { it.key }

    private fun settled(turnId: String, started: Long = 0, completed: Long = 0) =
        TurnInfo(turnId, "completed", started, completed)

    @Test
    fun `a finished turn collapses to its answer under a header`() {
        val feed =
            listOf(
                prompt("u1", 1_000),
                tool("t1", "turn-1", 2_000),
                tool("t2", "turn-1", 3_000),
                answer("a1", "turn-1", 4_000),
            )
        assertEquals(
            listOf("u1", "turn-fold:turn-1:header", "a1"),
            keys(presentFeed(feed, emptySet(), settled("turn-1", 1_000, 4_000))),
        )
    }

    @Test
    fun `expanding a turn restores its work and keeps the header`() {
        val feed = listOf(prompt("u1"), tool("t1", "turn-1"), answer("a1", "turn-1"))
        assertEquals(
            listOf("u1", "turn-fold:turn-1:header", "t1", "a1", "turn-fold:turn-1:footer"),
            keys(presentFeed(feed, emptySet(), settled("turn-1"), setOf("turn-1"))),
        )
    }

    @Test
    fun `an expanded turn has a distinct footer fold affordance`() {
        val feed = listOf(prompt("u1"), tool("t1", "turn-1"), answer("a1", "turn-1"))
        val rows = presentFeed(feed, emptySet(), settled("turn-1"), setOf("turn-1"))
        assertEquals("turn-fold:turn-1:footer", rows.last().key)
        assertEquals(2, rows.count { it is FeedRow.TurnFold })
        // The footer names the action; repeating the duration would read as a second
        // turn having happened.
        val folds = rows.filterIsInstance<FeedRow.TurnFold>()
        assertEquals("Worked", folds.first().text)
        assertEquals("Hide this turn's work", folds.last().text)
    }

    @Test
    fun `an expanded turn ending in tool calls still has both triggers`() {
        // The bug this pins: the footer was only attached after a non-work row or
        // after a *folded* run, so a turn whose last rows were a short run of tool
        // calls got a trigger at the top and nothing at the bottom — exactly the
        // long turn that needs one.
        val feed = listOf(prompt("u1"), answer("a1", "turn-1"), tool("t1", "turn-1"))
        val rows = presentFeed(feed, emptySet(), settled("turn-1"), setOf("turn-1"))
        assertEquals(
            listOf("u1", "turn-fold:turn-1:header", "a1", "t1", "turn-fold:turn-1:footer"),
            keys(rows),
        )
    }

    @Test
    fun `an expanded turn ending in a folded run puts the footer after the toggle`() {
        val feed =
            listOf(
                prompt("u1"),
                answer("a1", "turn-1"),
                tool("t1", "turn-1"),
                tool("t2", "turn-1"),
                tool("t3", "turn-1"),
            )
        // The work toggle belongs to the rows above it, so the turn trigger — which
        // closes everything — comes last.
        assertEquals(
            listOf(
                "u1",
                "turn-fold:turn-1:header",
                "a1",
                "t1",
                "t2",
                "t3",
                "work-toggle:work-group:t1",
                "turn-fold:turn-1:footer",
            ),
            keys(presentFeed(feed, setOf("work-group:t1"), settled("turn-1"), setOf("turn-1"))),
        )
    }

    @Test
    fun `a collapsed turn has one trigger`() {
        // Nothing is shown, so there is no bottom to reach: a second trigger would
        // be two controls one row apart doing the same thing.
        val feed = listOf(prompt("u1"), tool("t1", "turn-1"), answer("a1", "turn-1"))
        val rows = presentFeed(feed, emptySet(), settled("turn-1"))
        assertEquals(1, rows.count { it is FeedRow.TurnFold })
    }
    @Test
    fun `the open turn is never folded`() {
        val feed = listOf(prompt("u1"), tool("t1", "turn-1"), answer("a1", "turn-1"))
        val running = TurnInfo("turn-1", "running", 1_000, null)
        assertEquals(listOf("u1", "t1", "a1"), keys(presentFeed(feed, emptySet(), running)))
    }

    @Test
    fun `a turn whose answer is still streaming is never folded`() {
        val feed =
            listOf(prompt("u1"), tool("t1", "turn-1"), answer("a1", "turn-1", streaming = true))
        // The turn record says settled, but the message says otherwise; the message
        // wins, because that is what is visibly still changing on screen.
        assertEquals(listOf("u1", "t1", "a1"), keys(presentFeed(feed, emptySet(), settled("turn-1"))))
    }

    @Test
    fun `a turn that is only its answer gets no header`() {
        val feed = listOf(prompt("u1"), answer("a1", "turn-1"))
        assertEquals(listOf("u1", "a1"), keys(presentFeed(feed, emptySet(), settled("turn-1"))))
    }

    @Test
    fun `a turn with no answer folds everything away`() {
        // A turn that only ran tools and said nothing still gets a header: the work
        // happened, and "Worked for 2s" with nothing under it is the honest summary.
        val feed = listOf(prompt("u1", 0), tool("t1", "turn-1", 1_000), tool("t2", "turn-1", 2_000))
        assertEquals(
            listOf("u1", "turn-fold:turn-1:header"),
            keys(presentFeed(feed, emptySet(), settled("turn-1", 0, 2_000))),
        )
    }

    @Test
    fun `older turns fold while the open one stays expanded`() {
        val feed =
            listOf(
                prompt("u1"),
                tool("t1", "turn-1"),
                answer("a1", "turn-1"),
                prompt("u2"),
                tool("t2", "turn-2"),
                answer("a2", "turn-2"),
            )
        val running = TurnInfo("turn-2", "running", 5_000, null)
        assertEquals(
            listOf("u1", "turn-fold:turn-1:header", "a1", "u2", "t2", "a2"),
            keys(presentFeed(feed, emptySet(), running)),
        )
    }

    @Test
    fun `rows with no turn id are never folded`() {
        // Some providers attribute nothing. Those rows must stay: there is no header
        // that would ever bring them back.
        val feed = listOf(prompt("u1"), tool("t1", null), answer("a1", null))
        assertEquals(listOf("u1", "t1", "a1"), keys(presentFeed(feed, emptySet(), settled("turn-1"))))
    }

    @Test
    fun `work folding and turn folding do not both hide the same rows`() {
        val feed =
            listOf(
                prompt("u1"),
                tool("t1", "turn-1"),
                tool("t2", "turn-1"),
                tool("t3", "turn-1"),
                answer("a1", "turn-1"),
            )
        // Folded turn: no work-group toggle at all, since the turn header already
        // covers every one of those rows. Two toggles for the same rows would be two
        // controls that disagree.
        assertEquals(
            listOf("u1", "turn-fold:turn-1:header", "a1"),
            keys(presentFeed(feed, emptySet(), settled("turn-1"))),
        )
        // Expanded turn: the run is long enough that the inner fold applies again.
        assertEquals(
            listOf(
                "u1",
                "turn-fold:turn-1:header",
                "t3",
                "work-toggle:work-group:t1",
                "a1",
                "turn-fold:turn-1:footer",
            ),
            keys(presentFeed(feed, emptySet(), settled("turn-1"), setOf("turn-1"))),
        )
    }

    @Test
    fun `the label measures from the prompt, not the first tool call`() {
        val feed =
            listOf(prompt("u1", 0), tool("t1", "turn-1", 30_000), answer("a1", "turn-1", 65_000))
        // No turn record, so the boundary is the user's message: 65s, not 35s.
        val fold = turnFolds(feed, null).values.single()
        assertEquals("Worked for 1m 5s", fold.label)
    }

    @Test
    fun `an interrupted turn says the user stopped it`() {
        val feed = listOf(prompt("u1", 0), tool("t1", "turn-1", 1_000), answer("a1", "turn-1", 3_000))
        val interrupted = TurnInfo("turn-1", "interrupted", 0, 3_000)
        assertEquals("You stopped after 3.0s", turnFolds(feed, interrupted).values.single().label)
    }

    @Test
    fun `a turn with no usable clock says only that it worked`() {
        val feed = listOf(tool("t1", "turn-1"), answer("a1", "turn-1"))
        assertEquals("Worked", turnFolds(feed, settled("turn-1")).values.single().label)
    }

    @Test
    fun `the working row lands at the live edge, below everything else`() {
        val feed = listOf(prompt("u1", 0), tool("t1", "turn-1", 1_000), answer("a1", "turn-1", 2_000))
        val rows =
            presentFeed(feed, emptySet(), settled("turn-1"), emptySet(), activeWorkStartedAtMillis = 3_000)
        assertEquals("working-indicator", rows.last().key)
        assertEquals(3_000L, (rows.last() as FeedRow.Working).startedAtMillis)
    }

    @Test
    fun `a turn that has produced nothing yet is still one visible row`() {
        // The case the row exists for: an empty transcript with a provider thinking.
        // Without it the screen is indistinguishable from an idle thread.
        val rows = presentFeed(emptyList(), emptySet(), null, emptySet(), activeWorkStartedAtMillis = 500)
        assertEquals(listOf("working-indicator"), keys(rows))
    }

    @Test
    fun `no working row when nothing is running`() {
        val feed = listOf(prompt("u1"), answer("a1", "turn-1"))
        assertTrue(presentFeed(feed, emptySet()).none { it is FeedRow.Working })
    }

    @Test
    fun `a half-recorded turn clock is ignored in favour of the feed`() {
        val feed =
            listOf(prompt("u1", 0), tool("t1", "turn-1", 1_000), answer("a1", "turn-1", 20_000))
        // Completed, but with no recorded start. Taking the completion from the record
        // and the start from the feed would produce a duration belonging to neither,
        // so the record is dropped whole.
        val half = TurnInfo("turn-1", "completed", null, 19_000)
        assertEquals("Worked for 20s", turnFolds(feed, half).values.single().label)
    }

    @Test
    fun `durations format the way the other clients format them`() {
        assertEquals("1ms", formatDuration(0))
        assertEquals("1ms", formatDuration(1))
        assertEquals("940ms", formatDuration(940))
        assertEquals("1.5s", formatDuration(1_500))
        assertEquals("42s", formatDuration(42_400))
        assertEquals("2m", formatDuration(120_000))
        assertEquals("2m 5s", formatDuration(125_000))
        // 59.6s of the minute rounds to 60, which reads as the next whole minute
        // rather than "1m 60s".
        assertEquals("2m", formatDuration(119_600))
    }
}
