package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.model.FeedEntry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ThreadMessageJumpTest {
    private val answer =
        FeedEntry.AgentMessage(id = "item-2", markdown = "found it", timeLabel = "", turnId = "turn-1", messageId = "msg-2")
    private val other = FeedEntry.AgentMessage(id = "item-1", markdown = "hi", timeLabel = "", turnId = "turn-0")

    private fun step(
        feed: List<FeedEntry>,
        rows: List<FeedRow>,
        expanded: Set<String> = emptySet(),
        hasMore: Boolean = false,
        loading: Boolean = false,
    ) = messageJumpStep("msg-2", feed, rows, expanded, hasMore, loading)

    @Test
    fun `a shown message scrolls to its row by message id, not item id`() {
        assertEquals(
            MessageJumpStep.Scroll(1),
            step(listOf(other, answer), listOf(FeedRow.Entry(other), FeedRow.Entry(answer))),
        )
    }

    @Test
    fun `a folded message unfolds its turn first`() {
        assertEquals(MessageJumpStep.Unfold("turn-1"), step(listOf(other, answer), listOf(FeedRow.Entry(other))))
        // Already unfolded and still not shown: nothing more to do.
        assertEquals(MessageJumpStep.GiveUp, step(listOf(other, answer), listOf(FeedRow.Entry(other)), setOf("turn-1")))
    }

    @Test
    fun `an unloaded message pages earlier history until it runs out`() {
        val rows = listOf<FeedRow>(FeedRow.Entry(other))
        assertEquals(MessageJumpStep.LoadEarlier, step(listOf(other), rows, hasMore = true))
        assertEquals(MessageJumpStep.Wait, step(listOf(other), rows, hasMore = true, loading = true))
        assertEquals(MessageJumpStep.GiveUp, step(listOf(other), rows))
    }

    @Test
    fun `a request applies only to its thread and lapses`() {
        val pending = ThreadMessageJumps.Pending("env:thr", "msg-2", atMillis = 1_000)
        assertEquals("msg-2", ThreadMessageJumps.messageFor(pending, "env:thr", nowMillis = 2_000))
        assertNull(ThreadMessageJumps.messageFor(pending, "env:other", nowMillis = 2_000))
        assertNull(ThreadMessageJumps.messageFor(pending, "env:thr", nowMillis = 1_000 + 30_000))
    }
}
