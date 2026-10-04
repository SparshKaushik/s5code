package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.*
import club.touchtech.s5code.kotlin.transport.TransportJson
import club.touchtech.s5code.kotlin.transport.isVisible
import club.touchtech.s5code.kotlin.transport.visibilityPredicate
import club.touchtech.s5code.kotlin.transport.v2String
import club.touchtech.s5code.kotlin.transport.v2Long
import club.touchtech.s5code.kotlin.transport.v2Bool
import club.touchtech.s5code.kotlin.transport.v2Objects
import club.touchtech.s5code.kotlin.transport.wire.*
import kotlinx.serialization.json.*

internal data class V2Presentation(
    val feed: List<FeedEntry>,
    val approval: PendingApproval?,
    val userInput: PendingUserInput?,
    val queuedRuns: List<QueuedRun>,
    val providerNativeSubagent: Boolean,
    val canSteer: Boolean,
    val relationships: List<ThreadRelationship>,
    val usageLimitResetAt: String?,
    val limitRecoveryAutoResume: Boolean,
)

/** V2 ordering comes from visibleTurnItems, including inherited fork history, never timestamps. */
internal fun v2Presentation(projection: V2ProjectionDto): V2Presentation {
    val requests = projection.runtimeRequests.associateBy { it.v2String("id") }
    val runs = projection.runs.associateBy { it.v2String("id") }
    val pending = projection.runtimeRequests.filter { it.v2String("status") == "pending" &&
        (it["responseCapability"] as? JsonObject)?.v2String("type") != "not_resumable" }
        .sortedBy { it.v2String("createdAt") }
    val show = projection.visibilityPredicate()
    val items = projection.visibleTurnItems.sortedBy { it.position }.filter {
        it.visibility != "local" || show(it.item)
    }
    val approvalItem = pending.asSequence().filter { it.v2String("kind") !in setOf("user_input", "auth_refresh", "dynamic_tool_call") }
        .mapNotNull { request -> projection.turnItems.firstOrNull {
            it.v2String("requestId") == request.v2String("id") && it.v2String("type") == "approval_request"
        } }.firstOrNull()
    val questionItem = pending.asSequence().filter { it.v2String("kind") == "user_input" }
        .mapNotNull { request -> projection.turnItems.firstOrNull {
            it.v2String("requestId") == request.v2String("id") && it.v2String("type") == "user_input_request"
        } }.firstOrNull()
    val feed = items.mapNotNull { row ->
        val item = row.item
        val id = row.sourceItemId
        val type = item.v2String("type")
        val runId = item.v2String("runId")
        val at = parseInstant(item.v2String("startedAt") ?: item.v2String("updatedAt")) ?: 0
        val end = parseInstant(item.v2String("completedAt") ?: item.v2String("updatedAt")) ?: at
        val state = item.v2String("status")
        val title = item.v2String("title")
        val attachments = item.v2Objects("attachments").map { raw ->
            val dto = TransportJson.decodeFromJsonElement(ChatAttachmentDto.serializer(), raw)
            ComposerAttachment(id = dto.id, name = dto.name, mimeType = dto.mimeType,
                sizeBytes = dto.sizeBytes, type = dto.type, uri = "")
        }
        when (type) {
            "user_message" -> {
                if (runs[runId]?.v2String("status") in setOf("queued", "preparing")) null
                else FeedEntry.UserMessage(id, item.v2String("text").orEmpty(), timeLabel(item.v2String("startedAt")),
                    attachments, contextRecordLabels(item["context"]), atMillis = at)
            }
            "assistant_message" -> if (item.v2String("text").isNullOrBlank() && attachments.isEmpty()) null
                else FeedEntry.AgentMessage(id, item.v2String("text").orEmpty(), timeLabel(item.v2String("startedAt")),
                    item.v2Bool("streaming"), attachments = attachments, turnId = runId, atMillis = at, endedAtMillis = end)
            "reasoning" -> item.v2String("text")?.takeIf { it.isNotBlank() }?.let {
                FeedEntry.Reasoning(id, it, thought = true, turnId = runId, atMillis = at, endedAtMillis = end)
            }
            "todo_list" -> FeedEntry.PlanUpdate(id, item.v2Objects("steps").map { step ->
                PlanStep(step.v2String("text").orEmpty(), when (step.v2String("status")) {
                    "completed" -> PlanStepState.Done
                    "running" -> PlanStepState.Active
                    else -> PlanStepState.Pending
                })
            }, runId, at)
            "proposed_plan" -> FeedEntry.AgentMessage(id, item.v2String("markdown").orEmpty(), "Plan",
                item.v2Bool("streaming"), turnId = runId, atMillis = at, endedAtMillis = end)
            "subagent" -> FeedEntry.Subagent(id, title ?: "Subagent", item.v2String("prompt").orEmpty(),
                state in setOf("pending", "running", "waiting"), runId, at,
                childThreadId = item.v2String("childThreadId"), result = item.v2String("result"), status = state)
            "error" -> FeedEntry.ErrorEntry(id,
                (item["failure"] as? JsonObject)?.v2String("message") ?: title ?: "Provider error", runId, at)
            "compaction" -> FeedEntry.Note(id, item.v2String("summary") ?: "Context compacted", runId, at, compaction = true)
            "handoff" -> FeedEntry.Note(id, "Provider handoff → ${item.v2String("toModel") ?: item.v2String("toProviderInstanceId")}\n" +
                item.v2String("summary").orEmpty(), runId, at)
            "fork" -> FeedEntry.Note(id, "Forked thread: ${item.v2String("targetThreadId")}", runId, at)
            "thread_created" -> FeedEntry.Note(id, "Created thread: ${item.v2String("targetModel")}", runId, at)
            "notification" -> FeedEntry.Note(id, item.v2String("summary").orEmpty() +
                item.v2String("detail")?.let { "\n$it" }.orEmpty(), runId, at)
            "system_notice", "run_interrupt_request", "run_interrupt_result" ->
                FeedEntry.Note(id, item.v2String("message") ?: title.orEmpty(), runId, at)
            "checkpoint" -> null
            "user_input_request" -> {
                val request = requests[item.v2String("requestId")]
                val answers = request?.get("answers") as? JsonObject
                FeedEntry.QuestionAnswer(id,
                    if (request?.v2String("status") == "pending") "User input requested" else "User input resolved",
                    answers?.values?.joinToString(" · ") { (it as? JsonPrimitive)?.contentOrNull ?: it.toString() }.orEmpty(),
                    item.v2Objects("questions").map { question -> FeedEntry.QuestionAnswerLine(
                        question.v2String("question").orEmpty(), answers?.get(question.v2String("id"))?.let {
                            (it as? JsonPrimitive)?.contentOrNull ?: it.toString()
                        }.orEmpty()) }, runId, at)
            }
            "approval_request" -> FeedEntry.Note(id, item.v2String("prompt") ?: title ?: "Approval request", runId, at)
            else -> {
                val detail = when (type) {
                    "command_execution" -> listOfNotNull(item.v2String("input"), item.v2String("output")).joinToString("\n")
                    "file_change" -> item.v2String("diffStr") ?: listOfNotNull(item.v2String("oldStr"), item.v2String("newStr")).joinToString("\n")
                    else -> listOfNotNull(item["input"], item["output"], item["results"]).joinToString("\n") {
                        (it as? JsonPrimitive)?.contentOrNull ?: it.toString()
                    }
                }
                FeedEntry.ToolCall(id, title ?: item.v2String("toolName") ?: type.orEmpty().replace('_', ' '),
                    item.v2String("fileName") ?: item.v2String("pattern") ?: item.v2String("input").orEmpty().take(160), detail,
                    when { state in setOf("pending", "running", "waiting") -> ToolState.Running
                        state in setOf("failed", "cancelled", "interrupted") || item.v2Bool("outputIndicatesFailure") -> ToolState.Failed
                        else -> ToolState.Succeeded }, runId, at, itemType = type.orEmpty(), toolTitle = title,
                    toolName = item.v2String("toolName"), changedFiles = listOfNotNull(item.v2String("fileName")),
                    command = if (type == "command_execution") item.v2String("input") else null,
                    lifecycleStatus = state)
            }
        }
    }
    val lineage = projection.thread["lineage"] as? JsonObject
    val recovery = projection.thread["limitRecovery"] as? JsonObject
    val latestRun = projection.runs.filter { it.v2String("status") !in setOf("queued", "cancelled", "rolled_back") }
        .maxByOrNull { it.v2Long("ordinal") ?: 0 }?.v2String("id")
    val failure = projection.turnItems.lastOrNull { it.v2String("type") == "error" && it.v2String("runId") == latestRun }
        ?.get("failure") as? JsonObject
    return V2Presentation(
        groupConsecutiveThoughts(feed),
        approvalItem?.let { PendingApproval(it.v2String("requestId")!!, it.v2String("title") ?: "Approval required",
            it.v2String("prompt").orEmpty(), null, approvalKindOf(it), it.v2String("appName"),
            it.v2String("requestKind"), approvalOptionsOf(it)) },
        questionItem?.let { item -> userInputOf(item)?.copy(id = item.v2String("requestId")!!,
            dismissible = (requests[item.v2String("requestId")]?.get("responseCapability") as? JsonObject)?.v2String("type") == "message") },
        projection.runs.filter { it.v2String("status") == "queued" }.sortedBy { it.v2Long("queuePosition") ?: Long.MAX_VALUE }
            .map { run -> QueuedRun(run.v2String("id")!!, run.v2String("userMessageId")!!,
                projection.messages.firstOrNull { it.v2String("id") == run.v2String("userMessageId") }?.v2String("text").orEmpty(),
                run.v2Bool("queueHeld")) },
        projection.thread.v2String("creationSource") == "provider" && lineage?.v2String("relationshipToParent") == "subagent",
        projection.runs.lastOrNull { it.v2String("status") in setOf("preparing", "starting", "running", "waiting") }
            ?.let { run -> projection.providerThreads.firstOrNull { it.v2String("id") == run.v2String("providerThreadId") } }
            ?.let { providerThread -> projection.providerSessions.firstOrNull { it.v2String("id") == providerThread.v2String("providerSessionId") } }
            ?.let { ((it["capabilities"] as? JsonObject)?.get("turns") as? JsonObject)?.v2Bool("supportsActiveSteering") } == true,
        buildList {
            lineage?.v2String("parentThreadId")?.let { add(ThreadRelationship(it, "Parent thread")) }
            projection.turnItems.forEach { item ->
                (item.v2String("childThreadId") ?: item.v2String("targetThreadId"))?.let {
                    add(ThreadRelationship(it, item.v2String("title") ?: item.v2String("type") ?: "Thread"))
                }
            }
        }.distinctBy { it.threadId },
        if (failure?.v2String("class") == "usage_limit") failure.v2String("resetAt") else null,
        recovery?.v2Bool("autoResume") == true,
    )
}
