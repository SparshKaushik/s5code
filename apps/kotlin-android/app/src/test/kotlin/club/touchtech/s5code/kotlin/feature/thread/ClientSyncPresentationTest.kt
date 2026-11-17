package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.model.*
import org.junit.Assert.*
import org.junit.Test

class ClientSyncPresentationTest {
    @Test fun `attachment-only dispatch asks the agent to respond`() {
        assertTrue(composerMessageText("", true).contains("Respond using the conversation context"))
        assertEquals(composerMessageText("", true), composerMessageText("![Image](t3-context://v1/image/image_1)", true))
        assertEquals("Explain this", composerMessageText("Explain this", true))
        assertEquals("", composerMessageText("", false))
    }

    @Test fun `prefilled editor answers keep whitespace and may be cleared`() {
        val question = UserInputQuestion("q", "Edit", "Edit text", UserInputKind.Text, emptyList(), initialAnswer = "old")
        val request = PendingUserInput("r", listOf(question))
        assertEquals(UserInputAnswer.Text("  new\n"), buildUserInputAnswers(request, mapOf("q" to "  new\n"), emptyMap())?.get("q"))
        assertEquals(UserInputAnswer.Text(""), buildUserInputAnswers(request, mapOf("q" to ""), emptyMap())?.get("q"))
        assertNull(buildUserInputAnswers(request, emptyMap(), emptyMap()))
    }

    @Test fun `running children stay visible after their parent turn completes`() {
        val child = FeedEntry.Subagent("child", "Review", "Check code", true, "turn")
        val tool = FeedEntry.ToolCall("tool", "Run", "Check", "done", ToolState.Succeeded, "turn")
        val answer = FeedEntry.AgentMessage("answer", "Done", "now", false, turnId = "turn")
        val rows = presentFeed(listOf(tool, child, answer), emptySet())
        assertTrue(rows.any { it is FeedRow.Entry && it.entry == child })
        assertTrue(rows.any { it is FeedRow.TurnFold })
    }

    @Test fun `live thought uses the provider heading or first sentence`() {
        assertEquals("Check the build", liveThoughtLine("**Check the build**\nMore detail."))
        assertEquals("Check the build.", liveThoughtLine("Check the build. Then inspect logs."))
        assertEquals("Read source", liveThoughtLine("[Read source](https://example.test)"))
        val rows = presentFeed(listOf(FeedEntry.Reasoning("r", "**Check the build**\nDetails", thought = true)), emptySet(), activeWorkStartedAtMillis = 1)
        assertEquals("Check the build", rows.filterIsInstance<FeedRow.WorkToggle>().single().thought)
    }
}
