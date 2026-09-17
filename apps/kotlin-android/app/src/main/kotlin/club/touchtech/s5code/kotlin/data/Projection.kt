package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ApprovalKind
import club.touchtech.s5code.kotlin.model.ApprovalOption
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
import club.touchtech.s5code.kotlin.model.PullRequestRef
import club.touchtech.s5code.kotlin.model.PullRequestState
import club.touchtech.s5code.kotlin.model.ProviderOptionChoice
import club.touchtech.s5code.kotlin.model.ProviderOptionDescriptor
import club.touchtech.s5code.kotlin.model.ProviderOptionSelection
import club.touchtech.s5code.kotlin.model.ProviderOptionValue
import club.touchtech.s5code.kotlin.model.RuntimeMode
import club.touchtech.s5code.kotlin.model.SentAttachment
import club.touchtech.s5code.kotlin.model.ThreadDetail
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadPage
import club.touchtech.s5code.kotlin.model.ThreadSettings
import club.touchtech.s5code.kotlin.model.ThreadStatus
import club.touchtech.s5code.kotlin.model.ThreadSummary
import club.touchtech.s5code.kotlin.model.ToolState
import club.touchtech.s5code.kotlin.model.TurnInfo
import club.touchtech.s5code.kotlin.model.UserInputKind
import club.touchtech.s5code.kotlin.model.UserInputOption
import club.touchtech.s5code.kotlin.transport.wire.ModelCapabilitiesDto
import club.touchtech.s5code.kotlin.transport.wire.ProjectShellDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadActivityDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadDetailPageDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadShellDto
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.add
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonObject

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

/**
 * Merges an older disjoint page below the currently loaded window — `mergeOlderPage`
 * in `packages/client-runtime/src/state/threads.ts`. All four windowed collections
 * prepend; identity dedupe guards the overlapping-page case so a row never renders
 * twice. Thread metadata stays the loaded (newer) snapshot's.
 */
fun mergeOlderThreadPage(loaded: ThreadDto, older: ThreadDto): ThreadDto {
    fun mergeById(olderRows: List<ThreadActivityDto>, loadedRows: List<ThreadActivityDto>) =
        loadedRows.mapTo(mutableSetOf()) { it.id }.let { seen ->
            olderRows.filter { it.id !in seen } + loadedRows
        }
    val loadedMessageIds = loaded.messages.mapTo(mutableSetOf()) { it.id }
    val loadedPlanIds = loaded.proposedPlans.mapTo(mutableSetOf()) { it.id }
    val loadedCheckpointTurns = loaded.checkpoints.mapTo(mutableSetOf()) { it.turnId }
    return loaded.copy(
        messages = older.messages.filter { it.id !in loadedMessageIds } + loaded.messages,
        activities = mergeById(older.activities, loaded.activities),
        proposedPlans = older.proposedPlans.filter { it.id !in loadedPlanIds } + loaded.proposedPlans,
        checkpoints =
            older.checkpoints.filter { it.turnId !in loadedCheckpointTurns } + loaded.checkpoints,
    )
}

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
        worktreePath = shell.worktreePath,
        updatedLabel = relativeLabel(shell.updatedAt ?: shell.createdAt, nowMillis),
        updatedAtMillis = parseInstant(shell.updatedAt ?: shell.createdAt) ?: 0,
        createdAtMillis = parseInstant(shell.createdAt) ?: 0,
        unsettledAtMillis = parseInstant(shell.unsettledAt) ?: 0,
        pinned = shell.pinnedAt != null,
        pinOrderKey = shell.pinOrderKey,
        activeOrderKey = shell.activeOrderKey,
        snoozedUntilLabel =
            shell.snoozedUntil?.takeIf { status == ThreadStatus.Snoozed }?.let {
                absoluteLabel(it)
            },
        // Only shown while the thread actually reads as failed, matching the RN
        // row. A recovered session still carries the last error it saw, and a red
        // excerpt on a working thread is the "stale label" the repo's guidance
        // calls out.
        lastError = shell.session?.lastError?.takeIf { status == ThreadStatus.Failed },
        pullRequest =
            shell.linkedPullRequest?.let { pr ->
                PullRequestRef(
                    number = pr.number,
                    state = PullRequestState.Open,
                    title = "#${pr.number}",
                )
            },
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
    page: ThreadDetailPageDto? = null,
    loadingOlder: Boolean = false,
): ThreadDetail {
    val sortedActivities = thread.activities.sortedWith(activityOrder)
    val summary =
        threadSummaryFrom(
            environmentId,
            thread.asShell(),
            driverFor,
            nowMillis,
        )

    // A folded user-input record supersedes the server's synthetic user message
    // for the same answers (`async-answer:<requestId>`); rendering both doubles
    // the answer in the transcript.
    val foldedAnswerMessageIds =
        thread.activities
            .asSequence()
            .filter {
                it.kind == "user-input.requested" ||
                    it.kind == "user-input.resolved" ||
                    it.kind == "user-input.answer-submitted"
            }
            .mapNotNull { (it.payload as? JsonObject)?.string("requestId") }
            .map { "async-answer:$it" }
            .toSet()

    val entries = buildList {
        thread.messages.forEach { message ->
            val contextRecordAttachments =
                (message.context as? JsonObject)?.let(::contextAttachmentIdsOf) ?: emptySet()
            when (message.role) {
                "user" ->
                    if (message.id !in foldedAnswerMessageIds) {
                    add(
                        Sortable(
                            message.createdAt,
                            FeedEntry.UserMessage(
                                id = message.id,
                                text = message.text,
                                timeLabel = timeLabel(message.createdAt),
                                attachments =
                                    message.attachments.orEmpty()
                                        // Context-bound attachments render as
                                        // their reference chip, not a second
                                        // thumbnail (`UserMessageContent`'s
                                        // attachment suppression in the RN feed).
                                        .filter { attachment ->
                                            attachment.id !in contextRecordAttachments
                                        }
                                        .map { attachment ->
                                            ComposerAttachment(
                                                id = attachment.id,
                                                name = attachment.name,
                                                mimeType = attachment.mimeType,
                                                sizeBytes = attachment.sizeBytes,
                                                type = attachment.type,
                                                // Attachment bytes are not in the
                                                // snapshot; a sent image renders as a
                                                // named chip rather than a broken
                                                // thumbnail.
                                                uri = "",
                                            )
                                        },
                                contextRecords = contextRecordLabels(message.context),
                                atMillis = parseInstant(message.createdAt) ?: 0L,
                            ),
                        )
                    )
                    }
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
                                                type = attachment.type,
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
        page =
            page?.let {
                ThreadPage(
                    beforeCursor = it.beforeCursor,
                    hasMore = it.hasMore,
                    loadingOlder = loadingOlder,
                )
            },
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

/**
 * `contextId → display label` from a message's `context` records
 * (`OrchestrationMessageContext`): the only fields the bubble needs are the
 * kind and label of each record a `t3-context://` reference can point at.
 * Unknown record kinds are kept — the reference still labels itself.
 */
private fun contextRecordLabels(context: JsonElement?): Map<String, FeedEntry.ContextRecordLabel> {
    val records = (context as? JsonObject)?.get("records") as? JsonArray ?: return emptyMap()
    return records.mapNotNull { element ->
        val record = element as? JsonObject ?: return@mapNotNull null
        val id = record.string("contextId") ?: return@mapNotNull null
        id to
            FeedEntry.ContextRecordLabel(
                kind = record.string("kind").orEmpty(),
                label = record.string("label").orEmpty(),
            )
    }.toMap()
}

/**
 * Attachment ids owned by `image`/`file` context records: those attachments
 * render as their in-text reference, not as a second thumbnail row.
 */
private fun contextAttachmentIdsOf(context: JsonObject): Set<String> {
    val records = context["records"] as? JsonArray ?: return emptySet()
    return records.mapNotNull { element ->
        val record = element as? JsonObject ?: return@mapNotNull null
        val kind = record.string("kind")
        if (kind == "image" || kind == "file") record.string("attachmentId") else null
    }.toSet()
}

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

    foldUserInputActivities(sortedActivities).forEach { activity ->
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
 * Folds `user-input.requested` / `user-input.resolved` /
 * `user-input.answer-submitted` activities into one synthesized
 * `answer-submitted` row per request id, following `foldUserInputActivities` in
 * `packages/client-runtime/src/work-log/userInput.ts`.
 *
 * The question prompts and the submitted answers are spread across the three
 * activities and merge into one payload carrying `questionTextById`, `answers`,
 * and `attachmentsByQuestionId`. The synthesized row takes the *first*
 * activity's slot in the timeline, so a question answered mid-turn renders
 * where it was asked rather than where it was answered.
 */
private fun foldUserInputActivities(
    activities: List<ThreadActivityDto>,
): List<ThreadActivityDto> {
    val groups = linkedMapOf<String, MutableList<ThreadActivityDto>>()
    for (activity in activities) {
        if (activity.kind != "user-input.requested" &&
            activity.kind != "user-input.resolved" &&
            activity.kind != "user-input.answer-submitted"
        ) {
            continue
        }
        val requestId =
            (activity.payload as? JsonObject)?.string("requestId") ?: continue
        groups.getOrPut(requestId) { mutableListOf() }.add(activity)
    }
    if (groups.isEmpty()) return activities

    val replacements = mutableMapOf<ThreadActivityDto, ThreadActivityDto?>()
    for ((requestId, group) in groups) {
        val questions = mutableMapOf<String, JsonObject>()
        val texts = linkedMapOf<String, String>()
        val attachments = linkedMapOf<String, JsonElement>()
        for (activity in group) {
            val payload = activity.payload as? JsonObject ?: continue
            (payload["questionTextById"] as? JsonObject)?.forEach { (id, text) ->
                (text as? JsonPrimitive)?.contentOrNull?.takeIf { it.isNotBlank() }?.let {
                    texts[id] = it
                }
            }
            (payload["questions"] as? JsonArray)?.forEach { raw ->
                val question = raw as? JsonObject ?: return@forEach
                val id = question.string("id") ?: return@forEach
                questions[id] = question
                question.string("question")?.let { texts[id] = it }
            }
            (payload["attachmentsByQuestionId"] as? JsonObject)?.forEach { (id, files) ->
                attachments[id] = files
            }
        }
        val submitted =
            group.lastOrNull {
                it.kind == "user-input.answer-submitted" &&
                    (it.payload as? JsonObject)?.get("answers") is JsonObject
            }
        val rawAnswers =
            (submitted?.payload as? JsonObject)?.get("answers") as? JsonObject
                ?: group.lastOrNull {
                    (it.payload as? JsonObject)?.get("answers") is JsonObject
                }?.let { (it.payload as JsonObject)["answers"] as JsonObject }
                ?: JsonObject(emptyMap())
        // Display maps each stored option `value` back to its label; the wire
        // value is an id the user never saw.
        val answers = linkedMapOf<String, JsonElement>()
        rawAnswers.forEach { (id, value) ->
            val labels = mutableMapOf<String, String>()
            (questions[id]?.get("options") as? JsonArray)?.forEach { raw ->
                val option = raw as? JsonObject ?: return@forEach
                val optionValue = option.string("value")
                val optionLabel = option.string("label")
                if (optionValue != null && optionLabel != null) labels[optionValue] = optionLabel
            }
            answers[id] = displayOptionAnswer(value, labels)
        }
        val submittedAnswer = answers.isNotEmpty() || attachments.isNotEmpty()
        val summary =
            when {
                submittedAnswer -> "User input submitted"
                group.any { it.kind == "user-input.resolved" } -> "User input dismissed"
                else -> "User input requested"
            }
        val foldedPayload =
            buildJsonObject {
                put("requestId", requestId)
                putJsonObject("questionTextById") { texts.forEach { (id, text) -> put(id, text) } }
                put("answers", JsonObject(answers))
                put("attachmentsByQuestionId", JsonObject(attachments))
            }
        for (activity in group) replacements[activity] = null
        replacements[group.first()] =
            group.first().copy(kind = "user-input.answer-submitted", summary = summary,
                tone = "tool", payload = foldedPayload)
    }

    val folded =
        activities.flatMap { activity ->
            when {
                !replacements.containsKey(activity) -> listOf(activity)
                else -> listOfNotNull(replacements[activity])
            }
        }
    return dedupeQuestionToolRows(folded)
}

/**
 * Maps one stored answer back to display text, following `displayOptionAnswer`:
 * strings resolve through the option value→label map, arrays recurse, and the
 * nested `{ answers }` retry shape unwraps.
 */
private fun displayOptionAnswer(value: JsonElement, labels: Map<String, String>): JsonElement =
    when (value) {
        is JsonPrimitive ->
            if (value.isString) JsonPrimitive(labels[value.content] ?: value.content) else value
        is JsonArray -> JsonArray(value.map { displayOptionAnswer(it, labels) })
        is JsonObject -> {
            val nested = value["answers"]
            if (nested != null) {
                JsonObject(value + ("answers" to displayOptionAnswer(nested, labels)))
            } else {
                value
            }
        }
        else -> value
    }

/**
 * Providers that ask questions through a native tool (AskUserQuestion and its
 * cousins) emit the question twice: once as a tool row and once as the folded
 * answer record. `withoutDuplicateQuestionTools` drops the tool copy, keyed on
 * the sorted set of question texts within the same turn.
 */
private fun dedupeQuestionToolRows(
    activities: List<ThreadActivityDto>,
): List<ThreadActivityDto> {
    val fingerprints = mutableSetOf<String>()
    for (activity in activities) {
        if (activity.kind != "user-input.answer-submitted") continue
        val turnId = activity.turnId ?: continue
        val payload = activity.payload as? JsonObject ?: continue
        val texts =
            (payload["questionTextById"] as? JsonObject)
                ?.values
                ?.mapNotNull { (it as? JsonPrimitive)?.contentOrNull?.trim() }
                .orEmpty()
        questionFingerprint(turnId, texts)?.let(fingerprints::add)
    }
    if (fingerprints.isEmpty()) return activities

    val duplicateToolKeys = mutableSetOf<String>()
    for (activity in activities) {
        if (!activity.kind.startsWith("tool.")) continue
        val turnId = activity.turnId ?: continue
        val payload = activity.payload as? JsonObject ?: continue
        val toolCallId = payload.string("toolCallId") ?: continue
        val data = payload["data"] as? JsonObject ?: continue
        val questionTexts = questionToolQuestionTexts(data, payload.string("title")) ?: continue
        val fingerprint = questionFingerprint(turnId, questionTexts) ?: continue
        if (fingerprint in fingerprints) duplicateToolKeys += "$turnId$toolCallId"
    }
    if (duplicateToolKeys.isEmpty()) return activities
    return activities.filter { activity ->
        if (activity.tone == "error") return@filter true
        if (!activity.kind.startsWith("tool.")) return@filter true
        val payload = activity.payload as? JsonObject
        if (payload?.string("status") in setOf("failed", "declined", "stopped", "cancelled")) {
            return@filter true
        }
        val turnId = activity.turnId ?: return@filter true
        val toolCallId = payload?.string("toolCallId") ?: return@filter true
        "$turnId$toolCallId" !in duplicateToolKeys
    }
}

private fun questionFingerprint(turnId: String, texts: List<String>): String? {
    if (texts.isEmpty() || texts.any { it.isEmpty() }) return null
    return turnId + "" + texts.sorted().joinToString("")
}

/**
 * Reads the question prompts out of a native question tool's payload, following
 * `projectQuestionToolInput` in `packages/shared/src/toolActivity.ts`. Null when
 * the payload is not a question tool at all.
 */
private fun questionToolQuestionTexts(data: JsonObject, title: String?): List<String>? {
    val item = data["item"] as? JsonObject
    val toolName =
        data.string("toolName")
            ?: data.string("tool")
            ?: item?.string("tool")
            ?: title
            ?: return null
    val name =
        toolName
            .split(Regex("__|[./]"))
            .lastOrNull()
            ?.replace(Regex("[_\\s]"), "")
            ?.lowercase()
    if (
        name == null ||
            !Regex("^(askuserquestion|requestuserinput(?:async)?|askquestion|question)$")
                .matches(name)
    ) {
        return null
    }
    val input =
        data["input"] as? JsonObject
            ?: data["rawInput"] as? JsonObject
            ?: (data["state"] as? JsonObject)?.get("input") as? JsonObject
            ?: item?.get("arguments") as? JsonObject
            ?: return null
    val questions =
        input["questions"] as? JsonArray
            ?: (input["params"] as? JsonObject)?.get("questions") as? JsonArray
            ?: return null
    return questions.mapNotNull { raw ->
        val question = raw as? JsonObject
        question?.string("question")
            ?: question?.string("question_text")
            ?: question?.string("prompt")
            ?: question?.string("title")
    }
}

/** Builds the history row for a folded user-input record. */
private fun questionAnswerEntry(
    activity: ThreadActivityDto,
    payload: JsonObject?,
    turnId: String?,
    at: Long,
): FeedEntry.QuestionAnswer {
    val texts = payload?.get("questionTextById") as? JsonObject
    val answers = payload?.get("answers") as? JsonObject
    val attachments = payload?.get("attachmentsByQuestionId") as? JsonObject
    val questionIds =
        buildList {
            texts?.keys?.forEach(::add)
            answers?.keys?.forEach(::add)
            attachments?.keys?.forEach(::add)
        }.distinct()
    val lines =
        questionIds.map { questionId ->
            FeedEntry.QuestionAnswerLine(
                question = texts?.string(questionId).orEmpty(),
                answer = questionAnswerText(answers?.get(questionId)),
                // The full `ChatAttachment` record is kept, not just the name:
                // the row links into the attachment viewer, which signs its own
                // URL and needs the id and mime type to do it.
                attachments =
                    (attachments?.get(questionId) as? JsonArray)
                        ?.mapNotNull { raw ->
                            (raw as? JsonObject)?.let { file ->
                                val name = file.string("name") ?: return@let null
                                SentAttachment(
                                    id = file.string("id").orEmpty(),
                                    name = name,
                                    mimeType = file.string("mimeType").orEmpty(),
                                    sizeBytes =
                                        (file["sizeBytes"] as? JsonPrimitive)
                                            ?.longOrNull
                                            ?: 0L,
                                    type = file.string("type") ?: "file",
                                )
                            }
                        }
                        .orEmpty(),
            )
        }
    // The collapsed preview, following `getQuestionAnswerPreview`: answers win,
    // then attachment names, then the bare question texts.
    val preview =
        lines.map { it.answer }.filter { it.isNotBlank() }.joinToString(" · ").ifBlank {
            val names = lines.flatMap { line -> line.attachments.map { it.name } }
            if (names.isNotEmpty()) names.joinToString(", ")
            else texts?.values?.mapNotNull { (it as? JsonPrimitive)?.contentOrNull }
                ?.joinToString(" · ")
                .orEmpty()
        }
    return FeedEntry.QuestionAnswer(
        id = activity.id,
        summary = activity.summary,
        preview = preview,
        lines = lines,
        turnId = turnId,
        atMillis = at,
    )
}

/** Display text for one stored answer, following `getQuestionAnswerText`. */
private fun questionAnswerText(value: JsonElement?): String =
    when (value) {
        is JsonPrimitive -> if (value.isString) value.content else ""
        is JsonArray -> value.map(::questionAnswerText).filter { it.isNotEmpty() }.joinToString(", ")
        is JsonObject -> questionAnswerText(value["answers"])
        else -> ""
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
    if (activity.kind == "context-window.updated") return true
    if (activity.kind == "checkpoint.captured") return true
    // The check is on the summary, not the kind: the same "Checkpoint captured"
    // string arrives under more than one kind, and RN drops them all.
    if (activity.summary == "Checkpoint captured") return true
    // Worktree setup noise stays out of the transcript; a failed setup script is
    // the exception the user needs to see, matching `isWorktreeSetupActivity`.
    if (
        (activity.kind == "setup-script.requested" || activity.kind == "setup-script.started") &&
            activity.tone != "error"
    ) {
        return true
    }
    // Adapters forward unknown wire-only SDK messages as runtime warnings; a row
    // carrying nothing displayable is not worth a line in the transcript.
    if (
        activity.kind == "runtime.warning" &&
            activity.summary.endsWith("(no displayable text content)")
    ) {
        return true
    }
    val payload = activity.payload as? JsonObject
    // `ExitPlanMode:` tool rows are the plan-mode boundary, already rendered as the
    // plan card.
    if (
        (activity.kind == "tool.updated" || activity.kind == "tool.completed") &&
            payload?.string("detail")?.startsWith("ExitPlanMode:") == true
    ) {
        return true
    }
    val isTaskRow =
        activity.kind == "task.started" ||
            activity.kind == "task.progress" ||
            activity.kind == "task.updated" ||
            activity.kind == "task.completed"
    // An agent spawn's `task.started` is the anchor row its progress ticks merge
    // into, unlike a background task's start, which its first progress row covers.
    if (
        isTaskRow &&
            payload?.string("taskId") != null &&
            payload.string("agentKind") == "agent"
    ) {
        return false
    }
    // A non-agent task start is bookkeeping: the progress row carries the label.
    if (activity.kind == "task.started") return true
    // Some providers settle agents through task.updated rather than task.completed.
    val terminalTaskRow =
        activity.kind == "task.completed" ||
            (activity.kind == "task.updated" &&
                payload?.string("status")?.let { status ->
                    status in TERMINAL_TASK_STATUSES ||
                        (payload.bool("timelineBypass") == true && status == "idle")
                } == true)
    if (activity.kind == "task.updated" && !terminalTaskRow) return true
    if (payload?.bool("timelineBypass") == true && !terminalTaskRow) return true
    // `agentId` marks ownership, not "hide me": only an agent's own background work
    // is internal, and its terminal row is what tells the user it finished.
    val ownedByAgent = payload?.string("agentId") != null
    if (!ownedByAgent) return false
    return !terminalTaskRow
}

private val TERMINAL_TASK_STATUSES = setOf("completed", "failed", "cancelled", "interrupted")

/** The subagent this activity belongs to, for identity-based collapsing. */
private fun taskIdOf(activity: ThreadActivityDto, payload: JsonObject?): String? {
    if (activity.kind != "task.started" &&
        activity.kind != "task.progress" &&
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
                summary = extractToolSummary(payload),
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
        // A task row's label is its own `summary`/`detail` payload field first,
        // falling back to the activity summary — the same choice RN's
        // `toDerivedWorkLogEntry` makes.
        // A main-thread progress tick is the reasoning row: RN gives it the
        // "thinking" work tone (`toDerivedWorkLogEntry`), which maps to the
        // collapsed Thinking card here. Agent-owned ticks stay subagent rows —
        // they describe spawned work, not the assistant's reasoning — and
        // `isHiddenActivity` already drops agent-internal ones.
        "task.progress" ->
            if (payload?.string("agentKind") == "agent") {
                subagentEntry(activity, payload, turnId, at)
            } else {
                FeedEntry.Reasoning(
                    id = activity.id,
                    text =
                        payload?.string("detail")
                            ?: payload?.string("summary")
                            ?: activity.summary,
                    turnId = turnId,
                    atMillis = at,
                )
            }

        "task.started",
        "task.completed",
        "task.updated" -> subagentEntry(activity, payload, turnId, at)

        // The folded user-input record: `foldUserInputActivities` synthesizes one
        // of these per request id from the request/resolve/answer triple.
        "user-input.answer-submitted" -> questionAnswerEntry(activity, payload, turnId, at)

        // Bare request/resolve rows that never folded (no requestId, or a payload
        // the fold could not assemble) still get their quiet history row.
        "user-input.requested",
        "user-input.resolved" ->
            FeedEntry.QuestionAnswer(
                id = activity.id,
                summary = activity.summary,
                preview = "",
                turnId = turnId,
                atMillis = at,
            )

        "runtime.warning" ->
            FeedEntry.Warning(
                activity.id,
                payload?.string("message") ?: activity.summary,
                turnId,
                at,
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

/**
 * A task lifecycle row for a subagent, extracted from [feedEntryFor] now that
 * `task.progress` splits by ownership.
 */
private fun subagentEntry(
    activity: ThreadActivityDto,
    payload: JsonObject?,
    turnId: String?,
    at: Long,
): FeedEntry =
    FeedEntry.Subagent(
        id = activity.id,
        name = payload?.string("title") ?: payload?.string("taskType") ?: "Subagent",
        task = payload?.string("summary") ?: payload?.string("detail") ?: activity.summary,
        active = activity.kind == "task.started" || activity.kind == "task.progress",
        turnId = turnId,
        atMillis = at,
    )

private fun extractToolSummary(payload: JsonObject?): String {
    if (payload == null) return ""
    val directCommand = payload.string("command")
    if (!directCommand.isNullOrBlank()) return directCommand
    val data = payload["data"] as? JsonObject
    val dataCommand = data?.string("command")
    if (!dataCommand.isNullOrBlank()) return dataCommand
    val item = data?.get("item") as? JsonObject
    val itemCommand = item?.string("command")
    if (!itemCommand.isNullOrBlank()) return itemCommand
    val itemInput = item?.get("input") as? JsonObject
    val itemInputCommand = itemInput?.string("command")
    if (!itemInputCommand.isNullOrBlank()) return itemInputCommand
    val itemResult = item?.get("result") as? JsonObject
    val itemResultCommand = itemResult?.string("command")
    if (!itemResultCommand.isNullOrBlank()) return itemResultCommand
    val rawOutput = data?.get("rawOutput") as? JsonObject
    val rawOutputCommand = rawOutput?.string("command")
    if (!rawOutputCommand.isNullOrBlank()) return rawOutputCommand
    return payload.string("itemType").orEmpty()
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
    val createdAtById = mutableMapOf<String, String>()
    // Request ids are unique. A terminal event stays final even when provider
    // sequences and server-generated activities arrive out of order, matching
    // `derivePendingRequests`.
    val closed = mutableSetOf<String>()
    sortedActivities.forEach { activity ->
        val payload = activity.payload as? JsonObject
        val requestId = payload?.string("requestId") ?: return@forEach
        when (activity.kind) {
            "approval.requested" -> {
                if (requestId in closed) return@forEach
                // The server replays these as approval-shaped activities, but they
                // are not user decisions and must not hold a gate open.
                val requestType = payload.string("requestType")
                if (requestType == "tool_user_input" || requestType == "auth_tokens_refresh") {
                    return@forEach
                }
                open[requestId] =
                    PendingApproval(
                        id = requestId,
                        title = activity.summary,
                        detail = payload.string("detail").orEmpty(),
                        command = payload.string("command"),
                        kind = approvalKindOf(payload),
                        appName = payload.string("appName"),
                        options = approvalOptionsOf(payload),
                    )
                createdAtById[requestId] = activity.createdAt.orEmpty()
            }
            "approval.resolved" -> {
                closed += requestId
                open.remove(requestId)
            }
            // A "stale request" failure means the provider already moved on, so
            // the gate must close or it blocks the composer forever.
            "provider.approval.respond.failed" ->
                if (isStaleRequestFailure(activity.kind, payload.string("detail"))) {
                    closed += requestId
                    open.remove(requestId)
                }
        }
    }
    return open.entries.minByOrNull { createdAtById[it.key].orEmpty() }?.value
}

/**
 * Maps `requestKind` onto the card's kind, falling back to the legacy
 * `requestType` vocabulary — `requestKindFromRequestType` in
 * `packages/client-runtime/src/pendingRequests.ts`.
 */
private fun approvalKindOf(payload: JsonObject): ApprovalKind =
    when (payload.string("requestKind")) {
        "file-read" -> ApprovalKind.FileRead
        "file-change" -> ApprovalKind.FileWrite
        "mcp-elicitation" -> ApprovalKind.McpElicitation
        "command" -> ApprovalKind.Command
        else ->
            when (payload.string("requestType")) {
                "file_read_approval" -> ApprovalKind.FileRead
                "file_change_approval",
                "apply_patch_approval" -> ApprovalKind.FileWrite
                "mcp_elicitation_approval" -> ApprovalKind.McpElicitation
                else -> ApprovalKind.Command
            }
    }

/**
 * Reads the decisions a provider attached to an `approval.requested` payload.
 * The strings pass through untouched: the reply echoes [ApprovalOption.decision]
 * and translating it here is how "always" used to reach OpenCode as a reject.
 */
private fun approvalOptionsOf(payload: JsonObject): List<ApprovalOption> =
    (payload["options"] as? JsonArray)
        ?.mapNotNull { raw ->
            val option = raw as? JsonObject ?: return@mapNotNull null
            val decision = option.string("decision") ?: return@mapNotNull null
            val label = option.string("label") ?: return@mapNotNull null
            ApprovalOption(decision = decision, label = label, warning = option.string("warning"))
        }
        .orEmpty()

/** Same replay for structured input requests, following `derivePendingRequests`. */
fun pendingUserInputOf(sortedActivities: List<ThreadActivityDto>): PendingUserInput? {
    val open = linkedMapOf<String, PendingUserInput>()
    val createdAtById = mutableMapOf<String, String>()
    val closed = mutableSetOf<String>()
    sortedActivities.forEach { activity ->
        val payload = activity.payload as? JsonObject
        val requestId = payload?.string("requestId") ?: return@forEach
        when (activity.kind) {
            "user-input.requested" -> {
                if (requestId in closed) return@forEach
                val request = userInputOf(payload) ?: return@forEach
                open[requestId] = request.copy(id = requestId)
                createdAtById[requestId] = activity.createdAt.orEmpty()
            }
            "user-input.resolved" -> {
                closed += requestId
                open.remove(requestId)
            }
            "provider.user-input.respond.failed" ->
                if (isStaleRequestFailure(activity.kind, payload.string("detail"))) {
                    closed += requestId
                    open.remove(requestId)
                }
        }
    }
    return open.entries.minByOrNull { createdAtById[it.key].orEmpty() }?.value
}

/**
 * Reads every structured question from a `user-input.requested` payload,
 * following `parseQuestions` in `packages/client-runtime/src/pendingRequests.ts`.
 * A malformed question (no `id`, `header`, or `question` text) or one that is
 * unanswerable (no options and custom answers disallowed) is dropped rather than
 * rendered as a card the user cannot satisfy.
 */
private fun userInputOf(payload: JsonObject): PendingUserInput? {
    val rawQuestions = payload["questions"] as? JsonArray ?: return null
    val questions =
        rawQuestions.mapNotNull { raw ->
            val question = raw as? JsonObject ?: return@mapNotNull null
            val id = question.string("id") ?: return@mapNotNull null
            val header = question.string("header") ?: return@mapNotNull null
            val prompt = question.string("question") ?: return@mapNotNull null
            val options =
                (question["options"] as? JsonArray)?.mapNotNull { rawOption ->
                    val option = rawOption as? JsonObject ?: return@mapNotNull null
                    val label = option.string("label") ?: return@mapNotNull null
                    UserInputOption(
                        label = label,
                        value = option.string("value"),
                        description = option.string("description"),
                    )
                } ?: return@mapNotNull null
            val allowCustomAnswer =
                (question["allowCustomAnswer"] as? JsonPrimitive)?.booleanOrNull != false
            if (options.isEmpty() && !allowCustomAnswer) return@mapNotNull null
            val multiSelect =
                (question["multiSelect"] as? JsonPrimitive)?.booleanOrNull == true
            UserInputQuestion(
                id = id,
                header = header,
                prompt = prompt,
                kind =
                    when {
                        options.isEmpty() -> UserInputKind.Text
                        multiSelect -> UserInputKind.MultiSelect
                        else -> UserInputKind.SingleSelect
                    },
                options = options,
                allowCustomAnswer = allowCustomAnswer,
            )
        }
    return questions.takeIf { it.isNotEmpty() }?.let {
        PendingUserInput(
            id = "",
            questions = it,
            // Async questions may be dismissed without a reply; native callback
            // questions cannot, because the provider is blocked on the answer.
            dismissible = payload.string("responseMode") == "message",
        )
    }
}

/**
 * The server reports a stale or unknown request through the failure text, and
 * the fragments differ per request family — a failed reply with any other text
 * stays open so the user can retry.
 */
private fun isStaleRequestFailure(kind: String, detail: String?): Boolean {
    val normalized = detail?.lowercase() ?: return false
    val fragments =
        when (kind) {
            "provider.approval.respond.failed" ->
                listOf(
                    "stale pending approval request",
                    "unknown pending approval request",
                    "unknown pending permission request",
                    "unknown pending codex approval request",
                )
            "provider.user-input.respond.failed" ->
                listOf(
                    "stale pending user-input request",
                    "unknown pending user-input request",
                    "unknown pending user input request",
                    "unknown pending codex user input request",
                )
            else -> return false
        }
    return fragments.any(normalized::contains)
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
