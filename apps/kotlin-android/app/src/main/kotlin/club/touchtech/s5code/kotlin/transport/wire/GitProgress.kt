package club.touchtech.s5code.kotlin.transport.wire

import kotlinx.serialization.Serializable

/** Forward-compatible projection of `GitActionProgressEvent`. */
@Serializable
data class GitActionProgressEventDto(
    val actionId: String = "",
    val cwd: String = "",
    val action: String = "",
    val kind: String = "",
    val phases: List<String> = emptyList(),
    val phase: String? = null,
    val label: String? = null,
    val hookName: String? = null,
    val stream: String? = null,
    val text: String? = null,
    val exitCode: Int? = null,
    val durationMs: Long? = null,
    val message: String? = null,
    val result: GitRunStackedActionResultDto? = null,
)

@Serializable
data class GitRunStackedActionResultDto(
    val action: String = "",
    val pr: GitPullRequestStepDto = GitPullRequestStepDto(),
    val toast: GitActionToastDto = GitActionToastDto(),
)

@Serializable
data class GitPullRequestStepDto(
    val status: String = "skipped_not_requested",
    val url: String? = null,
    val number: Int? = null,
    val title: String? = null,
)

@Serializable
data class GitActionToastDto(
    val title: String = "Source control action complete",
    val description: String? = null,
    val cta: GitActionToastCtaDto = GitActionToastCtaDto(),
)

@Serializable
data class GitActionToastCtaDto(
    val kind: String = "none",
    val label: String? = null,
    val url: String? = null,
)
