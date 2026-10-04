package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.data.v2Presentation
import club.touchtech.s5code.kotlin.model.FeedEntry
import club.touchtech.s5code.kotlin.transport.wire.*
import kotlinx.serialization.json.*
import org.junit.Assert.*
import org.junit.Test

class OrchestrationV2Test {
    private fun obj(text: String) = TransportJson.parseToJsonElement(text).jsonObject
    private val metadata = obj("""{"id":"thread","projectId":"project","title":"V2","providerInstanceId":"codex","modelSelection":{"instanceId":"codex","model":"gpt"}}""")
    private fun item(id: String, ordinal: Int, type: String = "assistant_message", run: String = "run") = obj(
        """{"id":"$id","threadId":"thread","runId":"$run","nodeId":"root","ordinal":$ordinal,"type":"$type","text":"$id","streaming":false,"status":"completed","inputIntent":"turn_start","messageId":"message-$id","attachments":[],"startedAt":"2026-10-01T00:00:00Z","updatedAt":"2026-10-01T00:00:00Z"}"""
    )
    private fun row(item: JsonObject, position: Int = 0) = V2VisibleItemDto(position, "local", "thread", item.v2String("id")!!, item)
    private fun event(type: String, payload: JsonObject) = buildJsonObject {
        put("type", type); put("threadId", "thread"); put("payload", payload)
    }

    @Test fun `socket advertises V2 and shell statuses retain active run over queued latest`() {
        assertTrue(EnvironmentHttp.socketUrl("ws://host", "ticket").contains("orchestrationProtocol=2"))
        val shell = TransportJson.decodeFromJsonElement(ThreadShellDto.serializer(), obj("""{
            "id":"thread","status":"queued","activeRunId":"active","latestRunId":"queued",
            "activityRunStatus":"running","activityRunStartedAt":"2026-10-01T00:00:00Z"
        }""")).normalizedV2()
        assertEquals("active", shell.session?.activeTurnId)
        assertEquals("running", shell.session?.status)
    }

    @Test fun `stream snapshot decodes flat V2 shape and preserves inherited ordering`() {
        val first = item("first", 7)
        val second = item("second", 1)
        val projection = V2ProjectionDto(metadata, visibleTurnItems = listOf(
            row(first).copy(visibility = "inherited", sourceThreadId = "parent"), row(second, 1)))
        val wire = buildJsonObject {
            put("kind", "snapshot"); put("snapshotSequence", 42)
            put("projection", TransportJson.encodeToJsonElement(V2ProjectionDto.serializer(), projection))
        }
        val decoded = TransportJson.decodeFromJsonElement(V2ThreadStreamDto.serializer(), wire)
        assertEquals(42L, decoded.snapshotSequence)
        assertEquals(listOf("first", "second"), v2Presentation(decoded.projection!!).feed.map { it.id })
    }

    @Test fun `stream updates replace items and rollbacks remove local history only`() {
        val first = item("first", 1)
        val projection = V2ProjectionDto(metadata, turnItems = listOf(first), visibleTurnItems = listOf(row(first),
            row(item("inherited", 2), 1).copy(visibility = "inherited", sourceThreadId = "parent")))
        val updatedItem = JsonObject(first + ("text" to JsonPrimitive("streamed")))
        val streamed = applyV2Event(projection, event("turn-item.updated", updatedItem), false, null)
        assertEquals(2, streamed.visibleTurnItems.size)
        assertEquals("streamed", streamed.visibleTurnItems.first().item.v2String("text"))
        val reverted = applyV2Event(streamed, event("run.updated", obj("""{"id":"run","status":"rolled_back"}""")), false, null)
        assertEquals(listOf("inherited"), reverted.visibleTurnItems.map { it.sourceItemId })
    }

    @Test fun `bounded replay does not resurrect missing old rows and history cannot overwrite streaming`() {
        val live = item("live", 10)
        val projection = V2ProjectionDto(metadata, turnItems = listOf(live), visibleTurnItems = listOf(row(live)))
        assertSame(projection, applyV2Event(projection, event("turn-item.updated", item("old", 2)), true, 10))
        val page = V2HistoryPageDto(items = listOf(row(item("old", 2)), row(JsonObject(live + ("text" to JsonPrimitive("stale"))), 1)))
        val merged = mergeV2History(projection, page)
        assertEquals(listOf("old", "live"), merged.visibleTurnItems.map { it.sourceItemId })
        assertEquals("live", merged.visibleTurnItems.last().item.v2String("text"))
    }

    @Test fun `queued messages stay in queue and cancelled queued input stays out of feed`() {
        val queued = JsonObject(item("queued", 1, "user_message") + ("inputIntent" to JsonPrimitive("queued_turn")))
        val projection = V2ProjectionDto(metadata,
            runs = listOf(obj("""{"id":"run","status":"queued","userMessageId":"message","queuePosition":1,"queueHeld":true}""")),
            messages = listOf(obj("""{"id":"message","text":"follow up"}""")), turnItems = listOf(queued), visibleTurnItems = listOf(row(queued)))
        val presented = v2Presentation(projection)
        assertTrue(presented.feed.none { it is FeedEntry.UserMessage })
        assertEquals("follow up", presented.queuedRuns.single().text)
        assertTrue(presented.queuedRuns.single().held)
        val cancelled = applyV2Event(projection, event("run.updated", obj("""{"id":"run","status":"cancelled"}""")), false, null)
        assertTrue(cancelled.visibleTurnItems.isEmpty())
    }

    @Test fun `runtime request resolution removes approval without replaying an old gate`() {
        val request = obj("""{"id":"request","kind":"command","status":"pending","responseCapability":{"type":"live"}}""")
        val approval = JsonObject(item("approval", 1, "approval_request") + mapOf(
            "requestId" to JsonPrimitive("request"), "requestKind" to JsonPrimitive("command"), "prompt" to JsonPrimitive("Run shell?")))
        val projection = V2ProjectionDto(metadata, runtimeRequests = listOf(request), turnItems = listOf(approval), visibleTurnItems = listOf(row(approval)))
        assertEquals("request", v2Presentation(projection).approval?.id)
        val resolved = applyV2Event(projection, event("runtime-request.updated", JsonObject(request + ("status" to JsonPrimitive("resolved")))), false, null)
        assertNull(v2Presentation(resolved).approval)
        assertEquals(1, resolved.visibleTurnItems.size)
    }
}
