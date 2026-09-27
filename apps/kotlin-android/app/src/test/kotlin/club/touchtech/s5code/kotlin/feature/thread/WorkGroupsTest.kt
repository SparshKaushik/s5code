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
 * "Used 7 tools" is exactly the stall the subagent row exists to explain, and a
 * folded error is an error the user does not know about. Every run — even a
 * single call — collapses to its summary row, matching RN's `work-toggle`.
 */
class WorkGroupsTest {

    private fun tool(id: String, state: ToolState = ToolState.Succeeded) =
        FeedEntry.ToolCall(id = id, name = "Bash", summary = "ls", detail = "", state = state)

    private fun command(id: String) =
        FeedEntry.ToolCall(
            id = id,
            name = "Bash",
            summary = "ls",
            detail = "",
            state = ToolState.Succeeded,
            command = "ls -la",
        )

    private fun message(id: String) = FeedEntry.AgentMessage(id, "text", "12:00")

    private fun subagent(id: String) =
        FeedEntry.Subagent(id = id, name = "explorer", task = "read files", active = true)

    private fun keys(rows: List<FeedRow>) = rows.map { it.key }

    @Test
    fun `a single call is its own summary row`() {
        val feed = listOf(message("m1"), command("t1"), message("m2"))
        assertEquals(listOf("m1", "work-toggle:work-group:t1", "m2"), keys(presentFeed(feed, emptySet())))
        val toggle =
            presentFeed(feed, emptySet()).filterIsInstance<FeedRow.WorkToggle>().single()
        // `singleToolCallLabel`: the command itself, as RN shows it.
        assertEquals("ls -la", toggle.summary)
    }

    @Test
    fun `a long run folds behind one summary toggle`() {
        val feed =
            listOf(message("m1"), command("t1"), command("t2"), command("t3"), message("m2"))
        val rows = presentFeed(feed, emptySet())
        assertEquals(listOf("m1", "work-toggle:work-group:t1", "m2"), keys(rows))
        val toggle = rows.filterIsInstance<FeedRow.WorkToggle>().single()
        assertEquals(3, toggle.hiddenCount)
        assertEquals("Ran 3 commands", toggle.summary)
    }

    @Test
    fun `expanding a group shows the summary then every row`() {
        val feed = listOf(command("t1"), command("t2"), command("t3"))
        val rows = presentFeed(feed, setOf("work-group:t1"))
        assertEquals(listOf("work-toggle:work-group:t1", "t1", "t2", "t3"), keys(rows))
        assertTrue(rows.filterIsInstance<FeedRow.WorkToggle>().single().expanded)
    }

    @Test
    fun `mixed runs summarize each action the way RN does`() {
        val feed =
            listOf(
                command("t1"),
                command("t2"),
                FeedEntry.ToolCall(
                    id = "r1",
                    name = "Read",
                    summary = "",
                    detail = "src/Main.kt",
                    state = ToolState.Succeeded,
                    requestKind = "file-read",
                ),
            )
        val toggle =
            presentFeed(feed, emptySet()).filterIsInstance<FeedRow.WorkToggle>().single()
        assertEquals("Ran 2 commands and read 1 file", toggle.summary)
    }

    @Test
    fun `messages break a run so two runs never merge`() {
        val feed = listOf(command("t1"), message("m1"), command("t2"))
        assertEquals(
            listOf("work-toggle:work-group:t1", "m1", "work-toggle:work-group:t2"),
            keys(presentFeed(feed, emptySet())),
        )
    }

    @Test
    fun `a plan card is never swallowed into a fold`() {
        val feed =
            listOf(command("t1"), command("t2"), FeedEntry.PlanUpdate("p1", emptyList()), command("t3"))
        val rows = keys(presentFeed(feed, emptySet()))
        assertTrue(rows.contains("p1"))
        assertEquals(
            listOf("work-toggle:work-group:t1", "p1", "work-toggle:work-group:t3"),
            rows,
        )
    }

    @Test
    fun `an error row is never folded away`() {
        val feed =
            listOf(command("t1"), FeedEntry.ErrorEntry("e1", "boom"), command("t2"), command("t3"))
        val rows = keys(presentFeed(feed, emptySet()))
        assertTrue(rows.contains("e1"))
    }

    @Test
    fun `a failed tool call stays visible rather than folding into the run`() {
        // RN's groupable run only takes non-error rows; a failed call is the thing
        // the user needs to see, not something to hide inside "Ran 2 commands".
        val feed =
            listOf(
                command("t1"),
                tool("t2", ToolState.Failed),
                command("t3"),
            )
        assertEquals(
            listOf("work-toggle:work-group:t1", "t2", "work-toggle:work-group:t3"),
            keys(presentFeed(feed, emptySet())),
        )
    }

    @Test
    fun `a subagent breaks the run and stays visible in place`() {
        val feed = listOf(command("t1"), subagent("s1"), command("t2"), command("t3"))
        val rows = keys(presentFeed(feed, emptySet()))
        assertEquals(
            listOf("work-toggle:work-group:t1", "s1", "work-toggle:work-group:t2"),
            rows,
        )
    }

    @Test
    fun `subagents alone never fold however many there are`() {
        val feed = listOf(subagent("s1"), subagent("s2"), subagent("s3"))
        assertEquals(listOf("s1", "s2", "s3"), keys(presentFeed(feed, emptySet())))
    }

    @Test
    fun `the group key follows the first row so a growing run stays open`() {
        val open = setOf("work-group:t1")
        val before = presentFeed(listOf(command("t1"), command("t2"), command("t3")), open)
        val after =
            presentFeed(listOf(command("t1"), command("t2"), command("t3"), command("t4")), open)
        assertTrue(before.filterIsInstance<FeedRow.WorkToggle>().single().expanded)
        assertTrue(after.filterIsInstance<FeedRow.WorkToggle>().single().expanded)
        assertEquals(4, after.filterIsInstance<FeedRow.Entry>().size)
    }

    @Test
    fun `an empty feed presents nothing`() {
        assertTrue(presentFeed(emptyList(), emptySet()).isEmpty())
    }

    @Test
    fun `a running call on the live edge labels the group in the present tense`() {
        val feed = listOf(command("t1"), command("t2").copy(state = ToolState.Running))
        val toggle =
            presentFeed(feed, emptySet(), activeWorkStartedAtMillis = 1_000)
                .filterIsInstance<FeedRow.WorkToggle>()
                .single()
        assertEquals("Running ls", toggle.summary)
    }

    @Test
    fun `a run at the very end of the feed still folds`() {
        val feed = listOf(message("m1"), command("t1"), command("t2"))
        assertEquals(
            listOf("m1", "work-toggle:work-group:t1"),
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
        // The single tool call presents as its summary toggle, still folded.
        assertEquals(
            listOf(
                "u1",
                "turn-fold:turn-1:header",
                "work-toggle:work-group:t1",
                "a1",
                "turn-fold:turn-1:footer",
            ),
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
            listOf(
                "u1",
                "turn-fold:turn-1:header",
                "a1",
                "work-toggle:work-group:t1",
                "turn-fold:turn-1:footer",
            ),
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
        // The work toggle opens its run; the turn trigger — which closes
        // everything — comes last.
        assertEquals(
            listOf(
                "u1",
                "turn-fold:turn-1:header",
                "a1",
                "work-toggle:work-group:t1",
                "t1",
                "t2",
                "t3",
                "turn-fold:turn-1:footer",
            ),
            keys(presentFeed(feed, setOf("work-group:t1"), settled("turn-1"), setOf("turn-1"))),
        )
        // Collapsed, the run is just its summary row between the answer and the
        // turn trigger.
        assertEquals(
            listOf(
                "u1",
                "turn-fold:turn-1:header",
                "a1",
                "work-toggle:work-group:t1",
                "turn-fold:turn-1:footer",
            ),
            keys(presentFeed(feed, emptySet(), settled("turn-1"), setOf("turn-1"))),
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
        assertEquals(
            listOf("u1", "work-toggle:work-group:t1", "a1"),
            keys(presentFeed(feed, emptySet(), running)),
        )
    }

    @Test
    fun `a turn whose answer is still streaming is never folded`() {
        val feed =
            listOf(prompt("u1"), tool("t1", "turn-1"), answer("a1", "turn-1", streaming = true))
        // The turn record says settled, but the message says otherwise; the message
        // wins, because that is what is visibly still changing on screen.
        assertEquals(
            listOf("u1", "work-toggle:work-group:t1", "a1"),
            keys(presentFeed(feed, emptySet(), settled("turn-1"))),
        )
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
            listOf("u1", "turn-fold:turn-1:header", "a1", "u2", "work-toggle:work-group:t2", "a2"),
            keys(presentFeed(feed, emptySet(), running)),
        )
    }

    @Test
    fun `rows with no turn id are never folded`() {
        // Some providers attribute nothing. Those rows must stay: there is no header
        // that would ever bring them back.
        val feed = listOf(prompt("u1"), tool("t1", null), answer("a1", null))
        assertEquals(
            listOf("u1", "work-toggle:work-group:t1", "a1"),
            keys(presentFeed(feed, emptySet(), settled("turn-1"))),
        )
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
        // Expanded turn: the run still folds inside it, behind its summary.
        assertEquals(
            listOf(
                "u1",
                "turn-fold:turn-1:header",
                "work-toggle:work-group:t1",
                "a1",
                "turn-fold:turn-1:footer",
            ),
            keys(presentFeed(feed, emptySet(), settled("turn-1"), setOf("turn-1"))),
        )
    }

    @Test
    fun `the first assistant message stays visible beside the answer`() {
        // RN's `firstAssistantMessageIdByTurn`/`terminalAssistantMessageIdByTurn`:
        // a turn that spoke twice reads as its opening line, the fold, and the
        // answer it ended on.
        val feed =
            listOf(
                prompt("u1"),
                answer("a1", "turn-1"),
                tool("t1", "turn-1"),
                answer("a2", "turn-1"),
            )
        assertEquals(
            listOf("u1", "a1", "turn-fold:turn-1:header", "a2"),
            keys(presentFeed(feed, emptySet(), settled("turn-1"))),
        )
    }

    @Test
    fun `a folded user-input record never hides inside the turn`() {
        val feed =
            listOf(
                prompt("u1"),
                FeedEntry.QuestionAnswer(id = "q1", summary = "User input submitted", preview = "ok", turnId = "turn-1"),
                tool("t1", "turn-1"),
                answer("a1", "turn-1"),
            )
        assertEquals(
            listOf("u1", "q1", "turn-fold:turn-1:header", "a1"),
            keys(presentFeed(feed, emptySet(), settled("turn-1"))),
        )
    }

    @Test
    fun `a turn hiding only thoughts does not collapse`() {
        // Thinking is work, but a question answered by thought alone keeps its
        // "Thought" row — `hidesFoldableWork` says there is nothing to disclose.
        val feed =
            listOf(
                prompt("u1"),
                FeedEntry.Reasoning(id = "r1", text = "thinking", thought = true, turnId = "turn-1"),
                answer("a1", "turn-1"),
            )
        assertEquals(
            listOf("u1", "work-toggle:work-group:r1", "a1"),
            keys(presentFeed(feed, emptySet(), settled("turn-1"))),
        )
    }

    @Test
    fun `a lone compaction row does not fold on its own`() {
        val feed =
            listOf(
                prompt("u1"),
                FeedEntry.Note(id = "c1", message = "Context compacted", turnId = "turn-1", compaction = true),
                answer("a1", "turn-1"),
            )
        assertEquals(
            listOf("u1", "c1", "a1"),
            keys(presentFeed(feed, emptySet(), settled("turn-1"))),
        )
    }

    @Test
    fun `the fold header sits above the first hidden row`() {
        // RN anchors the fold on `firstHiddenEntry.id`, so a visible opening
        // assistant message renders above it.
        val feed =
            listOf(
                prompt("u1"),
                answer("a1", "turn-1"),
                tool("t1", "turn-1"),
                answer("a2", "turn-1"),
            )
        val fold = turnFolds(feed, settled("turn-1")).values.single()
        assertEquals("t1", fold.anchorId)
    }

    @Test
    fun `thoughts count as thoughts in a folded run, not tools`() {
        val feed =
            listOf(
                FeedEntry.Reasoning(id = "r1", text = "a", thought = true, turnId = "turn-1"),
                FeedEntry.Reasoning(
                    id = "r2",
                    text = "b",
                    extraParts = listOf("c"),
                    thought = true,
                    turnId = "turn-1",
                ),
            )
        val toggle = presentFeed(feed, emptySet()).filterIsInstance<FeedRow.WorkToggle>().single()
        assertEquals("Thought (×3)", toggle.summary)
        assertEquals(3, toggle.hiddenCount)
    }

    @Test
    fun `a single thought row labels itself Thought`() {
        val feed =
            listOf(FeedEntry.Reasoning(id = "r1", text = "a", thought = true, turnId = "turn-1"))
        val toggle = presentFeed(feed, emptySet()).filterIsInstance<FeedRow.WorkToggle>().single()
        assertEquals("Thought", toggle.summary)
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
