package club.touchtech.s5code.kotlin.transport.wire

import kotlinx.serialization.Serializable

/**
 * `ProjectCloneSnapshot` — one tracked clone's progress. The server keeps these
 * in memory only: finished clones drop after a grace period, failed ones stay
 * until retried or the project is removed.
 */
@Serializable
data class ProjectCloneSnapshotDto(
    val projectId: String = "",
    val remoteUrl: String = "",
    val destinationPath: String = "",
    val repository: SourceControlRepositoryDto? = null,
    /** running | done | failed | cancelled */
    val phase: String = "running",
    /** connecting | counting | receiving | resolving | checkout */
    val stage: String = "connecting",
    /** Percent of the current stage, parsed from git's progress lines. */
    val percent: Int? = null,
    /** Trailing text from the progress line (transfer size and rate). */
    val detail: String? = null,
    /** Human readable reason when phase is failed. */
    val error: String? = null,
)

/** `ProjectCloneActionResult` for `projectClone.cancel`/`retry`. */
@Serializable
data class ProjectCloneActionResultDto(val applied: Boolean = false)

/**
 * `ProjectCloneStartResult` — `start` returns once the project exists and the
 * clone is running; progress arrives on `subscribeProjectClones`.
 */
@Serializable
data class ProjectCloneStartResultDto(
    val projectId: String = "",
    val cwd: String = "",
    val remoteUrl: String = "",
    val repository: SourceControlRepositoryDto? = null,
)
