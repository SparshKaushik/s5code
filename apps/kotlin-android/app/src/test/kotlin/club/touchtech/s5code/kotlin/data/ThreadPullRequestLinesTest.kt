package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.ThreadPullRequestLinkDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadPullRequestSnapshotDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadPullRequestStackDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadPullRequestStackLayerDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The thread linked-PR list: chain depths, sibling layers, cycles and open-first ordering. */
class ThreadPullRequestLinesTest {
    private fun link(
        number: Int,
        head: String,
        base: String,
        state: String = "open",
        updatedAt: String = "2026-10-0${number % 9 + 1}T10:00:00Z",
        source: String = "manual",
        stack: ThreadPullRequestStackDto? = null,
    ) =
        ThreadPullRequestLinkDto(
            host = "github.com",
            repository = "acme/web",
            number = number,
            url = "https://github.com/acme/web/pull/$number",
            source = source,
            linkedAt = "2026-10-01T00:00:00Z",
            snapshot = ThreadPullRequestSnapshotDto(state = state, title = "PR $number", headBranch = head, baseBranch = base, updatedAt = updatedAt),
            stack = stack,
        )

    private fun List<PullRequestListLine>.shape() = map { it.link.number to it.depth }

    @Test
    fun `chains by base branch, with siblings sharing a depth`() {
        val lines =
            linkedPullRequestLines(
                listOf(link(1, "a", "main"), link(2, "b", "a"), link(3, "c", "a"), link(4, "d", "b"))
            )
        assertEquals(listOf(1 to 0, 2 to 1, 4 to 2, 3 to 1), lines.shape())
        assertEquals("derived" to 4, lines.first().stack)
        assertNull(lines[1].stack)
        assertEquals(setOf("github.com/acme/web#1"), lines.map { it.chainKey }.toSet())
    }

    @Test
    fun `keeps a cycle visible as separate links`() {
        val lines = linkedPullRequestLines(listOf(link(1, "x", "y"), link(2, "y", "x")))
        assertEquals(setOf(1 to 0, 2 to 0), lines.shape().toSet())
        assertEquals(listOf(null, null), lines.map { it.stack })
    }

    @Test
    fun `lists open chains before finished ones, most recently updated first`() {
        val lines =
            linkedPullRequestLines(
                listOf(
                    link(1, "old", "main", updatedAt = "2026-10-01T00:00:00Z"),
                    link(2, "done", "main", state = "merged", updatedAt = "2026-10-09T00:00:00Z"),
                    link(3, "new", "main", updatedAt = "2026-10-05T00:00:00Z"),
                    link(4, "gone", "main", source = "stack-dismissed"),
                )
            )
        assertEquals(listOf(3, 1, 2), lines.map { it.link.number })
    }

    @Test
    fun `orders a host stack by its layers, ignoring branch names`() {
        val stack =
            ThreadPullRequestStackDto(
                id = "s",
                base = "main",
                layers = listOf(2, 1).map { ThreadPullRequestStackLayerDto(number = it) },
            )
        val lines = linkedPullRequestLines(listOf(link(1, "p", "q", stack = stack), link(2, "q", "p", stack = stack)))
        assertEquals(listOf(2 to 0, 1 to 1), lines.shape())
        assertEquals("native" to 2, lines.first().stack)
    }
}
