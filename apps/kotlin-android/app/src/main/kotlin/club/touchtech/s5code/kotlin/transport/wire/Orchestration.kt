package club.touchtech.s5code.kotlin.transport.wire

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

/**
 * Wire DTOs for the orchestration contracts in
 * `packages/contracts/src/orchestration.ts`.
 *
 * Two rules keep these decodable against a moving server:
 *
 * - Every field the server marked optional is optional here with a default, and
 *   every union the contract calls forward-compatible is decoded as a string
 *   rather than an enum. A client that fails a whole snapshot over one unknown
 *   literal takes its own connection down over data it did not need.
 * - Activity payloads stay [JsonElement]. Their shape is provider-defined and
 *   grows per adapter; the presentation layer reads the handful of keys it knows
 *   and ignores the rest, which is what the RN client does too.
 */

@Serializable
data class ModelSelectionDto(
    val instanceId: String = "",
    val model: String = "",
    val options: JsonElement? = null,
)

@Serializable
data class LatestTurnDto(
    val turnId: String,
    /** running | interrupted | completed | error */
    val state: String,
    val requestedAt: String? = null,
    val startedAt: String? = null,
    val completedAt: String? = null,
    val assistantMessageId: String? = null,
)

@Serializable
data class SessionDto(
    val threadId: String = "",
    /** idle | starting | running | ready | interrupted | stopped | error */
    val status: String = "idle",
    val providerName: String? = null,
    val providerInstanceId: String? = null,
    val runtimeMode: String = "full-access",
    val activeTurnId: String? = null,
    val lastError: String? = null,
    val updatedAt: String? = null,
)

@Serializable
data class PlanProgressDto(
    val step: String = "",
    val completedSteps: Int = 0,
    val totalSteps: Int = 0,
)

@Serializable
data class TitleRegenerationDto(val requestId: String = "", val startedAt: String? = null)

@Serializable
data class RepositoryIdentityDto(
    val canonicalKey: String = "",
    val displayName: String? = null,
    val owner: String? = null,
    val name: String? = null,
    val rootPath: String? = null,
)

@Serializable
data class ProjectShellDto(
    val id: String,
    val title: String = "",
    val workspaceRoot: String = "",
    val repositoryIdentity: RepositoryIdentityDto? = null,
    val defaultModelSelection: ModelSelectionDto? = null,
    /**
     * The project's own icon, relative to its workspace root, when the server
     * found one. A cache-key hint only: `assets.createUrl` re-reads the
     * authoritative path before signing, so a stale value costs a URL, not
     * correctness.
     */
    val faviconPath: String? = null,
    val createdAt: String? = null,
    val updatedAt: String? = null,
)

/**
 * The home list's row model. Note the derived flags (`hasPendingApprovals`,
 * `hasPendingUserInput`, `planProgress`, `backgroundLiveness`): the server
 * computes them so a client can render an accurate row without subscribing to
 * every thread's full activity history.
 */
@Serializable
data class ThreadShellDto(
    val id: String,
    val projectId: String = "",
    val title: String = "",
    val modelSelection: ModelSelectionDto = ModelSelectionDto(),
    val runtimeMode: String = "full-access",
    val interactionMode: String = "default",
    val branch: String? = null,
    val worktreePath: String? = null,
    val latestTurn: LatestTurnDto? = null,
    val createdAt: String? = null,
    val updatedAt: String? = null,
    val archivedAt: String? = null,
    val settledOverride: String? = null,
    val settledAt: String? = null,
    val snoozedUntil: String? = null,
    val snoozedAt: String? = null,
    val pinnedAt: String? = null,
    val pinOrderKey: String? = null,
    val titleRegeneration: TitleRegenerationDto? = null,
    val session: SessionDto? = null,
    val latestUserMessageAt: String? = null,
    val hasPendingApprovals: Boolean = false,
    val hasPendingUserInput: Boolean = false,
    val hasActionableProposedPlan: Boolean = false,
    /** working | monitoring, or absent for none. */
    val backgroundLiveness: String? = null,
    val planProgress: PlanProgressDto? = null,
)

@Serializable
data class ShellSnapshotDto(
    val snapshotSequence: Long = 0,
    val projects: List<ProjectShellDto> = emptyList(),
    val threads: List<ThreadShellDto> = emptyList(),
    val updatedAt: String? = null,
)

/**
 * One frame of `orchestration.subscribeShell`. The union is flattened into one
 * nullable-field class rather than a sealed hierarchy: kotlinx polymorphism
 * needs a discriminator it can register, and this union keys on `kind` with
 * per-variant field names, which a custom serializer would have to reimplement
 * for no gain over reading `kind` directly.
 */
@Serializable
data class ShellStreamItemDto(
    val kind: String,
    val snapshot: ShellSnapshotDto? = null,
    val sequence: Long? = null,
    val project: ProjectShellDto? = null,
    val projectId: String? = null,
    val thread: ThreadShellDto? = null,
    val threadId: String? = null,
)

@Serializable
data class ChatAttachmentDto(
    val type: String = "image",
    val id: String = "",
    val name: String = "",
    val mimeType: String = "",
    val sizeBytes: Long = 0,
)

@Serializable
data class MessageDto(
    val id: String,
    /** user | assistant | system */
    val role: String = "assistant",
    val text: String = "",
    val attachments: List<ChatAttachmentDto>? = null,
    val turnId: String? = null,
    val streaming: Boolean = false,
    val createdAt: String? = null,
    val updatedAt: String? = null,
)

@Serializable
data class ProposedPlanDto(
    val id: String,
    val turnId: String? = null,
    val planMarkdown: String = "",
    val implementedAt: String? = null,
    val implementationThreadId: String? = null,
    val createdAt: String? = null,
    val updatedAt: String? = null,
)

/**
 * A single provider-shaped event in the thread's history: tool calls, subagent
 * tasks, approvals, user-input requests, reasoning, plan updates. `kind` is an
 * open string and `payload` is opaque, exactly as the contract has it.
 */
@Serializable
data class ThreadActivityDto(
    val id: String,
    /** info | tool | approval | error */
    val tone: String = "info",
    val kind: String = "",
    val summary: String = "",
    val payload: JsonElement? = null,
    val turnId: String? = null,
    val sequence: Long? = null,
    val createdAt: String? = null,
)

@Serializable
data class CheckpointFileDto(
    val path: String = "",
    val kind: String = "",
    val additions: Int = 0,
    val deletions: Int = 0,
)

@Serializable
data class CheckpointSummaryDto(
    val turnId: String = "",
    val checkpointTurnCount: Int = 0,
    val checkpointRef: String = "",
    /** ready | missing | error */
    val status: String = "ready",
    val files: List<CheckpointFileDto> = emptyList(),
    val assistantMessageId: String? = null,
    val completedAt: String? = null,
)

@Serializable
data class ThreadDto(
    val id: String,
    val projectId: String = "",
    val title: String = "",
    val modelSelection: ModelSelectionDto = ModelSelectionDto(),
    val runtimeMode: String = "full-access",
    val interactionMode: String = "default",
    val branch: String? = null,
    val worktreePath: String? = null,
    val latestTurn: LatestTurnDto? = null,
    val createdAt: String? = null,
    val updatedAt: String? = null,
    val archivedAt: String? = null,
    val settledOverride: String? = null,
    val settledAt: String? = null,
    val snoozedUntil: String? = null,
    val snoozedAt: String? = null,
    val pinnedAt: String? = null,
    val pinOrderKey: String? = null,
    val titleRegeneration: TitleRegenerationDto? = null,
    val deletedAt: String? = null,
    val messages: List<MessageDto> = emptyList(),
    val proposedPlans: List<ProposedPlanDto> = emptyList(),
    val activities: List<ThreadActivityDto> = emptyList(),
    val checkpoints: List<CheckpointSummaryDto> = emptyList(),
    val session: SessionDto? = null,
)

@Serializable
data class ThreadDetailPageDto(
    val beforeCursor: String? = null,
    val hasMore: Boolean = false,
    val snapshotSequence: Long = 0,
    val threadSequence: Long? = null,
)

@Serializable
data class ThreadDetailSnapshotDto(
    val snapshotSequence: Long = 0,
    val thread: ThreadDto,
    val page: ThreadDetailPageDto? = null,
)

/**
 * One frame of `orchestration.subscribeThread`. Events arrive as raw JSON and are
 * applied by [club.touchtech.s5code.kotlin.transport.applyThreadEvent], which
 * only understands the event types that change what a screen renders.
 */
@Serializable
data class ThreadStreamItemDto(
    val kind: String,
    val snapshot: ThreadDetailSnapshotDto? = null,
    val event: JsonElement? = null,
)

/** One message-body hit from `orchestration.searchThreads`. */
@Serializable
data class ThreadSearchMatchDto(
    val threadId: String,
    val projectId: String,
    /** user | assistant */
    val source: String,
    val snippet: String = "",
    val messageCreatedAt: String? = null,
)

@Serializable
data class SearchThreadsResultDto(val matches: List<ThreadSearchMatchDto> = emptyList())

/** `dispatchCommand`'s ack: the event-log sequence the command produced. */
@Serializable data class DispatchResultDto(val sequence: Long = 0)
