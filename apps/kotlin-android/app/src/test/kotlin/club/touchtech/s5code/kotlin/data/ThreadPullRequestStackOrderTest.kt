package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.ThreadPullRequestLinkDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadPullRequestSnapshotDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadPullRequestStackDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadPullRequestStackLayerDto
import org.junit.Assert.assertEquals
import org.junit.Test

/** Port of the shared `threadPullRequests` tests for native stacks whose saved layer lists disagree. */
class ThreadPullRequestStackOrderTest {
    private fun stack(id: String, number: Int, layers: List<Int>) =
        ThreadPullRequestStackDto(
            id = id,
            number = number,
            url = "https://github.com/pingdotgg/t3code/stacks/$number",
            base = "main",
            layers = layers.map { ThreadPullRequestStackLayerDto(it, "pr-$it", "open") },
        )

    private fun link(
        number: Int,
        stack: ThreadPullRequestStackDto,
        linkedAt: String = "2026-01-01T00:00:${number.toString().padStart(2, '0')}.000Z",
        snapshot: ThreadPullRequestSnapshotDto? = null,
    ) =
        ThreadPullRequestLinkDto(
            host = "github.com",
            repository = "pingdotgg/t3code",
            number = number,
            url = "https://github.com/pingdotgg/t3code/pull/$number",
            source = "manual",
            linkedAt = linkedAt,
            snapshot = snapshot,
            stack = stack,
        )

    @Test
    fun `uses a later complete native snapshot for layer order and the current pull request`() {
        val links =
            listOf(18, 20, 83, 84).map { number ->
                link(
                    number,
                    stack(
                        "80",
                        80,
                        if (number < 83) listOf(79, 37, 38, 39, 76, 18, 20) else listOf(79, 37, 38, 39, 76, 18, 20, 83, 84),
                    ),
                    linkedAt = "2026-01-01T00:00:00.000Z",
                    snapshot =
                        ThreadPullRequestSnapshotDto(
                            title = "Change",
                            headBranch = "feature",
                            baseBranch = "main",
                            updatedAt = "2026-01-01T00:00:00.000Z",
                            syncedAt = "2026-01-01T00:00:00.000Z",
                        ),
                )
            }
        assertEquals(listOf(listOf(18, 20, 83, 84)), resolveThreadPullRequestChains(links).map { chain -> chain.layers.map { it.number } })
        val current = resolveThreadCurrentPullRequest(links) as ThreadCurrentPullRequest.Stack
        assertEquals(84, current.top.number)
        assertEquals(listOf(84, 83, 20, 18), current.open.map { it.number })
    }

    @Test
    fun `uses the earliest longest partial snapshot and appends missing members in stored order`() {
        val links =
            listOf(listOf(3, 2), listOf(9, 2, 3), listOf(9, 3, 2), listOf(9, 3, 2)).mapIndexed { index, numbers ->
                link(index + 1, stack("s1", 1, numbers))
            }
        assertEquals(listOf(listOf(2, 3, 1, 4)), resolveThreadPullRequestChains(links).map { chain -> chain.layers.map { it.number } })
    }
}
