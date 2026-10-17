package club.touchtech.s5code.kotlin.transport.wire

import kotlinx.serialization.Serializable

/**
 * `PullRequestListEntry`, decoded to the fields the composer mention menu
 * needs. The full row carries stats, actors and mergeability the `#` search
 * never reads.
 */
@Serializable
data class PullRequestListEntryDto(
    val projectId: String = "",
    val repository: String = "",
    val number: Int = 0,
    val title: String = "",
    val url: String = "",
    val headBranch: String = "",
    val baseBranch: String = "",
    /** `open` | `closed` | `merged`. */
    val state: String = "open",
    val isDraft: Boolean = false,
    val updatedAt: String = "",
)

@Serializable
data class PullRequestListResultDto(
    val entries: List<PullRequestListEntryDto> = emptyList(),
)

/**
 * `PullRequestDetail`, decoded to the same composer fields as the list entry.
 * Numeric `#` queries resolve through this when the listing page does not hold
 * the exact number. `headSha` joins them for detail reads: hosts report it with
 * the detail, and watch UIs compare it against a watch's recorded head.
 */
@Serializable
data class PullRequestDetailDto(
    val projectId: String = "",
    val repository: String = "",
    val number: Int = 0,
    val title: String = "",
    val url: String = "",
    val headBranch: String = "",
    val baseBranch: String = "",
    val state: String = "open",
    val isDraft: Boolean = false,
    val updatedAt: String = "",
    /** The head commit, where the host reports it with the detail. */
    val headSha: String? = null,
)
