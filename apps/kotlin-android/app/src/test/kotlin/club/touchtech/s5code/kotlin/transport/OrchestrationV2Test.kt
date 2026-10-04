package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.data.v2Presentation
import club.touchtech.s5code.kotlin.data.threadDetailFrom
import club.touchtech.s5code.kotlin.data.providerInstanceForId
import club.touchtech.s5code.kotlin.data.parseInstant
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.ThreadStatus
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
        val projection = V2ProjectionDto(metadata, turnItems = listOf(first), visibleTurnItems = listOf(
            row(item("inherited", 2)).copy(visibility = "inherited", sourceThreadId = "parent"), row(first, 1)))
        val updatedItem = JsonObject(first + ("text" to JsonPrimitive("streamed")))
        val streamed = applyV2Event(projection, event("turn-item.updated", updatedItem), false, null)
        assertEquals(2, streamed.visibleTurnItems.size)
        assertEquals(listOf("inherited", "first"), streamed.visibleTurnItems.map { it.sourceItemId })
        assertEquals("streamed", streamed.visibleTurnItems.last().item.v2String("text"))
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

    @Test fun `detail status follows resumable requests and native runless work`() {
        val pending = obj("""{"id":"request","kind":"command","status":"pending","responseCapability":{"type":"live"}}""")
        val projection = V2ProjectionDto(metadata, runtimeRequests = listOf(pending))
        fun status(value: V2ProjectionDto) = threadDetailFrom(EnvironmentId("env"), value.asThreadDto(),
            ::providerInstanceForId, parseInstant("2026-10-01T00:00:00Z")!!).summary.status
        assertEquals(ThreadStatus.AwaitingApproval, status(projection))
        assertEquals(ThreadStatus.Idle, status(projection.copy(runtimeRequests = listOf(JsonObject(pending +
            ("responseCapability" to obj("""{"type":"not_resumable","reason":"expired"}""")))))))
        val nativeMetadata = JsonObject(metadata + mapOf("creationSource" to JsonPrimitive("provider"),
            "lineage" to obj("""{"relationshipToParent":"subagent","parentThreadId":"parent"}""")))
        val native = V2ProjectionDto(nativeMetadata, nodes = listOf(obj("""{"id":"root","kind":"root_turn","runId":null,"status":"running","startedAt":"2026-10-01T00:00:00Z"}""")))
        assertEquals(ThreadStatus.Working, status(native))
        assertTrue(v2Presentation(native).providerNativeSubagent)
    }

    @Test fun `visited events clear unread completion without touching activity`() {
        val completedAt = "2026-10-01T00:00:00Z"
        val visitedMetadata = JsonObject(metadata + ("lastVisitedAt" to JsonPrimitive("2026-09-30T00:00:00Z")))
        val projection = V2ProjectionDto(visitedMetadata, updatedAt = completedAt,
            runs = listOf(obj("""{"id":"run","ordinal":1,"status":"completed","completedAt":"$completedAt"}""")))
        fun unread(value: V2ProjectionDto) = threadDetailFrom(EnvironmentId("env"), value.asThreadDto(),
            ::providerInstanceForId, parseInstant(completedAt)!!).summary.unread
        assertTrue(unread(projection))
        val visited = applyV2Event(projection, event("thread.visited", JsonObject(visitedMetadata +
            ("lastVisitedAt" to JsonPrimitive(completedAt)))), false, null)
        assertFalse(unread(visited))
        assertEquals(completedAt, visited.updatedAt)
        assertFalse(unread(projection.copy(thread = metadata)))
    }

    @Test fun `retained interrupt requests update outside a bounded window without reviving hidden results`() {
        val request = item("request", 1, "run_interrupt_request")
        val result = item("result", 2, "run_interrupt_result")
        val projection = V2ProjectionDto(metadata, turnItems = listOf(request),
            attempts = listOf(obj("""{"id":"attempt","runId":"run","rootNodeId":"root","status":"superseded"}""")))
        val updated = applyV2Event(projection, event("turn-item.updated", JsonObject(request + ("status" to JsonPrimitive("completed")))), true, 10)
        assertEquals("completed", updated.turnItems.single().v2String("status"))
        assertTrue(updated.visibleTurnItems.isEmpty())
        val page = V2HistoryPageDto(items = listOf(row(request), row(request), row(result)))
        val merged = mergeV2History(updated, page)
        assertEquals(listOf("request", "result"), merged.visibleTurnItems.map { it.sourceItemId })
        assertEquals("completed", merged.visibleTurnItems.first().item.v2String("status"))
    }

    @Test fun `provider replay preserves token usage when a terminal frame omits it`() {
        val turn = obj("""{"id":"native-turn","status":"running","tokenUsage":{"totalTokens":400}}""")
        val projection = V2ProjectionDto(metadata, providerTurns = listOf(turn))
        val updated = applyV2Event(projection, event("provider-turn.updated",
            obj("""{"id":"native-turn","status":"completed","tokenUsage":null}""")), false, null)
        assertEquals(turn["tokenUsage"], updated.providerTurns.single()["tokenUsage"])
    }

    @Test fun `background subagents hold completion while long lived commands do not`() {
        val run = obj("""{"id":"run","ordinal":1,"status":"completed","startedAt":"2026-10-01T00:00:00Z","completedAt":"2026-10-01T00:01:00Z"}""")
        val agent = JsonObject(item("agent", 2, "subagent") + ("status" to JsonPrimitive("running")))
        val command = JsonObject(item("server", 3, "command_execution") + ("status" to JsonPrimitive("running")))
        val projection = V2ProjectionDto(metadata, runs = listOf(run), turnItems = listOf(agent, command))
        fun status(value: V2ProjectionDto) = threadDetailFrom(EnvironmentId("env"), value.asThreadDto(),
            ::providerInstanceForId, parseInstant("2026-10-01T00:02:00Z")!!).summary.status
        assertEquals(ThreadStatus.Waiting, status(projection))
        assertEquals(ThreadStatus.Idle, status(projection.copy(turnItems = listOf(command))))
        val abandoned = projection.copy(runs = listOf(JsonObject(run + ("status" to JsonPrimitive("rolled_back")))))
        assertTrue(abandoned.pendingBackgroundWork().isEmpty())
    }

    @Test fun `subagent failures do not offer root run usage limit recovery`() {
        val run = obj("""{"id":"run","ordinal":1,"status":"failed","rootNodeId":"root"}""")
        val failure = obj("""{"class":"usage_limit","message":"Limit reached","resetAt":"2026-10-01T01:00:00Z"}""")
        val childError = JsonObject(item("child-error", 2, "error") + mapOf("nodeId" to JsonPrimitive("child"),
            "status" to JsonPrimitive("failed"), "failure" to failure))
        val projection = V2ProjectionDto(metadata, runs = listOf(run), turnItems = listOf(childError))
        assertFalse(v2Presentation(projection).usageLimitReached)
        val rootError = JsonObject(childError + ("nodeId" to JsonPrimitive("root")))
        assertTrue(v2Presentation(projection.copy(turnItems = listOf(rootError))).usageLimitReached)
    }

    @Test fun `automatic completion runs do not enter the editable user queue`() {
        val projection = V2ProjectionDto(metadata,
            runs = listOf(obj("""{"id":"automatic","ordinal":1,"status":"queued","userMessageId":"completion","queueHeld":true}"""),
                obj("""{"id":"user","ordinal":2,"status":"queued","userMessageId":"prompt"}""")),
            messages = listOf(obj("""{"id":"completion","delegatedCompletion":{"parentRunId":"parent","generation":1,"taskIds":[]}}"""),
                obj("""{"id":"prompt","text":"My next task"}""")))
        val state = v2Presentation(projection)
        assertEquals(listOf("user"), state.queuedRuns.map { it.id })
        assertTrue(state.queueHeld)
        assertFalse(state.canPromoteQueued)
    }

    @Test fun `model picker follows the attached sessions handoff capability`() {
        val native = obj("""{"id":"native","appThreadId":"thread","providerSessionId":"session"}""")
        val session = obj("""{"id":"session","status":"running","capabilities":{"sessions":{"supportsProviderSwitchingViaHandoff":false}}}""")
        val projection = V2ProjectionDto(metadata, providerThreads = listOf(native), providerSessions = listOf(session))
        assertFalse(v2Presentation(projection).canSwitchProvider)
        val enabled = JsonObject(session + ("capabilities" to obj("""{"sessions":{"supportsProviderSwitchingViaHandoff":true}}""")))
        assertTrue(v2Presentation(projection.copy(providerSessions = listOf(enabled))).canSwitchProvider)
        assertTrue(v2Presentation(V2ProjectionDto(metadata)).canSwitchProvider)
    }

    @Test fun `steering requires an active attempt and supports interrupt restart providers`() {
        val run = obj("""{"id":"run","status":"running","activeAttemptId":"attempt","providerThreadId":"native"}""")
        val projection = V2ProjectionDto(metadata, runs = listOf(run),
            providerThreads = listOf(obj("""{"id":"native","appThreadId":"thread","providerSessionId":"session"}""")),
            providerSessions = listOf(obj("""{"id":"session","status":"running","capabilities":{"turns":{"supportsActiveSteering":false,"supportsSteeringByInterruptRestart":true,"supportsQueuedMessages":true}}}""")))
        assertFalse(v2Presentation(projection).canSteer)
        val running = projection.copy(providerTurns = listOf(obj("""{"id":"turn","runAttemptId":"attempt","status":"running"}""")))
        assertTrue(v2Presentation(running).canSteer)
        assertTrue(v2Presentation(running).canPromoteQueued)
        assertTrue(v2Presentation(running).canReorderQueue)
        val waiting = running.copy(runs = listOf(JsonObject(run + ("status" to JsonPrimitive("waiting")))))
        assertFalse(v2Presentation(waiting).canSteer)
    }

    @Test fun `stale attached sessions do not use unrelated handoff capabilities`() {
        val projection = V2ProjectionDto(metadata,
            runs = listOf(obj("""{"id":"run","status":"running","providerThreadId":"native"}""")),
            providerThreads = listOf(obj("""{"id":"native","providerSessionId":"missing"}""")),
            providerSessions = listOf(obj("""{"id":"other","status":"running","capabilities":{"sessions":{"supportsProviderSwitchingViaHandoff":true}}}""")))
        assertFalse(v2Presentation(projection).canSwitchProvider)
    }
}
