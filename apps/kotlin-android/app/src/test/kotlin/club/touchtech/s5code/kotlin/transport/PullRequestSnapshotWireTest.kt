package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.data.providerInstanceForId
import club.touchtech.s5code.kotlin.data.threadSummaryFrom
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.transport.wire.ShellStreamItemDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PullRequestSnapshotWireTest {
    @Test
    fun `reconnect snapshots with synced PRs decode for both environments`() {
        for (mergeability in listOf("mergeable", "conflicting", "unknown")) {
            val wire = """{
                "kind":"snapshot",
                "snapshot":{
                    "snapshotSequence":42,
                    "threads":[{
                        "id":"thread-1","projectId":"project-1","status":"idle",
                        "pullRequests":[{
                            "host":"github.com","repository":"owner/repo","number":12,
                            "url":"https://github.com/owner/repo/pull/12","source":"agent",
                            "linkedAt":"2026-10-01T00:00:00Z",
                            "snapshot":{
                                "state":"open","title":"Fix reconnect","headBranch":"fix/reconnect",
                                "baseBranch":"main","isDraft":false,"updatedAt":null,
                                "syncedAt":"2026-10-01T00:00:00Z","mergeability":"$mergeability"
                            },
                            "stack":null,"watch":{"startedAt":"2026-10-01T00:00:00Z"}
                        }]
                    }]
                }
            }"""
            val decoded = TransportJson.decodeFromString(ShellStreamItemDto.serializer(), wire)
            val snapshot = applyShellStreamItem(
                club.touchtech.s5code.kotlin.transport.wire.ShellSnapshotDto(), decoded,
            )
            val thread = snapshot.threads.single()
            assertEquals(mergeability, thread.pullRequests!!.single().snapshot!!.mergeability)
            val summaries = listOf("env-1", "env-2").map { environment ->
                threadSummaryFrom(EnvironmentId(environment), thread, ::providerInstanceForId, 0)
            }
            assertTrue(summaries.all { it.pullRequest?.number == 12 })
            assertEquals(listOf("env-1", "env-2"), summaries.map { it.environmentId.value })
        }
    }
}
