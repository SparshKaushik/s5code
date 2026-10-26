package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.data.v2Presentation
import club.touchtech.s5code.kotlin.data.formatTokens
import club.touchtech.s5code.kotlin.data.asShell
import club.touchtech.s5code.kotlin.data.resumeCompactionAge
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

    @Test fun `inherited tool output retains its source thread for detail and assets`() {
        val tool = JsonObject(item("screenshot", 1, "dynamic_tool") + mapOf(
            "outputOmitted" to JsonPrimitive(true),
            "toolName" to JsonPrimitive("screenshot"),
        ))
        val projection = V2ProjectionDto(metadata, visibleTurnItems = listOf(
            row(tool).copy(visibility = "inherited", sourceThreadId = "parent"),
        ))
        val entry = v2Presentation(projection).feed.single() as FeedEntry.ToolCall
        assertEquals("parent", entry.sourceThreadId)
        assertEquals("screenshot", entry.id)
        assertTrue(entry.fetchesDetail)
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

    @Test fun `a pull request link update replaces only the matching link and never inserts`() {
        fun link(number: Int, title: String) = obj(
            """{"host":"github.com","repository":"acme/app","number":$number,"url":"https://github.com/acme/app/pull/$number","source":"manual","linkedAt":"2026-10-01T00:00:00Z","snapshot":{"state":"open","title":"$title"}}"""
        )
        val thread = JsonObject(metadata + ("pullRequests" to JsonArray(listOf(link(1, "one"), link(2, "two")))))
        val projection = V2ProjectionDto(thread)
        val updated = applyV2Event(projection, event("thread.pull-request-link-updated", link(2, "merged two")), false, null)
        assertEquals(listOf("one", "merged two"), updated.pullRequestLinks().map { it.snapshot?.title })
        val unknown = applyV2Event(projection, event("thread.pull-request-link-updated", link(3, "three")), false, null)
        assertEquals(listOf(1, 2), unknown.pullRequestLinks().map { it.number })
        // A thread without the array (an older server) stays as it was.
        assertEquals(metadata, applyV2Event(V2ProjectionDto(metadata), event("thread.pull-request-link-updated", link(2, "x")), false, null).thread)
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
        assertEquals("request", v2Presentation(projection).approvals.firstOrNull()?.id)
        val resolved = applyV2Event(projection, event("runtime-request.updated", JsonObject(request + ("status" to JsonPrimitive("resolved")))), false, null)
        assertTrue(v2Presentation(resolved).approvals.isEmpty())
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

    @Test fun `an open run whose provider turn already ended does not read as generating`() {
        val run = obj("""{"id":"run","status":"running","activeAttemptId":"attempt"}""")
        fun turn(status: String, ordinal: Int = 1) =
            obj("""{"id":"turn-$ordinal","runAttemptId":"attempt","ordinal":$ordinal,"status":"$status"}""")
        val projection = V2ProjectionDto(metadata, runs = listOf(run))
        assertFalse(v2Presentation(projection).turnEnded)
        assertFalse(v2Presentation(projection.copy(providerTurns = listOf(turn("running")))).turnEnded)
        assertFalse(v2Presentation(projection.copy(providerTurns = listOf(turn("pending")))).turnEnded)
        val failed = projection.copy(providerTurns = listOf(turn("failed")))
        assertTrue(v2Presentation(failed).turnEnded)
        assertTrue(threadDetailFrom(EnvironmentId("env"), failed.asThreadDto(),
            ::providerInstanceForId, parseInstant("2026-10-01T00:00:00Z")!!).turnEnded)
        // The attempt's latest provider turn decides.
        assertFalse(v2Presentation(projection.copy(providerTurns = listOf(turn("failed"), turn("running", 2)))).turnEnded)
        // A settled run has nothing left to suppress.
        val settled = failed.copy(runs = listOf(JsonObject(run + ("status" to JsonPrimitive("completed")))))
        assertFalse(v2Presentation(settled).turnEnded)
    }

    @Test fun `compaction rows say whether context is compacting and by how much`() {
        fun label(vararg fields: Pair<String, JsonElement>): String {
            val compaction = JsonObject(item("compact", 1, "compaction") + fields)
            val feed = v2Presentation(V2ProjectionDto(metadata, visibleTurnItems = listOf(row(compaction)))).feed
            return (feed.single() as FeedEntry.Note).message
        }
        assertEquals("Compacting context", label("status" to JsonPrimitive("running"), "beforeTokenCount" to JsonPrimitive(899_000)))
        assertEquals("Context compacted 899K → 19K tokens",
            label("beforeTokenCount" to JsonPrimitive(899_000), "afterTokenCount" to JsonPrimitive(19_000)))
        assertEquals("Compacting context", label("status" to JsonPrimitive("pending")))
        assertEquals("Compacting context", label("status" to JsonPrimitive("waiting")))
        assertEquals("Context compaction failed",
            label("status" to JsonPrimitive("failed"), "beforeTokenCount" to JsonPrimitive(899_000)))
        assertEquals("Context compaction stopped",
            label("status" to JsonPrimitive("interrupted"), "beforeTokenCount" to JsonPrimitive(899_000)))
        assertEquals("Context compaction stopped", label("status" to JsonPrimitive("cancelled")))
        // A provider summary is not the label, as on the other clients.
        assertEquals("Context compacted", label("summary" to JsonPrimitive("Private summary")))
    }

    @Test fun `async questions keep a working run working`() {
        fun shell(requestBlocking: String?, activity: String) = TransportJson.decodeFromJsonElement(ThreadShellDto.serializer(), obj("""{
            "id":"thread","status":"running","activeRunId":"run","latestRunId":"run","activityRunStatus":"$activity",
            "pendingRuntimeRequest":{"id":"q","kind":"user_input","createdAt":"2026-10-01T00:00:00Z"${requestBlocking?.let { ",\"blocking\":$it" }.orEmpty()}}
        }""")).normalizedV2()
        assertFalse(shell("false", "running").hasPendingUserInput)
        // A waiting run has ended its provider turn, so the question asks again.
        assertTrue(shell("false", "waiting").hasPendingUserInput)
        assertTrue(shell("true", "running").hasPendingUserInput)
        // Older servers omit the flag; every question was blocking then.
        assertTrue(shell(null, "running").hasPendingUserInput)
    }

    @Test fun `detail status keeps a run with an async question working and lets a secret ask`() {
        val run = obj("""{"id":"run","status":"running","ordinal":1}""")
        val question = obj("""{"id":"q","kind":"user_input","status":"pending","createdAt":"2026-10-01T00:00:00Z","responseCapability":{"type":"message"}}""")
        val projection = V2ProjectionDto(metadata, runs = listOf(run), runtimeRequests = listOf(question))
        fun summary(value: V2ProjectionDto) = threadDetailFrom(EnvironmentId("env"), value.asThreadDto(),
            ::providerInstanceForId, parseInstant("2026-10-01T00:00:00Z")!!).summary
        assertNotEquals(ThreadStatus.AwaitingInput, summary(projection).status)
        assertTrue(summary(projection).hasPendingRuntimeRequest)
        val secret = JsonObject(item("secret", 2, "secret_request") + mapOf("status" to JsonPrimitive("waiting")))
        assertEquals(ThreadStatus.AwaitingInput, summary(projection.copy(turnItems = listOf(secret))).status)
    }

    @Test fun `a running turn waiting only on another thread is blocked on that wait`() {
        val run = obj("""{"id":"run","status":"running"}""")
        fun tool(name: String, status: String = "running", input: String = "{}") =
            obj("""{"id":"$name","runId":"run","type":"dynamic_tool","toolName":"$name","status":"$status","input":$input}""")
        assertTrue(blockedOnThreadWait(run, listOf(tool("mcp__t3-code__t3_thread_wait"))))
        assertTrue(blockedOnThreadWait(run, listOf(tool("t3-code.delegate_task", input = """{"mode":"wait"}"""))))
        assertFalse(blockedOnThreadWait(run, listOf(tool("t3-code.delegate_task"))))
        assertFalse(blockedOnThreadWait(run, listOf(tool("mcp__t3-code__t3_thread_wait"), tool("shell"))))
        // Finished tools and other runs do not count either way.
        assertFalse(blockedOnThreadWait(run, listOf(tool("mcp__t3-code__t3_thread_wait", status = "completed"))))
        assertFalse(blockedOnThreadWait(obj("""{"id":"run","status":"waiting"}"""), listOf(tool("mcp__t3-code__t3_thread_wait"))))
    }

    @Test fun `token counts format the way the usage page does`() {
        assertEquals("999", formatTokens(999))
        assertEquals("1K", formatTokens(1_000))
        assertEquals("1.50K", formatTokens(1_500))
        assertEquals("19K", formatTokens(19_000))
        assertEquals("12.5K", formatTokens(12_500))
        assertEquals("899K", formatTokens(899_000))
        assertEquals("1.23M", formatTokens(1_234_567))
        assertEquals("2B", formatTokens(2_000_000_000))
        assertEquals("1T", formatTokens(1_000_000_000_000))
    }

    @Test fun `background work holds completion only when awaited, a subagent, or a background task`() {
        assertTrue(backgroundTaskHolds("subagent", awaited = false))
        assertTrue(backgroundTaskHolds("background_task", awaited = false))
        assertTrue(backgroundTaskHolds("monitor", awaited = true))
        assertFalse(backgroundTaskHolds("monitor", awaited = false))
        assertFalse(backgroundTaskHolds("command", awaited = false))
        val run = obj("""{"id":"run","ordinal":1,"status":"completed"}""")
        val monitor = JsonObject(item("watch", 2, "command_execution") + mapOf(
            "status" to JsonPrimitive("running"), "monitor" to JsonPrimitive(true)))
        val projection = V2ProjectionDto(metadata, runs = listOf(run), turnItems = listOf(monitor))
        val tasks = projection.pendingBackgroundWork()
        assertEquals(listOf("monitor"), tasks.map { it.v2String("kind") })
        assertFalse(backgroundWorkHoldsCompletion(tasks))
    }

    private val idleCompact = obj("""{"id":"compact-message","role":"user","text":"/compact","attachments":[],
        "idleCompaction":"onReturn","createdAt":"2026-10-01T00:05:00Z","updatedAt":"2026-10-01T00:05:00Z"}""")

    @Test fun `idle compactions stay out of the shell's status and sort unless they fail`() {
        val earlier = obj("""{"id":"m1","role":"user","text":"Hi","createdBy":"user","createdAt":"2026-10-01T00:00:00Z","updatedAt":"2026-10-01T00:00:00Z"}""")
        val done = obj("""{"id":"run-1","ordinal":1,"status":"completed","userMessageId":"m1"}""")
        val compacting = obj("""{"id":"run-2","ordinal":2,"status":"running","userMessageId":"compact-message"}""")
        val projection = V2ProjectionDto(metadata, runs = listOf(done, compacting), messages = listOf(earlier, idleCompact))
        assertTrue(projection.isSilentRun(compacting))
        val thread = projection.asThreadDto()
        // The open thread sees S5 Code's own compaction and can stop it; the shell stays quiet.
        assertEquals("run-2", thread.latestTurn?.turnId)
        assertEquals("running", thread.session?.status)
        val shell = thread.asShell()
        assertEquals("run-1", shell.latestTurn?.turnId)
        assertEquals("2026-10-01T00:00:00Z", shell.latestUserMessageAt)
        val failed = JsonObject(compacting + ("status" to JsonPrimitive("failed")))
        assertFalse(projection.copy(runs = listOf(done, failed)).isSilentRun(failed))
        // A legacy server marked its own `/compact` as agent-created.
        assertEquals("beforeCacheExpires", idleCompactionOf(obj("""{"role":"user","text":" /COMPACT ","createdBy":"agent","creationSource":"server"}""")))
        assertNull(idleCompactionOf(obj("""{"role":"user","text":"/compact","createdBy":"user"}""")))
        assertFalse(isCompactionCommandMessage(obj("""{"role":"user","text":"/compact","attachments":[{"id":"a"}]}""")))
    }

    @Test fun `a bare compact turn reads as one compaction row`() {
        val run = obj("""{"id":"run","ordinal":1,"status":"running"}""")
        val prompt = JsonObject(item("prompt", 1, "user_message") + mapOf("text" to JsonPrimitive("/compact"),
            "idleCompaction" to JsonPrimitive("beforeCacheExpires")))
        val waiting = V2ProjectionDto(metadata, runs = listOf(run), visibleTurnItems = listOf(row(prompt, 0)))
        val standIn = v2Presentation(waiting).feed.single() as FeedEntry.Note
        assertEquals("Compacting before the cache expires…", standIn.message)
        assertTrue(standIn.compactionCommand && standIn.status == "running")
        val compaction = JsonObject(item("compaction", 2, "compaction") + mapOf("beforeTokenCount" to JsonPrimitive(300_000),
            "afterTokenCount" to JsonPrimitive(40_000)))
        val finished = waiting.copy(runs = listOf(JsonObject(run + ("status" to JsonPrimitive("completed")))),
            visibleTurnItems = listOf(row(prompt, 0), row(compaction, 1)))
        val note = v2Presentation(finished).feed.single() as FeedEntry.Note
        assertEquals("Context automatically compacted before cache expiry 300K → 40K tokens", note.message)
        assertTrue(note.compactionCommand)
        val failed = JsonObject(compaction + mapOf("status" to JsonPrimitive("failed"), "summary" to JsonPrimitive("Prompt too long")))
        assertEquals("Context compaction failed · Prompt too long",
            (v2Presentation(finished.copy(visibleTurnItems = listOf(row(prompt, 0), row(failed, 1)))).feed.single() as FeedEntry.Note).message)
    }

    @Test fun `an answered resume question folds into the compaction it asked for`() {
        val question = "This session is 2h 5m old and uses 412,000 tokens. Compact it before continuing?"
        val ask = JsonObject(item("ask", 1, "user_input_request") + mapOf("requestId" to JsonPrimitive("request"),
            "questions" to JsonArray(listOf(buildJsonObject { put("id", "q"); put("question", question) }))))
        val request = obj("""{"id":"request","kind":"user_input","status":"resolved","answers":{"q":["Compact and continue"]}}""")
        val compaction = item("compaction", 2, "compaction")
        val projection = V2ProjectionDto(metadata, runtimeRequests = listOf(request),
            visibleTurnItems = listOf(row(ask, 0), row(compaction, 1)))
        val note = v2Presentation(projection).feed.single() as FeedEntry.Note
        assertEquals("Context compacted after 2h 5m idle", note.message)
        // The answer names only the next compaction in its run.
        val later = item("later", 3, "compaction")
        val both = v2Presentation(projection.copy(visibleTurnItems = listOf(row(ask, 0), row(compaction, 1), row(later, 2))))
            .feed.filterIsInstance<FeedEntry.Note>()
        assertEquals(listOf("Context compacted after 2h 5m idle", "Context compacted"), both.map { it.message })
        assertEquals("2h 5m", resumeCompactionAge(ask, request))
        val declined = obj("""{"id":"request","kind":"user_input","status":"resolved","answers":{"q":"Don't ask again"}}""")
        assertNull(resumeCompactionAge(ask, declined))
    }

    @Test fun `stop settles only still active local copies of background items`() {
        val command = JsonObject(item("server", 1, "command_execution") + ("status" to JsonPrimitive("running")))
        val inherited = V2VisibleItemDto(1, "inherited", "parent", "server", command)
        val projection = V2ProjectionDto(metadata, turnItems = listOf(command),
            visibleTurnItems = listOf(row(command, 0), inherited))
        assertEquals(listOf("server"), projection.pendingBackgroundTurnItems().map { it.v2String("id") })
        val settled = JsonObject(command + ("status" to JsonPrimitive("interrupted")))
        val next = projection.settleTurnItems(mapOf("server" to settled))
        assertEquals("interrupted", next.turnItems.single().v2String("status"))
        assertEquals(listOf("interrupted", "running"), next.visibleTurnItems.map { it.item.v2String("status") })
        // An event that already ended the item wins over the lookup.
        val completed = next.copy(turnItems = listOf(JsonObject(command + ("status" to JsonPrimitive("completed")))))
        assertSame(completed, completed.settleTurnItems(mapOf("server" to settled)))
    }

    @Test fun `guest share updates replace the thread payload`() {
        val shared = JsonObject(metadata + ("guestShare" to obj("""{"enabled":true}""")))
        val updated = applyV2Event(V2ProjectionDto(metadata), event("thread.guest-share-updated", shared), false, null)
        assertEquals(shared, updated.thread)
    }

    @Test fun `approvals carry their reason and whether feedback reaches the agent`() {
        val run = obj("""{"id":"run","ordinal":1,"status":"waiting"}""")
        val request = obj("""{"id":"request","kind":"command","status":"pending","createdAt":"2026-10-01T00:00:00Z",
            "responseCapability":{"type":"live","supportsFeedback":true}}""")
        val approval = JsonObject(item("approval", 1, "approval_request") + mapOf("requestId" to JsonPrimitive("request"),
            "reason" to JsonPrimitive("Needs network"), "status" to JsonPrimitive("waiting")))
        val projection = V2ProjectionDto(metadata, runs = listOf(run), runtimeRequests = listOf(request), turnItems = listOf(approval))
        val presented = v2Presentation(projection)
        assertEquals("Needs network", presented.approvals.single().reason)
        assertTrue(presented.approvals.single().supportsFeedback)
        assertFalse(presented.canSendNow)
        assertTrue(v2Presentation(V2ProjectionDto(metadata)).canSendNow)
    }
}
