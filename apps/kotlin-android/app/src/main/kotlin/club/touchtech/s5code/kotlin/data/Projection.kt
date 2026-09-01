package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ApprovalKind
import club.touchtech.s5code.kotlin.model.ApprovalPolicy
import club.touchtech.s5code.kotlin.model.Checkpoint
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.FeedEntry
import club.touchtech.s5code.kotlin.model.PendingApproval
import club.touchtech.s5code.kotlin.model.PendingUserInput
import club.touchtech.s5code.kotlin.model.UserInputQuestion
import club.touchtech.s5code.kotlin.model.PlanStep
import club.touchtech.s5code.kotlin.model.PlanStepState
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.ProjectId
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ProviderOptionChoice
import club.touchtech.s5code.kotlin.model.ProviderOptionDescriptor
import club.touchtech.s5code.kotlin.model.ProviderOptionSelection
import club.touchtech.s5code.kotlin.model.ProviderOptionValue
import club.touchtech.s5code.kotlin.model.RuntimeMode
import club.touchtech.s5code.kotlin.model.ThreadDetail
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadSettings
import club.touchtech.s5code.kotlin.model.ThreadStatus
import club.touchtech.s5code.kotlin.model.ThreadSummary
import club.touchtech.s5code.kotlin.model.ToolState
import club.touchtech.s5code.kotlin.model.TurnInfo
import club.touchtech.s5code.kotlin.model.UserInputKind
import club.touchtech.s5code.kotlin.transport.wire.ModelCapabilitiesDto
import club.touchtech.s5code.kotlin.transport.wire.ProjectShellDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadActivityDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadShellDto
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull

/**
 * Maps wire DTOs onto the presentation models the screens already render.
 *
 * This is where the two vocabularies meet. The server speaks in event-sourced
 * facts (a session status, a settled override, an activity kind); the UI speaks
 * in one status per row and one list of feed entries. Every decision below has an
 * RN counterpart, and where it does the comment says which, because two clients
 * disagreeing about whether a thread is "working" is a bug users notice
 * immediately.
 */

/* ── Threads ─────────────────────────────────────────────────────────── */

/**
 * Resolves the single status a row shows.
 *
 * The order is the RN list's, and it is load-bearing in a way the obvious order
 * is not. `buildThreadListV2Layout` partitions first (snoozed shelf, then pinned,
 * then settled, then active) and only then asks `resolveThreadListV2Status` what
 * badge the row wears. So the lifecycle states outrank the failure badge: a
 * thread that errored and has since been settled belongs on the settled shelf,
 * not at the top of the inbox. Checking failure first — which this used to do —
 * pinned every long-dead thread above live work.
 *
 * Blocked-on-you outranks working, which outranks the quiet states, because that
 * is the order a user scanning the list needs. Those three are also the blockers
 * inside snooze and settle, so they can be checked once here.
 */
fun threadStatusOf(shell: ThreadShellDto, nowMillis: Long): ThreadStatus =
    when {
        shell.hasPendingApprovals -> ThreadStatus.AwaitingApproval
        shell.hasPendingUserInput -> ThreadStatus.AwaitingInput
        shell.session?.status == "running" -> ThreadStatus.Working
        shell.session?.status == "starting" -> ThreadStatus.Working
        // A queued turn start is work the user just asked for that no session has
        // adopted yet. Without this the row looks idle for the seconds a provider
        // takes to come up, which reads as "my message was lost".
        hasQueuedTurnStart(shell, nowMillis) -> ThreadStatus.Queued
        isSnoozed(shell, nowMillis) -> ThreadStatus.Snoozed
        isSettled(shell, nowMillis) -> ThreadStatus.Settled
        // Only the session's own status, matching `resolveThreadListV2Status`. A
        // stale `latestTurn.state == "error"` is history: the turn that failed is
        // over, the thread is idle, and badging it failed forever is the "lying
        // label" the repo's guidance calls out.
        shell.session?.status == "error" -> ThreadStatus.Failed
        else -> ThreadStatus.Idle
    }

/**
 * A user message no turn has picked up, within the adoption grace window.
 * Mirrors `hasQueuedTurnStart` in `packages/client-runtime/src/state/threadSettled.ts`,
 * including its two-sided bound: message timestamps come from whichever device
 * sent them, so a clock ahead of this one would otherwise hold the queued state
 * for the whole skew.
 */
fun hasQueuedTurnStart(shell: ThreadShellDto, nowMillis: Long): Boolean {
    val messageAt = shell.latestUserMessageAt?.let(::parseInstant) ?: return false
    if (shell.session?.status == "error") return false
    if (kotlin.math.abs(nowMillis - messageAt) > QUEUED_TURN_START_GRACE_MS) return false
    val turn = shell.latestTurn ?: return true
    return listOf(turn.requestedAt, turn.startedAt, turn.completedAt).all { candidate ->
        candidate == null || (parseInstant(candidate) ?: return@all true) < messageAt
    }
}

/**
 * When the thread last did anything, following `threadLastActivityAt`. The latest
 * of the user's message and the turn's own timestamps, because a turn that ran
 * without a new message is still activity.
 */
private fun threadLastActivityAt(shell: ThreadShellDto): Long? =
    listOfNotNull(
            shell.latestUserMessageAt,
            shell.latestTurn?.requestedAt,
            shell.latestTurn?.startedAt,
            shell.latestTurn?.completedAt,
        )
        .mapNotNull(::parseInstant)
        .maxOrNull()

/**
 * Snooze is an overlay on the active lifecycle, not a fourth state: a snoozed
 * thread stays active in the model and is only hidden until the wake time passes
 * or it raises its hand.
 *
 * Callers check the blocked and working states first, so this only has to add the
 * raised-hand rules from `threadRaisedHandWhileSnoozed`: a *fresh* failure and a
 * run that finished after the snooze are both new information the user has not
 * seen. A thread snoozed while already failed stays snoozed — that snooze was the
 * user saying "I saw it, not now".
 */
private fun isSnoozed(shell: ThreadShellDto, nowMillis: Long): Boolean {
    val until = shell.snoozedUntil?.let(::parseInstant) ?: return false
    if (until <= nowMillis) return false
    val snoozedAt = shell.snoozedAt?.let(::parseInstant)
    val session = shell.session
    if (session?.status == "error") {
        val erroredAt = session.updatedAt?.let(::parseInstant)
        if (snoozedAt == null || erroredAt == null || erroredAt > snoozedAt) return false
    }
    val turn = shell.latestTurn
    if (snoozedAt != null && turn?.state == "completed") {
        val completedAt = turn.completedAt?.let(::parseInstant)
        if (completedAt != null && completedAt > snoozedAt) return false
    }
    return true
}

/**
 * Settled resolution over the server-backed lifecycle, following
 * `effectiveSettled`.
 *
 * Activity blockers hold a thread active regardless of any override, then the
 * user's explicit override wins in both directions, and without one a thread
 * settles on inactivity. That last path is why an old failed thread does not
 * live at the top of the list forever.
 *
 * One deliberate gap against the RN client: it also settles on a merged or closed
 * pull request, and an *open* one blocks the inactivity path entirely. The PR
 * state is not on the thread shell — RN reads it per row from a separate hook — so
 * this cannot see it. The cost is that a quiet thread with an open PR settles here
 * a few days before it would on mobile.
 */
private fun isSettled(shell: ThreadShellDto, nowMillis: Long): Boolean {
    if (shell.hasPendingApprovals || shell.hasPendingUserInput) return false
    if (shell.session?.status == "starting" || shell.session?.status == "running") return false
    // A pin overrides the lifecycle, as it does in the RN partition: pinned threads
    // render above the inbox and never auto-settle out of sight. Snooze still wins,
    // because it is checked before this.
    if (shell.pinnedAt != null) return false
    if (hasQueuedTurnStart(shell, nowMillis)) {
        // The queued blocker alone is forgivable, because it is clock-derived and
        // this `now` is coarser than the one the settle used. When the server
        // already accepted a settle after the message, trust that ruling.
        val settledAt = shell.settledAt?.let(::parseInstant)
        val messageAt = shell.latestUserMessageAt?.let(::parseInstant)
        val serverAdjudicated =
            shell.settledOverride == "settled" &&
                settledAt != null &&
                messageAt != null &&
                settledAt >= messageAt
        if (!serverAdjudicated) return false
    }
    if (shell.settledOverride == "settled") return true
    // "active" is the explicit keep-active pin: it suppresses auto-settle until
    // real activity clears it server-side.
    if (shell.settledOverride == "active") return false
    val lastActivityAt = threadLastActivityAt(shell) ?: return false
    return lastActivityAt < nowMillis - AUTO_SETTLE_AFTER_MS
}

/** True while a thread lives on the archive screen rather than the home list. */
fun isArchived(shell: ThreadShellDto): Boolean = shell.archivedAt != null

fun projectFrom(environmentId: EnvironmentId, dto: ProjectShellDto): Project =
    Project(
        id = ProjectId(dto.id),
        environmentId = environmentId,
        title = dto.title,
        workspaceRoot = dto.workspaceRoot,
        repository = dto.repositoryIdentity?.let { it.displayName ?: it.canonicalKey },
        // The project has no branch of its own; a thread's branch is the thread's.
        // Showing the default here would be a guess that goes stale.
        branch = "",
        faviconPath = dto.faviconPath,
    )

fun threadSummaryFrom(
    environmentId: EnvironmentId,
    shell: ThreadShellDto,
    driverFor: (String) -> ProviderInstance,
    nowMillis: Long,
): ThreadSummary {
    val status = threadStatusOf(shell, nowMillis)
    return ThreadSummary(
        id = ThreadId(shell.id),
        environmentId = environmentId,
        projectId = ProjectId(shell.projectId),
        title = shell.title,
        status = status,
        provider = driverFor(shell.modelSelection.instanceId),
        model = shell.modelSelection.model,
        branch = shell.branch,
        updatedLabel = relativeLabel(shell.updatedAt ?: shell.createdAt, nowMillis),
        updatedAtMillis = parseInstant(shell.updatedAt ?: shell.createdAt) ?: 0,
        pinned = shell.pinnedAt != null,
        snoozedUntilLabel =
            shell.snoozedUntil?.takeIf { status == ThreadStatus.Snoozed }?.let {
                absoluteLabel(it)
            },
        // Only shown while the thread actually reads as failed, matching the RN
        // row. A recovered session still carries the last error it saw, and a red
        // excerpt on a working thread is the "stale label" the repo's guidance
        // calls out.
        lastError = shell.session?.lastError?.takeIf { status == ThreadStatus.Failed },
        archived = isArchived(shell),
        // Only a running turn gets an elapsed label. A finished turn's duration is
        // history, and a row that keeps counting is the "lying spinner" the repo's
        // guidance calls out.
        elapsedLabel =
            shell.latestTurn
                ?.takeIf { it.state == "running" }
                ?.let { turn -> turn.startedAt?.let { elapsedLabel(it, nowMillis) } },
        activeTurnStartedAtMillis =
            shell.latestTurn
                ?.takeIf {
                    it.state == "running" &&
                        (shell.session?.status == "running" || shell.session?.status == "starting")
                }
                ?.startedAt
                ?.let(::parseInstant),
        excerpt = shell.planProgress?.step,
        titleRegenerating = shell.titleRegeneration != null,
    )
}

/* ── Thread detail ───────────────────────────────────────────────────── */

/**
 * Builds the transcript, the open gates, and the checkpoint list from one thread
 * snapshot.
 *
 * Messages and activities are separate lists on the wire and interleave by
 * timestamp in the UI, which is what `buildThreadFeed` does in the RN client.
 * Sorting by `createdAt` with the sequence as a tiebreak keeps a tool call
 * between the two assistant messages it ran between.
 */
fun threadDetailFrom(
    environmentId: EnvironmentId,
    thread: ThreadDto,
    driverFor: (String) -> ProviderInstance,
    nowMillis: Long,
): ThreadDetail {
    val sortedActivities = thread.activities.sortedWith(activityOrder)
    val summary =
        threadSummaryFrom(
            environmentId,
            thread.asShell(),
            driverFor,
            nowMillis,
        )

    val entries = buildList {
        thread.messages.forEach { message ->
            when (message.role) {
                "user" ->
                    add(
                        Sortable(
                            message.createdAt,
                            FeedEntry.UserMessage(
                                id = message.id,
                                text = message.text,
                                timeLabel = timeLabel(message.createdAt),
                                attachments =
                                    message.attachments.orEmpty().map { attachment ->
                                        ComposerAttachment(
                                            id = attachment.id,
                                            name = attachment.name,
                                            mimeType = attachment.mimeType,
                                            sizeBytes = attachment.sizeBytes,
                                            // Attachment bytes are not in the
                                            // snapshot; a sent image renders as a
                                            // named chip rather than a broken
                                            // thumbnail.
                                            uri = "",
                                        )
                                    },
                                atMillis = parseInstant(message.createdAt) ?: 0L,
                            ),
                        )
                    )
                // System messages are provider bookkeeping, not conversation.
                // An assistant message with neither text nor images is skipped for
                // the same reason RN skips it: it renders as an orphaned timestamp.
                "assistant" ->
                    if (message.text.isNotBlank() || !message.attachments.isNullOrEmpty()) {
                        add(
                            Sortable(
                                message.createdAt,
                                FeedEntry.AgentMessage(
                                    id = message.id,
                                    markdown = message.text,
                                    timeLabel = timeLabel(message.createdAt),
                                    streaming = message.streaming,
                                    attachments =
                                        message.attachments.orEmpty().map { attachment ->
                                            ComposerAttachment(
                                                id = attachment.id,
                                                name = attachment.name,
                                                mimeType = attachment.mimeType,
                                                sizeBytes = attachment.sizeBytes,
                                                uri = "",
                                            )
                                        },
                                    turnId = message.turnId,
                                    atMillis = parseInstant(message.createdAt) ?: 0L,
                                    endedAtMillis =
                                        parseInstant(message.updatedAt)
                                            ?: parseInstant(message.createdAt)
                                            ?: 0L,
                                ),
                            )
                        )
                    }
            }
        }

        // Activities collapse among themselves before they interleave with the
        // messages, exactly as `deriveWorkLogEntries` runs before `buildThreadFeed`
        // merges the two lists. Adjacency therefore ignores messages: a tool's
        // completion is the same row as its start even when the agent wrote a
        // paragraph in between.
        addAll(collapseToolLifecycle(sortedActivities))
    }

    val feed =
        entries
            // Timestamp only, and a stable sort, matching `Arr.sortWith` on
            // `Order.Date` in the RN feed. The messages were added first, so an
            // activity sharing a message's exact timestamp stays below it — the
            // sequence must not break that tie, or a tool call jumps above the
            // message that requested it.
            .sortedBy { parseInstant(it.createdAt) ?: 0L }
            .map { it.entry }

    return ThreadDetail(
        summary = summary,
        feed = feed,
        approval = pendingApprovalOf(sortedActivities),
        userInput = pendingUserInputOf(sortedActivities),
        workspaceRoot = thread.worktreePath,
        latestTurn =
            thread.latestTurn?.let { turn ->
                TurnInfo(
                    turnId = turn.turnId,
                    state = turn.state,
                    startedAtMillis = parseInstant(turn.startedAt),
                    completedAtMillis = parseInstant(turn.completedAt),
                )
            },
        sessionStatus = thread.session?.status,
        sessionUpdatedAtMillis = thread.session?.updatedAt?.let(::parseInstant),
        checkpoints =
            thread.checkpoints
                .sortedByDescending { it.checkpointTurnCount }
                .mapIndexed { index, checkpoint ->
                    Checkpoint(
                        id = checkpoint.checkpointTurnCount.toString(),
                        label = checkpoint.checkpointRef,
                        timeLabel = timeLabel(checkpoint.completedAt),
                        filesChanged = checkpoint.files.size,
                        current = index == 0,
                    )
                },
        settings =
            ThreadSettings(
                provider = driverFor(thread.modelSelection.instanceId),
                model = thread.modelSelection.model,
                runtimeMode =
                    if (thread.interactionMode == "plan") RuntimeMode.Plan else RuntimeMode.Default,
                approvalPolicy = approvalPolicyOf(thread.runtimeMode),
                options = providerOptionSelections(thread.modelSelection.options),
            ),
    )
}

/**
 * The thread's stored provider option values.
 *
 * Both stored shapes are read, matching `ProviderOptionSelections` in
 * `packages/contracts/src/model.ts`: the canonical `[{ id, value }]` array and the
 * legacy `{ effort: "max", fastMode: true }` object that predates migration 026.
 * A server that has not migrated still has to render its knobs correctly, and the
 * client never writes the legacy shape back.
 */
internal fun providerOptionSelections(options: JsonElement?): List<ProviderOptionSelection> =
    when (options) {
        is JsonArray ->
            options.mapNotNull { element ->
                val entry = element as? JsonObject ?: return@mapNotNull null
                val id = (entry["id"] as? JsonPrimitive)?.contentOrNull?.takeIf { it.isNotBlank() }
                val value = providerOptionValue(entry["value"])
                if (id == null || value == null) null else ProviderOptionSelection(id, value)
            }
        is JsonObject ->
            options.entries.mapNotNull { (id, element) ->
                providerOptionValue(element)?.let { ProviderOptionSelection(id, it) }
            }
        else -> emptyList()
    }

private fun providerOptionValue(element: JsonElement?): ProviderOptionValue? {
    val primitive = element as? JsonPrimitive ?: return null
    primitive.booleanOrNull?.let { return ProviderOptionValue.Flag(it) }
    // Not `isString`: the legacy object shape held bare values, and a quoted "true"
    // was already handled above.
    return primitive.contentOrNull?.takeIf { it.isNotBlank() }?.let(ProviderOptionValue::Text)
}

/**
 * A model's advertised knobs, from its `capabilities` block. Unknown descriptor
 * types are dropped rather than guessed: a future third shape rendered as a select
 * with no options would be a row that does nothing.
 */
fun optionDescriptorsFrom(capabilities: ModelCapabilitiesDto?): List<ProviderOptionDescriptor> =
    capabilities?.optionDescriptors.orEmpty().mapNotNull { descriptor ->
        if (descriptor.id.isBlank() || descriptor.label.isBlank()) return@mapNotNull null
        when (descriptor.type) {
            "select" ->
                ProviderOptionDescriptor.Select(
                    id = descriptor.id,
                    label = descriptor.label,
                    description = descriptor.description,
                    options =
                        descriptor.options.mapNotNull { choice ->
                            if (choice.id.isBlank()) null
                            else
                                ProviderOptionChoice(
                                    id = choice.id,
                                    label = choice.label.ifBlank { choice.id },
                                    description = choice.description,
                                    isDefault = choice.isDefault,
                                )
                        },
                    currentValue = descriptor.currentValue?.contentOrNull?.takeIf { it.isNotBlank() },
                )
                    // A select with nothing to select is not a control.
                    .takeIf { it.options.isNotEmpty() }
            "boolean" ->
                ProviderOptionDescriptor.Toggle(
                    id = descriptor.id,
                    label = descriptor.label,
                    description = descriptor.description,
                    currentValue = descriptor.currentValue?.booleanOrNull ?: false,
                )
            else -> null
        }
    }

private data class Sortable(val createdAt: String?, val entry: FeedEntry)

private val activityOrder =
    compareBy<ThreadActivityDto>({ it.sequence ?: Long.MAX_VALUE }, { it.createdAt.orEmpty() }, { it.id })

/**
 * Turns the activity list into transcript rows, collapsing each tool's lifecycle
 * into one.
 *
 * A tool emits `tool.started`, then any number of `tool.updated`, then
 * `tool.completed`. The started row is dropped outright (its completion always
 * arrives), and the rest describe *the same call* — so they have to merge, not
 * stack. Without this a thread shows the tool call twice: once running, and again
 * completed below the assistant message that followed, which is the duplicate row
 * users reported.
 *
 * Merging follows `collapseDerivedWorkLogEntries`: adjacent rows collapse when
 * they share a key derived from the tool's identity, and a row that already
 * completed never absorbs the next one, so two genuine calls to the same tool stay
 * two rows. Subagents collapse by task id instead of adjacency, because their
 * progress rows interleave with everything else.
 */
private fun collapseToolLifecycle(sortedActivities: List<ThreadActivityDto>): List<Sortable> {
    val collapsed = mutableListOf<Sortable>()
    // Index into `collapsed`, so a later update rewrites the row in place rather
    // than appending a second one.
    val toolRows = mutableMapOf<String, Int>()
    val taskRows = mutableMapOf<String, Int>()
    var lastToolKey: String? = null

    sortedActivities.forEach { activity ->
        if (isHiddenActivity(activity)) return@forEach
        val entry = feedEntryFor(activity) ?: return@forEach
        val payload = activity.payload as? JsonObject

        val taskId = taskIdOf(activity, payload)
        if (taskId != null) {
            val existing = taskRows[taskId]
            if (existing != null) {
                collapsed[existing] = collapsed[existing].copy(entry = entry)
            } else {
                taskRows[taskId] = collapsed.size
                collapsed += Sortable(activity.createdAt, entry)
            }
            lastToolKey = null
            return@forEach
        }

        val toolKey = toolCollapseKey(activity, payload)
        if (toolKey != null && toolKey == lastToolKey) {
            val index = toolRows.getValue(toolKey)
            val previous = collapsed[index].entry
            // A completed row is final: a following row with the same key is a
            // second call, not another update of the first.
            if (previous !is FeedEntry.ToolCall || previous.state == ToolState.Running) {
                collapsed[index] = collapsed[index].copy(entry = entry)
                return@forEach
            }
        }

        if (toolKey != null) toolRows[toolKey] = collapsed.size
        lastToolKey = toolKey
        collapsed += Sortable(activity.createdAt, entry)
    }
    return collapsed
}

/**
 * Activities the transcript never shows, following `deriveWorkLogEntries` and
 * `isAgentInternalActivity`.
 *
 * Two groups. Bookkeeping ("started" rows, progress heartbeats, context-window
 * accounting, checkpoint captures) is noise the completion or the UI already
 * covers. Agent-internal work is a provider's own background activity, which the
 * RN client routes to its Agents sheet; Android has no such surface, so a
 * *terminal* row for a nested agent stays — a surface that hides rows still owes
 * the user the finish.
 */
private fun isHiddenActivity(activity: ThreadActivityDto): Boolean {
    if (activity.kind == "tool.started") return true
    if (activity.kind == "tool.progress") return true
    if (activity.kind == "task.started") return true
    if (activity.kind == "context-window.updated") return true
    if (activity.kind == "checkpoint.captured") return true
    val payload = activity.payload as? JsonObject ?: return false
    // `ExitPlanMode:` tool rows are the plan-mode boundary, already rendered as the
    // plan card.
    if (
        (activity.kind == "tool.updated" || activity.kind == "tool.completed") &&
            payload.string("detail")?.startsWith("ExitPlanMode:") == true
    ) {
        return true
    }
    // Codex children never emit `task.completed`; a bypassed `task.updated` carrying
    // a terminal status is their only finish signal.
    val terminalTaskRow =
        activity.kind == "task.completed" ||
            (activity.kind == "task.updated" &&
                payload.bool("timelineBypass") == true &&
                payload.string("status") in TERMINAL_TASK_STATUSES)
    if (activity.kind == "task.updated" && !terminalTaskRow) return true
    if (payload.bool("timelineBypass") == true && !terminalTaskRow) return true
    // `agentId` marks ownership, not "hide me": only an agent's own background work
    // is internal, and its terminal row is what tells the user it finished.
    val ownedByAgent = payload.string("agentId") != null
    if (!ownedByAgent) return false
    return !(terminalTaskRow && payload.string("agentKind") == "agent")
}

private val TERMINAL_TASK_STATUSES =
    setOf("idle", "completed", "failed", "cancelled", "interrupted")

/** The subagent this activity belongs to, for identity-based collapsing. */
private fun taskIdOf(activity: ThreadActivityDto, payload: JsonObject?): String? {
    if (activity.kind != "task.progress" &&
        activity.kind != "task.completed" &&
        activity.kind != "task.updated"
    ) {
        return null
    }
    return payload?.string("taskId")
}

/**
 * Identity of a tool call across its lifecycle rows, from
 * `deriveToolLifecycleCollapseKey`. The label loses its trailing "complete" so the
 * finished row matches the running one it belongs to.
 */
private fun toolCollapseKey(activity: ThreadActivityDto, payload: JsonObject?): String? {
    if (activity.kind != "tool.updated" && activity.kind != "tool.completed") return null
    val label =
        (payload?.string("title") ?: activity.summary)
            .replace(Regex("\\s+(?:complete|completed)\\s*$", RegexOption.IGNORE_CASE), "")
            .trim()
    val detail = payload?.string("detail")?.trim().orEmpty()
    val itemType = payload?.string("itemType").orEmpty()
    if (label.isEmpty() && detail.isEmpty() && itemType.isEmpty()) return null
    return listOf(itemType, label, detail).joinToString("\u001f")
}

/**
 * Turns one activity into a transcript row, or null to hide it.
 *
 * The hidden kinds are the ones RN's `deriveWorkLogEntries` drops: lifecycle
 * "started" rows whose completion will arrive anyway, progress heartbeats, and
 * context-window bookkeeping. Showing them turns the transcript into a log.
 */
private fun feedEntryFor(activity: ThreadActivityDto): FeedEntry? {
    val payload = activity.payload as? JsonObject
    val turnId = activity.turnId
    val at = parseInstant(activity.createdAt) ?: 0L
    return when (activity.kind) {
        "tool.updated",
        "tool.completed" ->
            FeedEntry.ToolCall(
                id = activity.id,
                name = activity.summary,
                summary = payload?.string("command") ?: payload?.string("itemType").orEmpty(),
                detail = payload?.string("detail").orEmpty(),
                state =
                    when {
                        activity.tone == "error" -> ToolState.Failed
                        payload?.string("status") == "failed" -> ToolState.Failed
                        activity.kind == "tool.completed" -> ToolState.Succeeded
                        else -> ToolState.Running
                    },
                turnId = turnId,
                atMillis = at,
            )

        "tool.denied" ->
            FeedEntry.ToolCall(
                id = activity.id,
                name = activity.summary,
                summary = payload?.string("toolName").orEmpty(),
                detail = payload?.string("detail") ?: "Denied",
                state = ToolState.Failed,
                turnId = turnId,
                atMillis = at,
            )

        "turn.plan.updated" ->
            planStepsOf(payload)?.let { FeedEntry.PlanUpdate(activity.id, it, turnId, at) }

        // Subagents are the one internal activity worth surfacing: they are work
        // the user did not ask for directly and would otherwise look like a stall.
        "task.started",
        "task.completed",
        "task.updated" ->
            FeedEntry.Subagent(
                id = activity.id,
                name = payload?.string("title") ?: payload?.string("taskType") ?: "Subagent",
                task = payload?.string("detail") ?: activity.summary,
                active = activity.kind == "task.started",
                turnId = turnId,
                atMillis = at,
            )

        "runtime.error" ->
            FeedEntry.ErrorEntry(
                activity.id,
                payload?.string("message") ?: activity.summary,
                turnId,
                at,
            )

        else -> null
    }
}

private fun planStepsOf(payload: JsonObject?): List<PlanStep>? {
    val steps = payload?.get("plan") as? JsonArray ?: return null
    val parsed =
        steps.mapNotNull { element ->
            val step = element as? JsonObject ?: return@mapNotNull null
            val text = step.string("step") ?: return@mapNotNull null
            PlanStep(
                text = text,
                state =
                    when (step.string("status")) {
                        "completed" -> PlanStepState.Done
                        "inProgress" -> PlanStepState.Active
                        else -> PlanStepState.Pending
                    },
            )
        }
    return parsed.takeIf { it.isNotEmpty() }
}

/**
 * Derives the open approval by replaying request/resolve pairs, following
 * `derivePendingApprovals`. The server does not send "the current approval"; it
 * sends the history, and an approval is open until something resolves it.
 *
 * Only the oldest open request is returned: the UI shows one gate at a time, and
 * answering out of order is not something a provider expects.
 */
fun pendingApprovalOf(sortedActivities: List<ThreadActivityDto>): PendingApproval? {
    val open = linkedMapOf<String, PendingApproval>()
    sortedActivities.forEach { activity ->
        val payload = activity.payload as? JsonObject
        val requestId = payload?.string("requestId") ?: return@forEach
        when (activity.kind) {
            "approval.requested" ->
                open[requestId] =
                    PendingApproval(
                        id = requestId,
                        title = activity.summary,
                        detail = payload.string("detail").orEmpty(),
                        command = payload.string("command"),
                        kind =
                            when (payload.string("requestKind")) {
                                "file-change" -> ApprovalKind.FileWrite
                                "file-read" -> ApprovalKind.FileWrite
                                else -> ApprovalKind.Command
                            },
                    )
            "approval.resolved" -> open.remove(requestId)
            // A "stale request" failure means the provider already moved on, so
            // the gate must close or it blocks the composer forever.
            "provider.approval.respond.failed" ->
                if (isStaleRequestFailure(payload.string("detail"))) open.remove(requestId)
        }
    }
    return open.values.firstOrNull()
}

/** Same replay for structured input requests, following `derivePendingUserInputs`. */
fun pendingUserInputOf(sortedActivities: List<ThreadActivityDto>): PendingUserInput? {
    val open = linkedMapOf<String, PendingUserInput>()
    sortedActivities.forEach { activity ->
        val payload = activity.payload as? JsonObject
        val requestId = payload?.string("requestId") ?: return@forEach
        when (activity.kind) {
            "user-input.requested" -> {
                val request = userInputOf(payload) ?: return@forEach
                open[requestId] = request.copy(id = requestId)
            }
            "user-input.resolved" -> open.remove(requestId)
            "provider.user-input.respond.failed" ->
                if (isStaleRequestFailure(payload.string("detail"))) open.remove(requestId)
        }
    }
    return open.values.firstOrNull()
}

/** Reads every structured question from a `user-input.requested` payload. */
private fun userInputOf(payload: JsonObject): PendingUserInput? {
    val rawQuestions = payload["questions"] as? JsonArray ?: return null
    val questions =
        rawQuestions.mapNotNull { raw ->
            val question = raw as? JsonObject ?: return@mapNotNull null
            val prompt = question.string("question") ?: return@mapNotNull null
            val options =
                (question["options"] as? JsonArray)
                    ?.mapNotNull { (it as? JsonObject)?.string("label") }
                    .orEmpty()
            val multiSelect =
                (question["multiSelect"] as? JsonPrimitive)?.booleanOrNull == true
            UserInputQuestion(
                id = question.string("id") ?: "answer-${questionsHash(rawQuestions, raw)}",
                header = question.string("header") ?: "Question",
                prompt = prompt,
                kind =
                    when {
                        options.isEmpty() -> UserInputKind.Text
                        multiSelect -> UserInputKind.MultiSelect
                        else -> UserInputKind.SingleSelect
                    },
                options = options,
            )
        }
    return questions.takeIf { it.isNotEmpty() }?.let {
        PendingUserInput(id = "", questions = it)
    }
}

/** Stable fallback only for malformed legacy payloads that omitted an id. */
private fun questionsHash(questions: JsonArray, question: JsonObject): String =
    questions.indexOf(question).coerceAtLeast(0).toString()

private fun isStaleRequestFailure(detail: String?): Boolean {
    val normalized = detail?.lowercase() ?: return false
    return normalized.contains("stale pending") || normalized.contains("unknown pending")
}

/**
 * The runtime mode maps onto the permission rows the settings sheet offers.
 * `auto` and `full-access` are distinct server-side but read the same to a user
 * choosing a permission level, so both land on the least restrictive row.
 */
private fun approvalPolicyOf(runtimeMode: String): ApprovalPolicy =
    when (runtimeMode) {
        "approval-required" -> ApprovalPolicy.Ask
        "auto-accept-edits" -> ApprovalPolicy.AutoEdit
        else -> ApprovalPolicy.Full
    }

fun ApprovalPolicy.toRuntimeMode(): String =
    when (this) {
        ApprovalPolicy.Ask -> "approval-required"
        ApprovalPolicy.AutoEdit -> "auto-accept-edits"
        ApprovalPolicy.Full -> "full-access"
    }

/** A detail snapshot carries every shell field, so the row projection is reused. */
private fun ThreadDto.asShell(): ThreadShellDto =
    ThreadShellDto(
        id = id,
        projectId = projectId,
        title = title,
        modelSelection = modelSelection,
        runtimeMode = runtimeMode,
        interactionMode = interactionMode,
        branch = branch,
        worktreePath = worktreePath,
        latestTurn = latestTurn,
        createdAt = createdAt,
        updatedAt = updatedAt,
        archivedAt = archivedAt,
        settledOverride = settledOverride,
        settledAt = settledAt,
        snoozedUntil = snoozedUntil,
        snoozedAt = snoozedAt,
        pinnedAt = pinnedAt,
        pinOrderKey = pinOrderKey,
        titleRegeneration = titleRegeneration,
        session = session,
        latestUserMessageAt = messages.lastOrNull { it.role == "user" }?.createdAt,
        hasPendingApprovals = pendingApprovalOf(activities.sortedWith(activityOrder)) != null,
        hasPendingUserInput = pendingUserInputOf(activities.sortedWith(activityOrder)) != null,
        hasActionableProposedPlan = proposedPlans.any { it.implementedAt == null },
    )

/* ── Provider instances ──────────────────────────────────────────────── */

/**
 * Names a provider instance no connected server could describe.
 *
 * The instance id is the only fact available, and the driver is guessed from its
 * shape. That guess exists because a thread's `modelSelection.instanceId` outlives
 * the config that explained it: a thread started on an instance the user has since
 * removed still has to render. Guessing the *driver* is safe — it only picks a
 * glyph — while guessing the instance id would break routing, so the id is always
 * carried through verbatim.
 */
fun providerInstanceForId(instanceId: String): ProviderInstance =
    ProviderInstance(instanceId = instanceId, driver = driverSlugForInstanceId(instanceId))

/**
 * Best-effort driver slug for an unexplained instance id. Servers name their
 * default instance after its driver, so the common case is exact; a user-named
 * instance falls back to the id, and `formatProviderDriverName` title-cases it.
 */
private fun driverSlugForInstanceId(instanceId: String): String =
    when {
        instanceId.startsWith("codex") -> "codex"
        instanceId.startsWith("claude") -> "claudeAgent"
        instanceId.startsWith("cursor") -> "cursor"
        instanceId.startsWith("grok") -> "grok"
        instanceId.startsWith("opencode") -> "opencode"
        instanceId.startsWith("pi") -> "pi"
        else -> instanceId
    }

/* ── Time formatting ─────────────────────────────────────────────────── */

const val QUEUED_TURN_START_GRACE_MS = 2 * 60 * 1_000L

/**
 * How long a thread may sit untouched before it settles on its own. Matches the
 * RN list's `autoSettleAfterDays` default of three days; neither client offers a
 * setting for it yet.
 */
private const val AUTO_SETTLE_AFTER_MS = 3 * 24 * 60 * 60 * 1_000L

/**
 * Parses an ISO-8601 instant. `java.time` is available from API 26, so no
 * desugaring is needed. Unparseable input returns null and every caller treats
 * that as "unknown" rather than as the epoch, which would sort a broken
 * timestamp to the top of the list.
 */
fun parseInstant(value: String?): Long? {
    if (value.isNullOrBlank()) return null
    return runCatching { java.time.Instant.parse(value).toEpochMilli() }
        .recoverCatching {
            java.time.OffsetDateTime.parse(value).toInstant().toEpochMilli()
        }
        .getOrNull()
}

/** "now", "4m", "3h", "2d" — the compact form the RN list uses. */
fun relativeLabel(value: String?, nowMillis: Long): String {
    val millis = parseInstant(value) ?: return ""
    val delta = nowMillis - millis
    return when {
        delta < 60_000 -> "now"
        delta < 3_600_000 -> "${delta / 60_000}m"
        delta < 86_400_000 -> "${delta / 3_600_000}h"
        delta < 7 * 86_400_000L -> "${delta / 86_400_000}d"
        else -> absoluteLabel(value)
    }
}

fun absoluteLabel(value: String?): String {
    val millis = parseInstant(value) ?: return ""
    return java.time.format.DateTimeFormatter.ofPattern("d MMM, HH:mm")
        .withZone(java.time.ZoneId.systemDefault())
        .format(java.time.Instant.ofEpochMilli(millis))
}

fun timeLabel(value: String?): String {
    val millis = parseInstant(value) ?: return ""
    return java.time.format.DateTimeFormatter.ofPattern("HH:mm")
        .withZone(java.time.ZoneId.systemDefault())
        .format(java.time.Instant.ofEpochMilli(millis))
}

/** Elapsed time on a running turn: "12s", "4m 20s". */
fun elapsedLabel(startedAt: String, nowMillis: Long): String? {
    val started = parseInstant(startedAt) ?: return null
    return elapsedLabel(started, nowMillis)
}

/** Elapsed label when the projection already retained the parsed start time. */
fun elapsedLabel(startedAtMillis: Long, nowMillis: Long): String {
    val seconds = ((nowMillis - startedAtMillis) / 1000).coerceAtLeast(0)
    return if (seconds < 60) "${seconds}s" else "${seconds / 60}m ${seconds % 60}s"
}

private fun JsonObject.string(key: String): String? =
    (this[key] as? JsonPrimitive)?.takeIf { it !is JsonNull }?.content?.takeIf { it.isNotBlank() }

private fun JsonObject.bool(key: String): Boolean? =
    (this[key] as? JsonPrimitive)?.booleanOrNull
