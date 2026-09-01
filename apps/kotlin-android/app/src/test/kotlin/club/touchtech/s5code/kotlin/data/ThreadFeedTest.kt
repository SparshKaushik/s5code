package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.FeedEntry
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ToolState
import club.touchtech.s5code.kotlin.transport.wire.MessageDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadActivityDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadDto
import java.time.Instant
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Transcript assembly.
 *
 * The bug these guard against: a tool's `tool.updated` and `tool.completed` rows
 * both rendered, so the same call appeared twice — once running near the top, once
 * completed below the last assistant message. The RN feed collapses a tool's
 * lifecycle into one row before interleaving with messages.
 */
class ThreadFeedTest {

    private val start = Instant.parse("2026-03-01T12:00:00Z")

    private fun at(seconds: Long): String = start.plusSeconds(seconds).toString()

    private fun activity(
        id: String,
        kind: String,
        summary: String,
        sequence: Long,
        seconds: Long,
        tone: String = "tool",
        payload: JsonObject? = null,
    ) =
        ThreadActivityDto(
            id = id,
            tone = tone,
            kind = kind,
            summary = summary,
            payload = payload,
            sequence = sequence,
            createdAt = at(seconds),
        )

    private fun feedOf(
        messages: List<MessageDto> = emptyList(),
        activities: List<ThreadActivityDto> = emptyList(),
    ): List<FeedEntry> =
        threadDetailFrom(
                EnvironmentId("env-1"),
                ThreadDto(id = "t-1", messages = messages, activities = activities),
                { instanceId -> ProviderInstance(instanceId, instanceId) },
                start.toEpochMilli(),
            )
            .feed

    @Test
    fun `a tool's lifecycle collapses into one row`() {
        val payload = buildJsonObject {
            put("itemType", "command")
            put("detail", "ls -la")
        }
        val feed =
            feedOf(
                activities =
                    listOf(
                        activity("a-1", "tool.started", "Run command", 1, 0, payload = payload),
                        activity("a-2", "tool.updated", "Run command", 2, 1, payload = payload),
                        activity(
                            "a-3",
                            "tool.completed",
                            "Run command complete",
                            3,
                            2,
                            payload = payload,
                        ),
                    )
            )
        assertEquals(1, feed.size)
        val tool = feed.single() as FeedEntry.ToolCall
        assertEquals(ToolState.Succeeded, tool.state)
    }

    @Test
    fun `a tool that completes after a message does not render a second row`() {
        val payload = buildJsonObject {
            put("itemType", "command")
            put("detail", "bun test")
        }
        val feed =
            feedOf(
                messages =
                    listOf(
                        MessageDto(id = "m-1", role = "assistant", text = "Running the tests.", createdAt = at(1))
                    ),
                activities =
                    listOf(
                        activity("a-1", "tool.updated", "Run command", 1, 0, payload = payload),
                        activity("a-2", "tool.completed", "Run command", 2, 2, payload = payload),
                    ),
            )
        assertEquals(2, feed.size)
        assertEquals(1, feed.count { it is FeedEntry.ToolCall })
        // The collapsed row keeps its original position, so it stays above the
        // message it preceded rather than jumping below it.
        assertTrue(feed[0] is FeedEntry.ToolCall)
        assertTrue(feed[1] is FeedEntry.AgentMessage)
    }

    @Test
    fun `two separate calls to the same tool stay two rows`() {
        val payload = buildJsonObject {
            put("itemType", "command")
            put("detail", "git status")
        }
        val feed =
            feedOf(
                activities =
                    listOf(
                        activity("a-1", "tool.updated", "Run command", 1, 0, payload = payload),
                        activity("a-2", "tool.completed", "Run command", 2, 1, payload = payload),
                        activity("a-3", "tool.updated", "Run command", 3, 5, payload = payload),
                        activity("a-4", "tool.completed", "Run command", 4, 6, payload = payload),
                    )
            )
        assertEquals(2, feed.count { it is FeedEntry.ToolCall })
    }

    @Test
    fun `different tools do not collapse into each other`() {
        val feed =
            feedOf(
                activities =
                    listOf(
                        activity(
                            "a-1",
                            "tool.completed",
                            "Read file",
                            1,
                            0,
                            payload = buildJsonObject { put("detail", "src/main.kt") },
                        ),
                        activity(
                            "a-2",
                            "tool.completed",
                            "Run command",
                            2,
                            1,
                            payload = buildJsonObject { put("detail", "bun test") },
                        ),
                    )
            )
        assertEquals(2, feed.size)
    }

    @Test
    fun `a failed tool keeps its failure through the collapse`() {
        val payload = buildJsonObject {
            put("detail", "bun test")
            put("status", "failed")
        }
        val feed =
            feedOf(
                activities =
                    listOf(
                        activity(
                            "a-1",
                            "tool.updated",
                            "Run command",
                            1,
                            0,
                            payload = buildJsonObject { put("detail", "bun test") },
                        ),
                        activity(
                            "a-2",
                            "tool.completed",
                            "Run command",
                            2,
                            1,
                            tone = "error",
                            payload = payload,
                        ),
                    )
            )
        assertEquals(ToolState.Failed, (feed.single() as FeedEntry.ToolCall).state)
    }

    @Test
    fun `checkpoint captures and context bookkeeping stay out of the transcript`() {
        val feed =
            feedOf(
                activities =
                    listOf(
                        activity("a-1", "checkpoint.captured", "Checkpoint captured", 1, 0, tone = "info"),
                        activity("a-2", "context-window.updated", "Context", 2, 1, tone = "info"),
                    )
            )
        assertTrue(feed.isEmpty())
    }

    @Test
    fun `an agent's background work stays hidden but its finish does not`() {
        val feed =
            feedOf(
                activities =
                    listOf(
                        activity(
                            "a-1",
                            "task.progress",
                            "Searching",
                            1,
                            0,
                            payload =
                                buildJsonObject {
                                    put("agentId", "agent-1")
                                    put("agentKind", "background")
                                    put("taskId", "task-1")
                                },
                        ),
                        activity(
                            "a-2",
                            "task.completed",
                            "Reviewed the diff",
                            2,
                            1,
                            payload =
                                buildJsonObject {
                                    put("agentId", "agent-2")
                                    put("agentKind", "agent")
                                    put("taskId", "task-2")
                                    put("title", "Reviewer")
                                },
                        ),
                    )
            )
        assertEquals(1, feed.size)
        assertEquals("Reviewer", (feed.single() as FeedEntry.Subagent).name)
    }

    @Test
    fun `a subagent's rows collapse by identity rather than adjacency`() {
        fun taskPayload(status: String) = buildJsonObject {
            put("taskId", "task-1")
            put("title", "Reviewer")
            put("status", status)
        }
        val feed =
            feedOf(
                messages =
                    listOf(MessageDto(id = "m-1", role = "assistant", text = "Working.", createdAt = at(1))),
                activities =
                    listOf(
                        activity("a-1", "task.progress", "Reviewing", 1, 0, payload = taskPayload("running")),
                        activity("a-2", "task.completed", "Reviewed", 2, 2, payload = taskPayload("completed")),
                    ),
            )
        assertEquals(1, feed.count { it is FeedEntry.Subagent })
        assertEquals(false, (feed.first { it is FeedEntry.Subagent } as FeedEntry.Subagent).active)
    }

    @Test
    fun `an empty assistant message never renders as an orphaned timestamp`() {
        val feed =
            feedOf(
                messages =
                    listOf(
                        MessageDto(id = "m-1", role = "assistant", text = "   ", createdAt = at(0)),
                        MessageDto(id = "m-2", role = "assistant", text = "Done.", createdAt = at(1)),
                    )
            )
        assertEquals(1, feed.size)
    }

    @Test
    fun `the plan-mode boundary tool row is left to the plan card`() {
        val feed =
            feedOf(
                activities =
                    listOf(
                        activity(
                            "a-1",
                            "tool.completed",
                            "ExitPlanMode",
                            1,
                            0,
                            payload =
                                buildJsonObject { put("detail", "ExitPlanMode: here is the plan") },
                        )
                    )
            )
        assertTrue(feed.isEmpty())
    }
}
