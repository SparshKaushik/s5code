package club.touchtech.s5code.kotlin.transport.wire

import kotlinx.serialization.Serializable

/**
 * `WorktreeSetupSnapshot` from `packages/contracts/src/worktreeSetup.ts`: the
 * live progress of a bootstrap worktree creation. The server keeps it in memory
 * only while the thread's first turn is being prepared, so `subscribeWorktreeSetup`
 * streams null until tracking starts and again once it is dropped; the settled
 * outcome is re-recorded as a `worktree-setup` thread activity, which is what a
 * reload or second client renders from.
 */
@Serializable
data class WorktreeSetupStageDto(
    val id: String = "",
    val status: String = "pending",
    val startedAt: String? = null,
    val endedAt: String? = null,
    val percent: Int? = null,
    val detail: String? = null,
    val tail: List<String> = emptyList(),
)

/** Display name, command, and owning terminal of the setup script from t3.json. */
@Serializable
data class WorktreeSetupScriptDto(
    val name: String = "",
    val command: String = "",
    val terminalId: String = "",
)

@Serializable
data class WorktreeSetupSnapshotDto(
    val threadId: String = "",
    /** running | done | failed | cancelled */
    val phase: String = "running",
    val startedAt: String? = null,
    val endedAt: String? = null,
    val branch: String? = null,
    val baseRef: String? = null,
    val worktreePath: String? = null,
    val setupScript: WorktreeSetupScriptDto? = null,
    val stages: List<WorktreeSetupStageDto> = emptyList(),
    val error: String? = null,
    val sequence: Int = 0,
)
