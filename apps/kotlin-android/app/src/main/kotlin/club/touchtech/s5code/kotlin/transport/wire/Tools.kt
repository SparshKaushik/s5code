package club.touchtech.s5code.kotlin.transport.wire

import club.touchtech.s5code.kotlin.transport.EnvironmentPlatformDto
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonPrimitive

/**
 * Wire DTOs for the non-orchestration RPCs the mobile client uses: git status
 * and refs, the project file index, review diffs, terminals, usage, and rewind.
 *
 * All of these are addressed by `cwd`, not by thread id. The thread's
 * `worktreePath` (or its project's `workspaceRoot` when it has no worktree) is
 * the directory a tool call operates in, which is why the projection keeps both
 * on every thread row.
 */

/* ── Server config ───────────────────────────────────────────────────── */

@Serializable
data class ProviderOptionChoiceDto(
    val id: String = "",
    val label: String = "",
    val description: String? = null,
    val isDefault: Boolean = false,
)

/**
 * One knob a model advertises, from `ProviderOptionDescriptor` in
 * `packages/contracts/src/model.ts`.
 *
 * Flattened across the select/boolean union for the same reason the stream items
 * are: the two variants differ only in `options` and the type of `currentValue`,
 * and `currentValue` is the one field that cannot share a slot, so it arrives as
 * a [JsonPrimitive] and is read per type.
 */
@Serializable
data class ProviderOptionDescriptorDto(
    val id: String = "",
    val label: String = "",
    val description: String? = null,
    /** select | boolean */
    val type: String = "select",
    val options: List<ProviderOptionChoiceDto> = emptyList(),
    val currentValue: JsonPrimitive? = null,
    val promptInjectedValues: List<String> = emptyList(),
)

@Serializable
data class ModelCapabilitiesDto(
    val optionDescriptors: List<ProviderOptionDescriptorDto> = emptyList(),
)

@Serializable
data class ServerProviderModelDto(
    val slug: String = "",
    val name: String = "",
    val shortName: String? = null,
    val isDefault: Boolean = false,
    val isLegacy: Boolean = false,
    /**
     * Null when the provider advertises no knobs for this model, which is a real
     * answer and not a missing field: OpenCode and Pi ship empty capabilities.
     */
    val capabilities: ModelCapabilitiesDto? = null,
)

@Serializable
data class ServerProviderSlashCommandDto(
    val name: String = "",
    val description: String? = null,
)

@Serializable
data class ServerProviderAuthDto(
    /** `authenticated` | `unauthenticated` | `unknown`. Unknown is not a refusal. */
    val status: String = "unknown",
    val type: String? = null,
    val label: String? = null,
    val email: String? = null,
)

/**
 * One configured provider instance. `instanceId` is the routing key the server
 * insists on; `driver` is only metadata. A client that routes on driver breaks
 * as soon as a user configures two instances of the same CLI.
 */
@Serializable
data class ServerProviderDto(
    val instanceId: String = "",
    val driver: String = "",
    val displayName: String? = null,
    val badgeLabel: String? = null,
    val enabled: Boolean = false,
    val installed: Boolean = false,
    val version: String? = null,
    val status: String = "unknown",
    val auth: ServerProviderAuthDto = ServerProviderAuthDto(),
    val availability: String? = null,
    val unavailableReason: String? = null,
    val showInteractionModeToggle: Boolean = false,
    val requiresNewThreadForModelChange: Boolean = false,
    val models: List<ServerProviderModelDto> = emptyList(),
    val slashCommands: List<ServerProviderSlashCommandDto> = emptyList(),
)

@Serializable
data class ServerConfigDto(
    val environment: ServerEnvironmentDto = ServerEnvironmentDto(),
    val cwd: String = "",
    val providers: List<ServerProviderDto> = emptyList(),
)

@Serializable
data class ServerEnvironmentDto(
    val environmentId: String = "",
    val label: String = "",
    val platform: EnvironmentPlatformDto = EnvironmentPlatformDto(),
    val serverVersion: String = "",
    val capabilities: ServerCapabilitiesDto = ServerCapabilitiesDto(),
)

@Serializable
data class ServerCapabilitiesDto(
    val connectionProbe: Boolean = false,
    val threadSettlement: Boolean = false,
    val threadSnooze: Boolean = false,
    val threadPinning: Boolean = false,
    val threadTitleRegeneration: Boolean = false,
    val pullRequests: Boolean = false,
)

/* ── Git / VCS ───────────────────────────────────────────────────────── */

@Serializable
data class VcsWorkingTreeFileDto(
    val path: String = "",
    val insertions: Int = 0,
    val deletions: Int = 0,
)

@Serializable
data class VcsWorkingTreeDto(
    val files: List<VcsWorkingTreeFileDto> = emptyList(),
    val insertions: Int = 0,
    val deletions: Int = 0,
)

@Serializable
data class VcsChangeRequestDto(
    val number: Int = 0,
    val title: String = "",
    val url: String = "",
    val baseRef: String = "",
    val headRef: String = "",
    /** open | draft | merged | closed */
    val state: String = "open",
)

/**
 * `vcs.refreshStatus` merges the local and remote halves the subscription sends
 * separately. Remote fields default rather than being nullable as a group,
 * because a repo with no upstream is normal, not an error.
 */
@Serializable
data class VcsStatusDto(
    val isRepo: Boolean = false,
    val hasPrimaryRemote: Boolean = false,
    val isDefaultRef: Boolean = false,
    val refName: String? = null,
    val hasWorkingTreeChanges: Boolean = false,
    val workingTree: VcsWorkingTreeDto = VcsWorkingTreeDto(),
    val hasUpstream: Boolean = false,
    val aheadCount: Int = 0,
    val behindCount: Int = 0,
    val aheadOfDefaultCount: Int = 0,
    val pr: VcsChangeRequestDto? = null,
)

@Serializable
data class VcsRefDto(
    val name: String = "",
    val isRemote: Boolean = false,
    val remoteName: String? = null,
    val current: Boolean = false,
    val isDefault: Boolean = false,
    val worktreePath: String? = null,
)

@Serializable
data class VcsListRefsResultDto(
    val refs: List<VcsRefDto> = emptyList(),
    val isRepo: Boolean = false,
    val hasPrimaryRemote: Boolean = false,
    val nextCursor: Int? = null,
    val totalCount: Int = 0,
)

/* ── Project file index ──────────────────────────────────────────────── */

@Serializable
data class ProjectEntryDto(
    val path: String = "",
    /** file | directory */
    val kind: String = "file",
)

@Serializable
data class ProjectListEntriesResultDto(
    val entries: List<ProjectEntryDto> = emptyList(),
    val truncated: Boolean = false,
)

@Serializable
data class ProjectReadFileResultDto(
    val relativePath: String = "",
    val contents: String = "",
    val byteLength: Long = 0,
    val truncated: Boolean = false,
)

/* ── Review ──────────────────────────────────────────────────────────── */

/**
 * A unified diff as text plus its provenance. The server deliberately does not
 * pre-parse it; the client parses once and caches by `diffHash`.
 */
@Serializable
data class ReviewDiffSourceDto(
    val id: String = "",
    /** working-tree | branch-range */
    val kind: String = "working-tree",
    val title: String = "",
    val baseRef: String? = null,
    val headRef: String? = null,
    val diff: String = "",
    val diffHash: String = "",
    val truncated: Boolean = false,
)

@Serializable
data class ReviewDiffPreviewResultDto(
    val cwd: String = "",
    val sources: List<ReviewDiffSourceDto> = emptyList(),
)

/* ── Terminal ────────────────────────────────────────────────────────── */

@Serializable
data class TerminalSnapshotDto(
    val threadId: String = "",
    val terminalId: String = "",
    val cwd: String = "",
    val worktreePath: String? = null,
    /** starting | running | exited | error */
    val status: String = "starting",
    val pid: Int? = null,
    val history: String = "",
    val exitCode: Int? = null,
    val exitSignal: Int? = null,
    val label: String = "",
    val updatedAt: String? = null,
    val sequence: Long? = null,
)

/**
 * One frame of `terminal.attach`. Flattened for the same reason as the shell
 * stream item: the union keys on `type` with per-variant fields.
 */
@Serializable
data class TerminalStreamEventDto(
    val type: String,
    val threadId: String = "",
    val terminalId: String = "",
    val sequence: Long? = null,
    val snapshot: TerminalSnapshotDto? = null,
    val data: String? = null,
    val exitCode: Int? = null,
    val exitSignal: Int? = null,
    val message: String? = null,
    val hasRunningSubprocess: Boolean = false,
    val label: String? = null,
)

/* ── Usage ───────────────────────────────────────────────────────────── */

@Serializable
data class UsageTokenTotalsDto(
    val uncachedInputTokens: Long = 0,
    val cachedInputTokens: Long = 0,
    val cacheCreationTokens: Long = 0,
    val outputTokens: Long = 0,
    val reasoningTokens: Long = 0,
)

@Serializable
data class UsageBucketDto(
    val day: String = "",
    /**
     * UTC start of a rolling hourly bucket, present only when the request asked
     * for hourly resolution.
     */
    val hourStart: String? = null,
    /** claude | codex | cursor | pi */
    val provider: String = "",
    val model: String = "",
    val apiProvider: String = "",
    val totals: UsageTokenTotalsDto = UsageTokenTotalsDto(),
    val costUsd: Double = 0.0,
    val records: Long = 0,
    val unpricedRecords: Long = 0,
    val sessions: Long = 0,
)

@Serializable
data class UsagePricingDto(
    /** fresh | cached | unavailable */
    val status: String = "unavailable",
    val source: String = "",
    val knownModels: Int = 0,
)

@Serializable
data class UsageSummaryDto(
    val contractVersion: Int = 0,
    val timeZone: String = "UTC",
    val sinceDay: String = "",
    val untilDay: String = "",
    val buckets: List<UsageBucketDto> = emptyList(),
    val pricing: UsagePricingDto = UsagePricingDto(),
)

/* ── Rewind ──────────────────────────────────────────────────────────── */

/** One captured turn in a thread's rewind history. */
@Serializable
data class RewindEntryDto(
    val turnId: String = "",
    val sequence: Int = 0,
    val userMessageId: String? = null,
    val assistantMessageId: String? = null,
    val prompt: String = "",
    val files: List<String> = emptyList(),
    /** applied | undone */
    val state: String = "applied",
    val createdAt: String? = null,
)

/**
 * `available` false means the controls are hidden entirely, not disabled: the
 * thread has no workspace or the snapshot store could not be created, so there
 * is nothing to offer.
 */
@Serializable
data class RewindStatusDto(
    val threadId: String = "",
    val available: Boolean = false,
    val undo: RewindEntryDto? = null,
    val redo: RewindEntryDto? = null,
    val appliedCount: Int = 0,
    val undoneCount: Int = 0,
)

/* ── Source control ──────────────────────────────────────────────────── */

/**
 * `sourceControl.lookupRepository` resolves one repository reference. The server
 * has no repository *search* RPC, so the add-project flow validates a typed
 * reference rather than offering a browsable list.
 */
@Serializable
data class SourceControlRepositoryDto(
    val provider: String = "",
    val nameWithOwner: String = "",
    val url: String = "",
    val sshUrl: String = "",
)

/** `sourceControl.cloneRepository` result. */
@Serializable
data class SourceControlCloneResultDto(val cwd: String = "", val remoteUrl: String = "")

/* ── Filesystem browse ───────────────────────────────────────────────── */

@Serializable
data class FilesystemBrowseEntryDto(val name: String = "", val fullPath: String = "")

@Serializable
data class FilesystemBrowseResultDto(
    val parentPath: String = "",
    val entries: List<FilesystemBrowseEntryDto> = emptyList(),
)

/* ── Assets ──────────────────────────────────────────────────────────── */

/**
 * `assets.createUrl` result. The URL is relative to the environment's HTTP origin
 * and carries a signed, expiring token in its path, which is what lets an image
 * be fetched with a plain GET instead of an authenticated RPC.
 */
@Serializable
data class AssetUrlResultDto(
    val relativeUrl: String = "",
    val expiresAt: Long = 0,
    val sourcePath: String? = null,
)
