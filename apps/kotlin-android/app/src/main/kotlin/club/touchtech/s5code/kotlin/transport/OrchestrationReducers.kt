package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.transport.wire.CheckpointFileDto
import club.touchtech.s5code.kotlin.transport.wire.CheckpointSummaryDto
import club.touchtech.s5code.kotlin.transport.wire.LatestTurnDto
import club.touchtech.s5code.kotlin.transport.wire.MessageDto
import club.touchtech.s5code.kotlin.transport.wire.ModelSelectionDto
import club.touchtech.s5code.kotlin.transport.wire.ProposedPlanDto
import club.touchtech.s5code.kotlin.transport.wire.SessionDto
import club.touchtech.s5code.kotlin.transport.wire.ShellSnapshotDto
import club.touchtech.s5code.kotlin.transport.wire.ShellStreamItemDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadActivityDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadDto
import club.touchtech.s5code.kotlin.transport.wire.TitleRegenerationDto
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.serializer

/**
 * Pure reducers that fold orchestration stream events into the snapshots the
 * projection reads, mirroring `applyShellStreamEvent` and
 * `applyThreadDetailEvent` in `packages/client-runtime/src/state`.
 *
 * These exist because the server sends a snapshot once and then deltas forever.
 * A client that refetches the snapshot per event would work and would also make
 * a streaming reply re-download the whole thread on every token.
 */

/* ── Shell ───────────────────────────────────────────────────────────── */

/**
 * True when a cache contains enough real workspace data to render while the live
 * socket is still connecting. An empty-but-sequenced snapshot is also authoritative
 * (the account may genuinely have no chats), but a synthetic reducer seed is not.
 */
fun ShellSnapshotDto.hasCacheableWorkspaceContent(): Boolean =
    snapshotSequence > 0 || updatedAt != null || projects.isNotEmpty() || threads.isNotEmpty()

/**
 * Folds one shell frame into the current snapshot. Events at or below the
 * snapshot's sequence are dropped, which is what makes an overlapping catch-up
 * replay after a reconnect idempotent.
 */
fun applyShellStreamItem(
    snapshot: ShellSnapshotDto,
    item: ShellStreamItemDto,
): ShellSnapshotDto =
    when (item.kind) {
        "snapshot" -> item.snapshot ?: snapshot
        "synchronized" -> snapshot
        else -> {
            val sequence = item.sequence
            if (sequence == null || sequence <= snapshot.snapshotSequence) snapshot
            else
                when (item.kind) {
                    "project-upserted" ->
                        item.project?.let { project ->
                            snapshot.copy(
                                projects = snapshot.projects.upsertBy(project) { it.id == project.id },
                                snapshotSequence = sequence,
                            )
                        } ?: snapshot
                    "project-removed" ->
                        snapshot.copy(
                            projects = snapshot.projects.filterNot { it.id == item.projectId },
                            snapshotSequence = sequence,
                        )
                    "thread-upserted" ->
                        item.thread?.let { thread ->
                            snapshot.copy(
                                threads = snapshot.threads.upsertBy(thread) { it.id == thread.id },
                                snapshotSequence = sequence,
                            )
                        } ?: snapshot
                    "thread-removed" ->
                        snapshot.copy(
                            threads = snapshot.threads.filterNot { it.id == item.threadId },
                            snapshotSequence = sequence,
                        )
                    // Forward compatible: a newer server's event kind leaves the
                    // snapshot alone rather than taking the subscription down.
                    else -> snapshot
                }
        }
    }

private inline fun <T> List<T>.upsertBy(value: T, predicate: (T) -> Boolean): List<T> =
    if (any(predicate)) map { if (predicate(it)) value else it } else this + value

/* ── Thread detail ───────────────────────────────────────────────────── */

sealed interface ThreadReduction {
    data class Updated(val thread: ThreadDto) : ThreadReduction

    data object Deleted : ThreadReduction

    data object Unchanged : ThreadReduction
}

/**
 * Folds one orchestration event into a thread.
 *
 * Only the events that change something a screen renders are handled; the rest
 * fall through to [ThreadReduction.Unchanged]. Payloads are read positionally
 * from JSON rather than through generated types because the event union has ~30
 * members whose payloads share almost no fields, and a sealed hierarchy for it
 * would be far more code than the dozen reads that actually matter.
 */
fun applyThreadEvent(thread: ThreadDto, event: JsonElement): ThreadReduction {
    val obj = event as? JsonObject ?: return ThreadReduction.Unchanged
    val type = obj.string("type") ?: return ThreadReduction.Unchanged
    val payload = obj["payload"] as? JsonObject ?: JsonObject(emptyMap())
    val occurredAt = obj.string("occurredAt")

    // Events for other threads reach a per-thread subscription only through a
    // server bug, but silently mutating the wrong thread would be much worse
    // than ignoring one event.
    payload.string("threadId")?.let { if (it != thread.id) return ThreadReduction.Unchanged }

    return when (type) {
        "thread.deleted" -> ThreadReduction.Deleted

        "thread.archived" ->
            thread
                .copy(archivedAt = payload.string("archivedAt"), titleRegeneration = null)
                .touched(payload, occurredAt)

        "thread.unarchived" -> thread.copy(archivedAt = null).touched(payload, occurredAt)

        "thread.settled" ->
            thread
                .copy(settledOverride = "settled", settledAt = payload.string("settledAt"))
                .touched(payload, occurredAt)

        "thread.unsettled" ->
            thread
                .copy(
                    // Only a user unsettle pins the thread active; an activity
                    // unsettle is a neutral reset, so the override clears.
                    settledOverride = if (payload.string("reason") == "user") "active" else null,
                    settledAt = null,
                )
                .touched(payload, occurredAt)

        "thread.snoozed" ->
            thread
                .copy(
                    snoozedUntil = payload.string("snoozedUntil"),
                    snoozedAt = payload.string("snoozedAt"),
                )
                .touched(payload, occurredAt)

        "thread.unsnoozed" ->
            thread.copy(snoozedUntil = null, snoozedAt = null).touched(payload, occurredAt)

        "thread.pinned" ->
            thread
                .copy(
                    pinnedAt = payload.string("pinnedAt"),
                    pinOrderKey = payload.string("pinOrderKey") ?: thread.pinOrderKey,
                )
                .touched(payload, occurredAt)

        "thread.unpinned" ->
            thread.copy(pinnedAt = null, pinOrderKey = null).touched(payload, occurredAt)

        "thread.pin-reordered" ->
            thread.copy(pinOrderKey = payload.string("orderKey")).touched(payload, occurredAt)

        "thread.meta-updated" ->
            thread
                .copy(
                    title = payload.string("title") ?: thread.title,
                    branch = if (payload.containsKey("branch")) payload.string("branch") else thread.branch,
                    worktreePath =
                        if (payload.containsKey("worktreePath")) payload.string("worktreePath")
                        else thread.worktreePath,
                    modelSelection =
                        payload.decode("modelSelection", ModelSelectionDto.serializer())
                            ?: thread.modelSelection,
                    titleRegeneration =
                        if (payload.containsKey("titleRegeneration"))
                            payload.decode("titleRegeneration", TitleRegenerationDto.serializer())
                        else thread.titleRegeneration,
                )
                .touched(payload, occurredAt)

        "thread.runtime-mode-set" ->
            thread
                .copy(runtimeMode = payload.string("runtimeMode") ?: thread.runtimeMode)
                .touched(payload, occurredAt)

        "thread.interaction-mode-set" ->
            thread
                .copy(interactionMode = payload.string("interactionMode") ?: thread.interactionMode)
                .touched(payload, occurredAt)

        "thread.message-sent" -> applyMessageSent(thread, payload, occurredAt)

        "thread.session-set" ->
            payload.decode("session", SessionDto.serializer())?.let { session ->
                thread.copy(session = session, latestTurn = latestTurnFor(thread, session))
                    .touched(payload, occurredAt)
            } ?: ThreadReduction.Unchanged

        "thread.turn-start-requested" ->
            thread
                .copy(
                    latestTurn =
                        thread.latestTurn?.takeIf { it.state == "running" }
                            ?: thread.latestTurn,
                    interactionMode = payload.string("interactionMode") ?: thread.interactionMode,
                    runtimeMode = payload.string("runtimeMode") ?: thread.runtimeMode,
                )
                .touched(payload, occurredAt)

        "thread.turn-interrupt-requested" -> {
            val turnId = payload.string("turnId")
            val latest = thread.latestTurn
            if (turnId == null || latest == null || latest.turnId != turnId) {
                ThreadReduction.Unchanged
            } else {
                thread
                    .copy(
                        latestTurn =
                            latest.copy(
                                state = "interrupted",
                                startedAt = latest.startedAt ?: payload.string("createdAt"),
                                completedAt = latest.completedAt ?: payload.string("createdAt"),
                            )
                    )
                    .touched(payload, occurredAt)
            }
        }

        "thread.activity-appended" ->
            payload.decode("activity", ThreadActivityDto.serializer())?.let { activity ->
                thread
                    .copy(
                        activities =
                            thread.activities.filterNot { it.id == activity.id } + activity
                    )
                    .touched(payload, occurredAt)
            } ?: ThreadReduction.Unchanged

        "thread.proposed-plan-upserted" ->
            payload.decode("proposedPlan", ProposedPlanDto.serializer())?.let { plan ->
                thread
                    .copy(proposedPlans = thread.proposedPlans.filterNot { it.id == plan.id } + plan)
                    .touched(payload, occurredAt)
            } ?: ThreadReduction.Unchanged

        "thread.turn-diff-completed" -> applyTurnDiffCompleted(thread, payload, occurredAt)

        "thread.reverted" -> {
            val turnCount = payload.int("turnCount") ?: return ThreadReduction.Unchanged
            thread
                .copy(checkpoints = thread.checkpoints.filter { it.checkpointTurnCount <= turnCount })
                .touched(payload, occurredAt)
        }

        else -> ThreadReduction.Unchanged
    }
}

/**
 * Streaming assistant messages arrive as deltas on the same message id: the
 * `text` in each event is the increment, not the whole message. Appending vs
 * replacing on `streaming` is the difference between a readable reply and a
 * flickering last-chunk-only one.
 */
private fun applyMessageSent(
    thread: ThreadDto,
    payload: JsonObject,
    occurredAt: String?,
): ThreadReduction {
    val messageId = payload.string("messageId") ?: return ThreadReduction.Unchanged
    val streaming = payload["streaming"]?.jsonPrimitive?.booleanOrNull ?: false
    val text = payload.string("text").orEmpty()
    val existing = thread.messages.firstOrNull { it.id == messageId }

    val messages =
        if (existing == null) {
            thread.messages +
                MessageDto(
                    id = messageId,
                    role = payload.string("role") ?: "assistant",
                    text = text,
                    attachments = payload.decodeList("attachments"),
                    turnId = payload.string("turnId"),
                    streaming = streaming,
                    createdAt = payload.string("createdAt"),
                    updatedAt = payload.string("updatedAt"),
                )
        } else {
            thread.messages.map { message ->
                if (message.id != messageId) message
                else
                    message.copy(
                        text =
                            when {
                                streaming -> message.text + text
                                text.isNotEmpty() -> text
                                else -> message.text
                            },
                        streaming = streaming,
                        turnId = payload.string("turnId") ?: message.turnId,
                        updatedAt = if (streaming) message.updatedAt else payload.string("updatedAt"),
                        attachments = payload.decodeList("attachments") ?: message.attachments,
                    )
            }
        }

    val turnId = payload.string("turnId")
    val role = payload.string("role")
    // A turn is not over just because an assistant message completed: providers
    // emit commentary between tool calls, so the session's own status is the
    // authority on whether work is still running.
    val turnStillRunning =
        turnId != null && thread.session?.status == "running" && thread.session.activeTurnId == turnId
    val settles = !streaming && !turnStillRunning
    val latestTurn =
        if (role == "assistant" &&
            turnId != null &&
            (thread.latestTurn == null || thread.latestTurn.turnId == turnId)
        ) {
            val previous = thread.latestTurn?.takeIf { it.turnId == turnId }
            LatestTurnDto(
                turnId = turnId,
                state =
                    if (settles) {
                        when (previous?.state) {
                            "interrupted" -> "interrupted"
                            "error" -> "error"
                            else -> "completed"
                        }
                    } else {
                        "running"
                    },
                requestedAt = previous?.requestedAt ?: payload.string("createdAt"),
                startedAt = previous?.startedAt ?: payload.string("createdAt"),
                completedAt = if (settles) payload.string("updatedAt") else previous?.completedAt,
                assistantMessageId = messageId,
            )
        } else {
            thread.latestTurn
        }

    return thread.copy(messages = messages, latestTurn = latestTurn).touched(payload, occurredAt)
}

private fun applyTurnDiffCompleted(
    thread: ThreadDto,
    payload: JsonObject,
    occurredAt: String?,
): ThreadReduction {
    val turnId = payload.string("turnId") ?: return ThreadReduction.Unchanged
    val checkpoint =
        CheckpointSummaryDto(
            turnId = turnId,
            checkpointTurnCount = payload.int("checkpointTurnCount") ?: 0,
            checkpointRef = payload.string("checkpointRef").orEmpty(),
            status = payload.string("status") ?: "ready",
            files = payload.decodeList<CheckpointFileDto>("files").orEmpty(),
            assistantMessageId = payload.string("assistantMessageId"),
            completedAt = payload.string("completedAt"),
        )
    return thread
        .copy(
            checkpoints = thread.checkpoints.filterNot { it.turnId == turnId } + checkpoint
        )
        .touched(payload, occurredAt)
}

/**
 * A running session defines the latest turn; leaving "running" settles it. The
 * session's `updatedAt` is the authoritative turn end, because a running turn's
 * `completedAt` can only hold a mid-turn checkpoint placeholder.
 */
private fun latestTurnFor(thread: ThreadDto, session: SessionDto): LatestTurnDto? {
    val activeTurn = session.activeTurnId
    if (session.status == "running" && activeTurn != null) {
        val previous = thread.latestTurn?.takeIf { it.turnId == activeTurn }
        return LatestTurnDto(
            turnId = activeTurn,
            state = "running",
            requestedAt = previous?.requestedAt ?: session.updatedAt,
            startedAt = previous?.startedAt ?: session.updatedAt,
            completedAt = null,
            assistantMessageId = previous?.assistantMessageId,
        )
    }
    val settledState =
        when (session.status) {
            "interrupted" -> "interrupted"
            "error" -> "error"
            "idle", "ready", "stopped" -> "completed"
            else -> null
        }
    val latest = thread.latestTurn
    return if (latest != null && latest.state == "running" && settledState != null) {
        latest.copy(state = settledState, completedAt = session.updatedAt)
    } else {
        latest
    }
}

private fun ThreadDto.touched(payload: JsonObject, occurredAt: String?): ThreadReduction =
    ThreadReduction.Updated(copy(updatedAt = payload.string("updatedAt") ?: occurredAt ?: updatedAt))

/* ── JSON helpers ─────────────────────────────────────────────────────── */

/**
 * A present, non-null string. Explicit JSON nulls read as absent, because every
 * nullable field in these payloads means "cleared", and the callers that care
 * about the difference check [JsonObject.containsKey] first.
 */
private fun JsonObject.string(key: String): String? =
    (this[key] as? JsonPrimitive)?.takeIf { it !is JsonNull }?.content

private fun JsonObject.int(key: String): Int? =
    (this[key] as? JsonPrimitive)?.takeIf { it !is JsonNull }?.content?.toIntOrNull()

private fun <T> JsonObject.decode(
    key: String,
    serializer: kotlinx.serialization.KSerializer<T>,
): T? {
    val element = this[key]?.takeIf { it !is JsonNull } ?: return null
    return runCatching { TransportJson.decodeFromJsonElement(serializer, element) }.getOrNull()
}

private inline fun <reified T> JsonObject.decodeList(key: String): List<T>? {
    val element = this[key]?.takeIf { it !is JsonNull } ?: return null
    return runCatching {
            TransportJson.decodeFromJsonElement(ListSerializer(serializer<T>()), element)
        }
        .getOrNull()
}
