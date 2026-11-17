package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.FeedEntry
import club.touchtech.s5code.kotlin.transport.TransportJson
import club.touchtech.s5code.kotlin.transport.wire.V2ProjectionDto
import club.touchtech.s5code.kotlin.transport.wire.V2VisibleItemDto
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class V2FeedRowsTest {
    private fun obj(text: String) = TransportJson.parseToJsonElement(text).jsonObject
    private val metadata = obj("""{"id":"thread","projectId":"project","title":"Feed"}""")
    private fun item(id: String, type: String, run: String = "run", vararg extra: Pair<String, String>) = JsonObject(obj(
        """{"id":"$id","threadId":"thread","runId":"$run","nodeId":"root","type":"$type","text":"$id","status":"completed",
            "messageId":"message-$id","attachments":[],"startedAt":"2026-10-01T00:00:00Z","updatedAt":"2026-10-01T00:00:00Z"}"""
    ) + extra.map { (key, value) -> key to JsonPrimitive(value) })
    private fun feed(runStatus: String, vararg items: JsonObject) = v2Presentation(V2ProjectionDto(
        metadata,
        runs = listOf(obj("""{"id":"run","ordinal":1,"status":"$runStatus"}""")),
        visibleTurnItems = items.mapIndexed { index, it -> V2VisibleItemDto(index, "local", "thread", it["id"].toString().trim('"'), it) },
    )).feed

    @Test
    fun `a pending queued turn stays in the queue, not the feed`() {
        val pending = item("pending", "user_message", extra = arrayOf("inputIntent" to "queued_turn", "status" to "pending"))
        val started = item("started", "user_message", extra = arrayOf("inputIntent" to "queued_turn"))
        assertEquals(listOf("started"), feed("running", pending, started).map { it.id })
    }

    @Test
    fun `a stopped run reads as stopped and its request stays out`() {
        val rows = feed("interrupted",
            item("prompt", "user_message"),
            item("request", "run_interrupt_request", extra = arrayOf("message" to "Stopping")),
            item("result", "run_interrupt_result", extra = arrayOf("message" to "Stopped")))
        assertEquals(listOf("prompt", "result"), rows.map { it.id })
        val note = rows.last() as FeedEntry.Note
        assertEquals("Stopped", note.message)
        assertTrue(note.stopped)
    }

    @Test
    fun `a stopped compact keeps only the compaction row once the provider reported one`() {
        val prompt = item("prompt", "user_message", extra = arrayOf("text" to "/compact"))
        val result = item("result", "run_interrupt_result", extra = arrayOf("message" to "Stopped"))
        val compaction = item("compaction", "compaction", extra = arrayOf("status" to "cancelled"))
        assertEquals(listOf("compaction"), feed("interrupted", prompt, compaction, result).map { it.id })
        // Stopped before the provider started: nothing else says it stopped.
        assertEquals(listOf("prompt", "result"), feed("interrupted", prompt, result).map { it.id })
    }

    @Test
    fun `compaction on return says it is compacting, then that it compacted before sending`() {
        assertEquals("Compacting…", idleCompactingLabel("onReturn"))
        assertEquals("Context compacted before sending", idleCompactionTitle("onReturn"))
        val question = "This session is 2h 5m old and uses 412,000 tokens. Compact it before continuing?"
        val ask = JsonObject(item("ask", "user_input_request") + mapOf("requestId" to JsonPrimitive("request"),
            "questions" to TransportJson.parseToJsonElement("""[{"id":"q","question":"$question"}]""")))
        val request = obj("""{"id":"request","kind":"user_input","status":"resolved","answers":{"q":["Compact and continue"]}}""")
        val labels = compactionLabelsByItem(listOf(ask, item("compaction", "compaction")), mapOf<String?, JsonObject>("request" to request))
        assertEquals(CompactionLabels("Context compacted after 2h 5m idle", "Compacting…"), labels["compaction"])
    }
}
