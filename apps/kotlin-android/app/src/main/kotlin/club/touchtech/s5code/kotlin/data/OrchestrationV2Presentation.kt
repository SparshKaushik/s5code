package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.*
import club.touchtech.s5code.kotlin.transport.TransportJson
import club.touchtech.s5code.kotlin.transport.isVisible
import club.touchtech.s5code.kotlin.transport.visibilityPredicate
import club.touchtech.s5code.kotlin.transport.latestRootFailure
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
    val usageLimitReached: Boolean,
    val limitRecoverySnoozed: Boolean,
    val canSwitchProvider: Boolean,
    val queueHeld: Boolean,
    val canReorderQueue: Boolean,
    val canPromoteQueued: Boolean,
)

/** V2 ordering comes from visibleTurnItems, including inherited fork history, never timestamps. */
internal fun v2Presentation(projection: V2ProjectionDto): V2Presentation {
    val requests = projection.runtimeRequests.associateBy { it.v2String("id") }
    val runs = projection.runs.associateBy { it.v2String("id") }
    val activeRun = projection.runs.lastOrNull { it.v2String("status") in setOf("preparing", "starting", "running", "waiting") }
    val native = projection.providerThreads.firstOrNull { it.v2String("id") ==
        (activeRun?.v2String("providerThreadId") ?: projection.thread.v2String("activeProviderThreadId")) }
        ?: projection.providerThreads.firstOrNull { it.v2String("appThreadId") == projection.thread.v2String("id") && it.v2String("providerSessionId") != null }
    val sessionId = native?.v2String("providerSessionId")
    val session = if (sessionId != null) projection.providerSessions.firstOrNull { it.v2String("id") == sessionId }
        else projection.providerSessions.lastOrNull { it.v2String("status") !in setOf("stopped", "error") }
    val capabilities = session?.get("capabilities") as? JsonObject
    val turns = capabilities?.get("turns") as? JsonObject
    val canSteer = activeRun?.v2String("status") == "running" && activeRun.v2String("activeAttemptId") != null &&
        projection.providerTurns.any { it.v2String("runAttemptId") == activeRun.v2String("activeAttemptId") && it.v2String("status") == "running" } &&
        (turns?.v2Bool("supportsActiveSteering") == true || turns?.v2Bool("supportsSteeringByInterruptRestart") == true)
    val automaticMessageIds = projection.messages.filter { (it["delegatedCompletion"] != null && it["delegatedCompletion"] != JsonNull) ||
        (it["notification"] != null && it["notification"] != JsonNull) }.mapTo(hashSetOf()) { it.v2String("id") }
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
    val retryablePreparationRuns = workspacePreparationRetryRunIds(projection)
    val feed = items.mapNotNull { row ->
        val item = row.item
        // Workspace setup is client bookkeeping; preparation failures have
        // their own error item, and a retry cancels that item — so neither the
        // "Preparing workspace" command nor a cancelled preparation error is a
        // row (`turnItemIsWorkspacePreparation` in the RN work log).
        if (turnItemIsWorkspacePreparation(item)) return@mapNotNull null
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
                (item["failure"] as? JsonObject)?.v2String("message") ?: title ?: "Provider error", runId, at,
                retryablePreparationRunId = runId?.takeIf { it in retryablePreparationRuns })
            "secret_request" -> FeedEntry.SecretRequest(id,
                threadId = item.v2String("threadId") ?: row.sourceThreadId,
                label = item.v2String("label") ?: title ?: "Secret request",
                reason = item.v2String("reason").orEmpty(),
                placeholder = item.v2String("placeholder"),
                secretStatus = item.v2String("secretStatus") ?: "pending",
                answerable = item.v2String("secretStatus") == "pending" && row.visibility == "local",
                turnId = runId, atMillis = at)
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
                    lifecycleStatus = state,
                    fetchesDetail = turnItemNeedsDetailFetch(item),
                    detailRevision = turnItemDetailRevision(item),
                    outputImageCount = if (type == "dynamic_tool" && !item.v2Bool("outputOmitted"))
                         toolOutputImageCount(item["output"]) else 0,
                     sourceThreadId = row.sourceThreadId)
            }
        }
    }
    val lineage = projection.thread["lineage"] as? JsonObject
    val recovery = projection.thread["limitRecovery"] as? JsonObject
    val latestRun = projection.runs.filter { it.v2String("status") !in setOf("queued", "cancelled", "rolled_back") }
        .maxByOrNull { it.v2Long("ordinal") ?: 0 }
    val rootFailure = projection.latestRootFailure(latestRun)
    val sessionError = projection.providerSessions.lastOrNull { it.v2String("providerInstanceId") == projection.thread.v2String("providerInstanceId") }?.v2String("lastError")
    val failure = rootFailure?.takeIf { sessionError == null || sessionError == it.v2String("message") }
    return V2Presentation(
        groupConsecutiveThoughts(feed),
        approvalItem?.let { PendingApproval(it.v2String("requestId")!!, it.v2String("title") ?: "Approval required",
            it.v2String("prompt").orEmpty(), null, approvalKindOf(it), it.v2String("appName"),
            it.v2String("requestKind"), approvalOptionsOf(it)) },
        questionItem?.let { item -> userInputOf(item)?.copy(id = item.v2String("requestId")!!,
            dismissible = (requests[item.v2String("requestId")]?.get("responseCapability") as? JsonObject)?.v2String("type") == "message") },
        projection.runs.filter { it.v2String("status") == "queued" && it.v2String("userMessageId") !in automaticMessageIds }
            .sortedWith(compareBy({ it.v2Long("queuePosition") ?: it.v2Long("ordinal") ?: Long.MAX_VALUE }, { it.v2Long("ordinal") ?: 0 }))
            .map { run -> QueuedRun(run.v2String("id")!!, run.v2String("userMessageId")!!,
                projection.messages.firstOrNull { it.v2String("id") == run.v2String("userMessageId") }?.v2String("text").orEmpty(),
                run.v2Bool("queueHeld")) },
        projection.thread.v2String("creationSource") == "provider" && lineage?.v2String("relationshipToParent") == "subagent",
        canSteer,
        buildList {
            lineage?.v2String("parentThreadId")?.let { add(ThreadRelationship(it, "Parent thread")) }
            projection.turnItems.forEach { item ->
                (item.v2String("childThreadId") ?: item.v2String("targetThreadId"))?.let {
                    add(ThreadRelationship(it, item.v2String("title") ?: item.v2String("type") ?: "Thread"))
                }
            }
        }.distinctBy { it.threadId },
        if (failure?.v2String("class") == "usage_limit") failure.v2String("resetAt") else null,
        recovery?.v2Bool("autoResume") == true && recovery.v2String("runId") == latestRun?.v2String("id") && recovery.v2String("resetAt") == failure?.v2String("resetAt"),
        failure?.v2String("class") == "usage_limit",
        recovery?.v2Bool("snooze") == true && recovery.v2String("runId") == latestRun?.v2String("id") &&
            parseInstant(projection.thread.v2String("snoozedUntil")) == parseInstant(failure?.v2String("resetAt")),
        if (session != null) (capabilities?.get("sessions") as? JsonObject)?.v2Bool("supportsProviderSwitchingViaHandoff") == true
            else activeRun == null && (projection.runs.isEmpty() || projection.thread.v2String("historyOrigin") == "v1_import" ||
                projection.providerThreads.any { it.v2String("id") == projection.thread.v2String("activeProviderThreadId") &&
                    it.v2String("appThreadId") == projection.thread.v2String("id") &&
                    it.v2String("providerInstanceId") == (projection.thread["modelSelection"] as? JsonObject)?.v2String("instanceId") &&
                    it["nativeThreadRef"] != null && it["nativeThreadRef"] != JsonNull }),
        projection.runs.any { it.v2String("status") == "queued" && it.v2Bool("queueHeld") },
        turns?.v2Bool("supportsQueuedMessages") == true,
        canSteer,
    )
}

/* ── Turn-item detail and workspace preparation ──────────────────────── */

/**
 * `ORCHESTRATION_V2_WORKSPACE_PREPARATION_FAILURE_CODE`: the error code a
 * failed workspace preparation leaves on its cancelled/failed error item.
 */
internal const val WORKSPACE_PREPARATION_FAILURE_CODE = "workspace_preparation_failed"

/** The literal input a workspace-preparation command item carries. */
private const val WORKSPACE_PREPARATION_INPUT = "Preparing workspace"

/**
 * `turnItemIsWorkspacePreparation`: workspace setup is client bookkeeping and
 * preparation failures have their own error item, so the "Preparing workspace"
 * command row and a cancelled preparation error never render — a retry cancels
 * that item, leaving nothing to say.
 */
internal fun turnItemIsWorkspacePreparation(item: JsonObject): Boolean =
    (item.v2String("type") == "command_execution" && item.v2String("input") == WORKSPACE_PREPARATION_INPUT) ||
        (item.v2String("type") == "error" && item.v2String("status") == "cancelled" &&
            (item["failure"] as? JsonObject)?.v2String("code") == WORKSPACE_PREPARATION_FAILURE_CODE)

/**
 * `workspacePreparationRetryRunIds`: the runs a Retry can prepare again —
 * failed runs with a recorded `workspacePreparation` whose preparation error
 * item is the failure kind, so Retry only appears where `prepared-run.retry`
 * applies. Older servers record no preparation and never offer it.
 */
internal fun workspacePreparationRetryRunIds(projection: V2ProjectionDto): Set<String> {
    val failed = projection.runs
        .filter { it.v2String("status") == "failed" && it["workspacePreparation"] != null }
        .mapNotNull { it.v2String("id") }
        .toSet()
    if (failed.isEmpty()) return emptySet()
    return projection.turnItems.mapNotNull { item ->
        val runId = item.v2String("runId")
        if (item.v2String("type") == "error" && item.v2String("status") == "failed" &&
            (item["failure"] as? JsonObject)?.v2String("code") == WORKSPACE_PREPARATION_FAILURE_CODE &&
            runId != null && runId in failed
        ) runId else null
    }.toSet()
}

/** Live statuses share one cache revision; a settled item keys on `updatedAt`. */
private val LIVE_TURN_ITEM_STATUSES = setOf("idle", "pending", "running", "waiting")

/**
 * `turnItemDetailRevision`: the cache key `getTurnItem` takes. A running item
 * keeps "live", so an open row fetches once while it streams and again when it
 * finishes rather than on every update.
 */
internal fun turnItemDetailRevision(item: JsonObject): String =
    if (item.v2String("status") in LIVE_TURN_ITEM_STATUSES) "live" else item.v2String("updatedAt").orEmpty()

/** Wire projection replaces a large dynamic input with `{ summary, truncated: true }`. */
internal fun JsonObject.isSummarizedValue(): Boolean =
    (this["truncated"] as? JsonPrimitive)?.booleanOrNull == true && v2String("summary") != null

/** `turnItemNeedsDetailFetch`: the timeline item withholds content `getTurnItem` returns. */
internal fun turnItemNeedsDetailFetch(item: JsonObject): Boolean =
    when (item.v2String("type")) {
        "command_execution" -> item.v2Bool("outputOmitted")
        "dynamic_tool" ->
            item.v2Bool("outputOmitted") ||
                (item["input"] as? JsonObject)?.isSummarizedValue() == true
        else -> false
    }

/** `MAX_TOOL_OUTPUT_IMAGES` — a tool returns one screenshot or a few frames. */
internal const val MAX_TOOL_OUTPUT_IMAGES = 8

/**
 * `toolOutputImages`, counted rather than listed: the bytes never ride the
 * timeline, so the row only needs how many `tool-output-image` asset indices to
 * request. Accepts both the MCP `{ data, mimeType }` shape and the Anthropic
 * `{ source: { media_type } }` shape Claude stores.
 */
internal fun toolOutputImageCount(output: JsonElement?): Int {
    val blocks = when (output) {
        is JsonArray -> output
        is JsonObject -> output["content"] as? JsonArray ?: listOf(output)
        else -> return 0
    }
    var count = 0
    for (block in blocks) {
        val record = block as? JsonObject ?: continue
        if (record.v2String("type") != "image") continue
        val source = record["source"] as? JsonObject
        if (source != null && source.v2String("type") != "base64") continue
        val mimeType = (source?.v2String("media_type") ?: record.v2String("mimeType"))
            ?.lowercase() ?: continue
        if (mimeType in ComposerAttachmentLimits.SUPPORTED_IMAGE_MIME_TYPES) {
            count += 1
            if (count >= MAX_TOOL_OUTPUT_IMAGES) break
        }
    }
    return count
}
