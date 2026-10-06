package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.transport.wire.*
import kotlinx.serialization.json.*

internal fun JsonObject.v2String(key: String): String? = (get(key) as? JsonPrimitive)?.contentOrNull
internal fun JsonObject.v2Long(key: String): Long? = (get(key) as? JsonPrimitive)?.longOrNull
internal fun JsonObject.v2Bool(key: String): Boolean = (get(key) as? JsonPrimitive)?.booleanOrNull == true
internal fun JsonObject.v2Objects(key: String): List<JsonObject> =
    (get(key) as? JsonArray).orEmpty().mapNotNull { it as? JsonObject }

internal fun ThreadShellDto.normalizedV2(): ThreadShellDto {
    if (status == null) return this
    val live = activityRunStatus ?: status
    val backgroundHolds = pendingBackgroundTasks.any { it.v2String("kind") != "command" }
    val turnId = activeRunId ?: latestRunId
    return copy(
        latestTurn = turnId?.let {
            LatestTurnDto(it, v2LegacyState(live), latestRunRequestedAt,
                activityRunStartedAt ?: latestRunStartedAt, latestRunCompletedAt)
        },
        session = SessionDto(id, if (backgroundHolds && live == "waiting") "ready" else v2LegacySession(live), providerInstanceId = providerInstanceId,
            activeTurnId = activeRunId, lastError = lastError, updatedAt = updatedAt),
        hasPendingApprovals = pendingRuntimeRequest?.v2String("kind")?.let { it !in setOf("user_input", "auth_refresh", "dynamic_tool_call") } == true,
        hasPendingUserInput = pendingRuntimeRequest?.v2String("kind") == "user_input",
        backgroundLiveness = if (pendingBackgroundTasks.any { it.v2String("kind") != "command" }) "monitoring" else null,
    )
}

internal fun v2LegacyState(status: String): String = when (status) {
    "pending", "preparing", "starting", "running", "waiting" -> "running"
    "failed" -> "error"
    else -> status
}

internal fun v2LegacySession(status: String): String = when (status) {
    "pending", "preparing", "starting" -> "starting"
    "running", "waiting" -> "running"
    "failed" -> "error"
    else -> "ready"
}

internal fun V2ProjectionDto.latestRootFailure(run: JsonObject?): JsonObject? {
    if (run?.v2String("status") != "failed") return null
    return turnItems.filter { it.v2String("type") == "error" && it.v2String("status") == "failed" &&
        it.v2String("runId") == run.v2String("id") && it.v2String("nodeId") == run.v2String("rootNodeId") }
        .maxWithOrNull(compareBy<JsonObject>({ it.v2String("updatedAt").orEmpty() }, { it.v2Long("ordinal") ?: 0 }, { it.v2String("id").orEmpty() }))
        ?.get("failure") as? JsonObject
}

/**
 * The thread's decoded `pullRequests` link array, or empty when the field is
 * absent (a server that predates linking) or fails to decode — an unreadable
 * link set behaves like none rather than taking the projection down.
 */
internal fun V2ProjectionDto.pullRequestLinks(): List<ThreadPullRequestLinkDto> {
    val raw = thread["pullRequests"] ?: return emptyList()
    return runCatching {
        TransportJson.decodeFromJsonElement(
            kotlinx.serialization.builtins.ListSerializer(ThreadPullRequestLinkDto.serializer()),
            raw,
        )
    }.getOrNull().orEmpty()
}

/**
 * `pull-request-watch:<host>/<repo>#<n>` monitor rows for links the server is
 * watching: a watch wakes the agent, so the thread stays working between
 * wakes instead of returning to the inbox. Tombstoned stack members are
 * excluded, matching `pullRequestWatchTasks` in
 * `packages/shared/src/orchestrationV2PendingBackgroundWork.ts`.
 */
private fun V2ProjectionDto.pullRequestWatchTasks(): List<JsonObject> {
    return pullRequestLinks().filter { it.source != "stack-dismissed" && it.watch != null }.map { link ->
        buildJsonObject {
            put("taskId", "pull-request-watch:${threadPullRequestKeyOf(link)}")
            put("kind", "monitor")
            put("description", "Watching pull request #${link.number}")
        }
    }
}

/**
 * Commands left running do not hold completion; subagents and monitors do.
 *
 * [includePullRequestWatches] adds the thread's server-side PR watches as
 * monitor tasks — the "Waiting on" pill wants them. Callers that pick a run to
 * interrupt pass false: Stop ends watches on its own, and counting them as
 * run-bound work would interrupt a settled run that owns nothing.
 */
internal fun V2ProjectionDto.pendingBackgroundWork(
    includePullRequestWatches: Boolean = true,
): List<JsonObject> {
    if (runs.any { it.v2String("status") in setOf("preparing", "starting", "running") }) return emptyList()
    val latest = runs.filterNot { it.v2String("status") == "queued" && it.v2Bool("queueHeld") }.maxByOrNull { it.v2Long("ordinal") ?: 0 }
    // A thread that never ran waits on nothing else, but a watch started on it still wakes it.
    if (latest == null) return if (includePullRequestWatches) pullRequestWatchTasks() else emptyList()
    if (latest.v2String("status") !in setOf("completed", "cancelled", "failed", "interrupted", "waiting")) return emptyList()
    val tasks = linkedMapOf<String, JsonObject>()
    val providerId = thread.v2String("activeProviderThreadId")
    providerThreads.filter { providerId == null || it.v2String("id") == providerId }.forEach { native ->
        native.v2Objects("pendingBackgroundTasks").forEach { task ->
            task.v2String("taskId")?.let { tasks.putIfAbsent(it, task) }
        }
    }
    val rolledBack = runs.filter { it.v2String("status") == "rolled_back" }.mapTo(hashSetOf()) { it.v2String("id") }
    turnItems.filter { it.v2String("type") in setOf("command_execution", "dynamic_tool", "subagent") &&
        it.v2String("status") in setOf("pending", "running", "waiting") && it.v2String("runId") !in rolledBack &&
        !(it.v2String("type") == "dynamic_tool" && (it["input"] as? JsonObject)?.v2Bool("persistent") == true) }
        .forEach { item ->
            val id = (item["nativeItemRef"] as? JsonObject)?.v2String("nativeId") ?: item.v2String("id") ?: return@forEach
            tasks.putIfAbsent(id, buildJsonObject {
                put("taskId", id)
                put("kind", when (item.v2String("type")) { "command_execution" -> "command"; "subagent" -> "subagent"; else -> "background_task" })
                put("description", item.v2String("title") ?: item.v2String("prompt") ?: item.v2String("toolName"))
                item.v2String("childThreadId")?.let { put("childThreadId", it) }
            })
        }
    if (includePullRequestWatches) pullRequestWatchTasks().forEach { task ->
        task.v2String("taskId")?.let { tasks.putIfAbsent(it, task) }
    }
    return tasks.values.toList()
}

/** Adapt the control plane for existing Kotlin screens; the V2 projection remains authoritative. */
internal fun V2ProjectionDto.asThreadDto(): ThreadDto {
    val metadata = TransportJson.decodeFromJsonElement(ThreadDto.serializer(), thread)
    val active = runs.filter { it.v2String("status") in setOf("preparing", "starting", "running", "waiting") }
        .maxByOrNull { it.v2Long("ordinal") ?: 0 }
    val latest = active ?: runs.filter { it.v2String("status") !in setOf("queued", "cancelled", "rolled_back") }
        .maxByOrNull { it.v2Long("ordinal") ?: 0 }
    val provider = thread.v2String("providerInstanceId")
    val session = providerSessions.lastOrNull { it.v2String("providerInstanceId") == provider }
    val runless = nodes.lastOrNull { it.v2String("kind") == "root_turn" && it.v2String("runId") == null }
    val native = thread.v2String("creationSource") == "provider" &&
        (thread["lineage"] as? JsonObject)?.v2String("relationshipToParent") == "subagent"
    val status = latest?.v2String("status") ?: if (native) runless?.v2String("status") ?: "idle" else "idle"
    val rootFailure = latestRootFailure(latest)
    val backgroundHolds = pendingBackgroundWork().any { it.v2String("kind") != "command" }
    return metadata.copy(
        projection = this,
        latestTurn = (latest ?: if (native) runless else null)?.let {
            LatestTurnDto(it.v2String("id")!!, v2LegacyState(status), it.v2String("requestedAt"),
                 it.v2String("workStartedAt") ?: it.v2String("startedAt") ?: it.v2String("requestedAt"), it.v2String("completedAt"))
        },
        session = SessionDto(metadata.id, if (backgroundHolds && status == "waiting") "ready" else v2LegacySession(status), session?.v2String("driver"), provider,
            metadata.runtimeMode, active?.v2String("id"), session?.v2String("lastError") ?: rootFailure?.v2String("message"), updatedAt),
        updatedAt = updatedAt ?: metadata.updatedAt,
        checkpoints = checkpoints.filter { it.v2Long("appRunOrdinal") != null && it.v2String("status") == "ready" }.map { checkpoint ->
            CheckpointSummaryDto(
                turnId = checkpoint.v2String("runId").orEmpty(),
                checkpointTurnCount = checkpoint.v2Long("appRunOrdinal")?.toInt() ?: 0,
                checkpointRef = checkpoint.v2String("ref").orEmpty(),
                status = checkpoint.v2String("status") ?: "missing",
                files = checkpoint.v2Objects("files").map {
                    TransportJson.decodeFromJsonElement(CheckpointFileDto.serializer(), it)
                },
                completedAt = checkpoint.v2String("capturedAt"),
            )
        },
    )
}

private fun List<JsonObject>.v2Upsert(value: JsonObject): List<JsonObject> {
    val id = value.v2String("id") ?: return this
    val index = indexOfFirst { it.v2String("id") == id }
    if (index < 0) return this + value
    return toMutableList().apply { this[index] = value }
}

internal fun V2ProjectionDto.isVisible(item: JsonObject): Boolean {
    val runId = item.v2String("runId") ?: return true
    val status = runs.firstOrNull { it.v2String("id") == runId }?.v2String("status")
    if (status == "rolled_back") return false
    if (status == "cancelled" && item.v2String("type") == "user_message" &&
        item.v2String("inputIntent") == "queued_turn") return false
    if (item.v2String("type") == "run_interrupt_result" && attempts.any {
        it.v2String("runId") == runId && it.v2String("rootNodeId") == item.v2String("nodeId") &&
            it.v2String("status") == "superseded"
    } && turnItems.none { it.v2String("type") == "run_interrupt_request" && it.v2String("runId") == runId }) return false
    return true
}

/** Index control-plane state once when reconciling an entire timeline. */
internal fun V2ProjectionDto.visibilityPredicate(): (JsonObject) -> Boolean {
    val statuses = runs.associate { it.v2String("id") to it.v2String("status") }
    val superseded = attempts.filter { it.v2String("status") == "superseded" }
        .mapTo(hashSetOf()) { it.v2String("runId") to it.v2String("rootNodeId") }
    val interruptRuns = turnItems.filter { it.v2String("type") == "run_interrupt_request" }
        .mapTo(hashSetOf()) { it.v2String("runId") }
    return { item ->
        val runId = item.v2String("runId")
        val status = statuses[runId]
        status != "rolled_back" &&
            !(status == "cancelled" && item.v2String("type") == "user_message" && item.v2String("inputIntent") == "queued_turn") &&
            !(item.v2String("type") == "run_interrupt_result" && runId != null && item.v2String("nodeId") != null &&
                (runId to item.v2String("nodeId")) in superseded && runId !in interruptRuns)
    }
}

/** Incremental V2 replay, including rollback visibility and bounded-history watermarks. */
internal fun applyV2Event(
    projection: V2ProjectionDto,
    event: JsonObject,
    partialTimeline: Boolean,
    latestLocalTurnOrdinal: Long?,
): V2ProjectionDto {
    if (event.v2String("threadId") != projection.thread.v2String("id")) return projection
    val payload = event["payload"] as? JsonObject ?: return projection
    val type = event.v2String("type") ?: return projection
    val base = if (type in setOf("thread.visited", "thread.marked-unread")) projection
        else projection.copy(updatedAt = event.v2String("occurredAt") ?: projection.updatedAt)
    val next = when (type) {
        "thread.created", "thread.archived", "thread.unarchived", "thread.deleted", "thread.settled",
        "thread.unsettled", "thread.snoozed", "thread.unsnoozed", "thread.auto-settle-set",
        "thread.pinned", "thread.unpinned", "thread.pin-reordered", "thread.active-reordered",
        "thread.metadata-updated", "thread.pull-request-synced", "thread.runtime-mode-updated",
        "thread.interaction-mode-updated", "thread.model-selection-updated", "thread.provider-switched",
        "thread.visited", "thread.marked-unread" -> base.copy(thread = payload)
        "run.created", "run.updated" -> base.copy(runs = base.runs.v2Upsert(payload))
        "run.background-work-cancelled" -> base.copy(runs = base.runs.map { run ->
            if (run.v2String("id") == payload.v2String("runId")) JsonObject(run +
                ("restartCancelledBackgroundWork" to payload.getValue("restartCancelledBackgroundWork"))) else run
        })
        "run-attempt.created", "run-attempt.updated" -> base.copy(attempts = base.attempts.v2Upsert(payload))
        "node.updated" -> base.copy(nodes = base.nodes.v2Upsert(payload))
        "subagent.updated" -> base.copy(subagents = base.subagents.v2Upsert(payload))
        "provider-session.attached", "provider-session.updated" -> base.copy(providerSessions = base.providerSessions.v2Upsert(payload))
        "provider-session.detached" -> base.copy(providerSessions = base.providerSessions.filterNot {
            it.v2String("id") == payload.v2String("providerSessionId")
        })
        "provider-thread.updated" -> base.copy(providerThreads = base.providerThreads.v2Upsert(payload))
        "provider-turn.updated" -> {
            val old = base.providerTurns.firstOrNull { it.v2String("id") == payload.v2String("id") }
            val value = if ((payload["tokenUsage"] == null || payload["tokenUsage"] == JsonNull) && old?.get("tokenUsage") != null)
                JsonObject(payload + ("tokenUsage" to old.getValue("tokenUsage"))) else payload
            base.copy(providerTurns = base.providerTurns.v2Upsert(value))
        }
        "runtime-request.updated" -> base.copy(runtimeRequests = base.runtimeRequests.v2Upsert(payload))
        "message.updated" -> base.copy(messages = base.messages.v2Upsert(payload))
        "plan.updated" -> base.copy(plans = base.plans.v2Upsert(payload))
        "checkpoint-scope.created" -> base.copy(checkpointScopes = base.checkpointScopes.v2Upsert(payload))
        "checkpoint.captured" -> base.copy(checkpoints = base.checkpoints.v2Upsert(payload))
        "context-handoff.updated" -> base.copy(contextHandoffs = base.contextHandoffs.v2Upsert(payload))
        "context-transfer.created", "context-transfer.updated" -> base.copy(contextTransfers = base.contextTransfers.v2Upsert(payload))
        "turn-item.updated" -> {
            val id = payload.v2String("id") ?: return projection
            val existing = base.visibleTurnItems.firstOrNull { it.sourceItemId == id }
            val ordinal = payload.v2Long("ordinal") ?: 0
            val oldest = base.visibleTurnItems.filter { it.visibility == "local" }.minOfOrNull { it.item.v2Long("ordinal") ?: 0 }
            val outsideWindow = partialTimeline && existing == null && (ordinal <= (latestLocalTurnOrdinal ?: -1) ||
                (oldest != null && ordinal < oldest))
            if (outsideWindow && base.turnItems.none { it.v2String("id") == id }) return projection
            val itemBase = base.copy(turnItems = base.turnItems.v2Upsert(payload))
            val rows = base.visibleTurnItems.filterNot { it.sourceItemId == id }.toMutableList()
            if (!outsideWindow && itemBase.isVisible(payload)) {
                val index = rows.indexOfFirst { it.visibility == "local" &&
                    ((it.item.v2Long("ordinal") ?: 0) > ordinal ||
                        (it.item.v2Long("ordinal") == ordinal && it.sourceItemId > id)) }
                rows.add(if (index < 0) rows.size else index,
                    V2VisibleItemDto(visibility = "local", sourceThreadId = payload.v2String("threadId")!!,
                        sourceItemId = id, item = payload))
            }
            itemBase.copy(visibleTurnItems = rows)
        }
        "checkpoint.rollback-requested" -> base
        else -> return projection
    }
    val reconcile = type in setOf("run.created", "run.updated", "run-attempt.created", "run-attempt.updated") ||
        (type == "turn-item.updated" && (payload.v2String("type") == "run_interrupt_request" ||
            projection.turnItems.any { it.v2String("id") == payload.v2String("id") && it.v2String("type") == "run_interrupt_request" }))
    val visible = if (reconcile) next.visibilityPredicate().let { show ->
        next.visibleTurnItems.filter { it.visibility != "local" || show(it.item) }
    } else next.visibleTurnItems
    return if (reconcile || type == "turn-item.updated") next.copy(
        visibleTurnItems = visible.mapIndexed { index, row -> row.copy(position = index) },
    ) else next
}

/** Older history fills missing rows; current streaming values always win. */
internal fun mergeV2History(projection: V2ProjectionDto, page: V2HistoryPageDto): V2ProjectionDto {
    val currentIds = projection.visibleTurnItems.mapTo(hashSetOf()) { "${it.sourceThreadId}:${it.sourceItemId}" }
    val currentItems = projection.turnItems.associateBy { it.v2String("id") }
    val older = page.items.mapNotNull { row ->
        if ("${row.sourceThreadId}:${row.sourceItemId}" in currentIds) return@mapNotNull null
        val local = row.visibility == "local" || row.sourceThreadId == projection.thread.v2String("id")
        val current = if (local) currentItems[row.sourceItemId] else null
        if (current != null && current.v2String("type") != "run_interrupt_request") return@mapNotNull null
        val item = current ?: row.item
        if (local && !projection.isVisible(item)) null else {
            currentIds.add("${row.sourceThreadId}:${row.sourceItemId}")
            row.copy(item = item)
        }
    }
    val rows = (older + projection.visibleTurnItems).mapIndexed { index, row -> row.copy(position = index) }
    val itemIds = projection.turnItems.mapTo(hashSetOf()) { it.v2String("id") }
    return projection.copy(
        visibleTurnItems = rows,
        turnItems = older.filter { it.visibility == "local" && it.sourceItemId !in itemIds }.map { it.item } + projection.turnItems,
    )
}
