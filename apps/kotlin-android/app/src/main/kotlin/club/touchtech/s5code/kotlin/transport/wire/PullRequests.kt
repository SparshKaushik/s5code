package club.touchtech.s5code.kotlin.transport.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

/*
 * `packages/contracts/src/pullRequest.ts`. Every field defaults so a newer
 * server's extra or missing optional field never fails a whole page; literals
 * stay strings, matched where a screen reads them.
 */

/** `PullRequestActor`. */
@Serializable
data class PullRequestActorDto(
    val isBot: Boolean? = null,
    val login: String = "",
    val name: String? = null,
    val avatarUrl: String? = null,
)

/** `PullRequestLabel`; [color] is the host's hex without `#`, where it has one. */
@Serializable
data class PullRequestLabelDto(val name: String = "", val color: String? = null)

/** `PullRequestCheck`. [status]: pending | action-required | success | failure | skipped | neutral | cancelled. */
@Serializable
data class PullRequestCheckDto(
    val name: String = "",
    val status: String = "pending",
    val description: String? = null,
    val url: String? = null,
    val required: Boolean? = null,
)

/** `PullRequestReaction`. */
@Serializable
data class PullRequestReactionDto(
    val content: String = "",
    val count: Int = 0,
    val actors: List<String> = emptyList(),
    val viewerHasReacted: Boolean = false,
)

/** `PullRequestComment`. [kind]: issue-comment | review-comment | review. */
@Serializable
data class PullRequestCommentDto(
    val id: String = "",
    val kind: String = "issue-comment",
    val author: PullRequestActorDto? = null,
    val body: String = "",
    val createdAt: String = "",
    val editedAt: String? = null,
    val url: String? = null,
    val path: String? = null,
    val reviewState: String? = null,
    val reactions: List<PullRequestReactionDto>? = null,
    val canEdit: Boolean? = null,
)

/** `PullRequestThreadComment`. */
@Serializable
data class PullRequestThreadCommentDto(
    val id: String = "",
    val author: PullRequestActorDto? = null,
    val body: String = "",
    val createdAt: String = "",
    val editedAt: String? = null,
    val url: String? = null,
    val reactions: List<PullRequestReactionDto>? = null,
    val canEdit: Boolean? = null,
)

/** `PullRequestReviewThread`. [side]: left | right. */
@Serializable
data class PullRequestReviewThreadDto(
    val id: String = "",
    val path: String = "",
    val line: Int? = null,
    val side: String = "right",
    val isResolved: Boolean = false,
    val isOutdated: Boolean = false,
    val comments: List<PullRequestThreadCommentDto> = emptyList(),
    val commentCount: Int? = null,
    val nextCommentsCursor: String? = null,
    val canResolve: Boolean? = null,
)

/** `PullRequestCommit`. */
@Serializable
data class PullRequestCommitDto(
    val oid: String = "",
    val messageHeadline: String = "",
    val committedDate: String = "",
    val additions: Int? = null,
    val deletions: Int? = null,
    val authors: List<PullRequestActorDto>? = null,
)

@Serializable
data class PullRequestReviewCapabilitiesDto(
    val inlineComment: Boolean = false,
    val reply: Boolean = false,
    val resolve: Boolean = false,
    val verdicts: List<String> = emptyList(),
)

@Serializable
data class PullRequestEditCapabilitiesDto(
    val changeRequest: Boolean = false,
    val comment: Boolean = false,
    val maintainerEdits: Boolean? = null,
)

@Serializable
data class PullRequestReviewerCapabilitiesDto(
    val request: Boolean = false,
    val listCandidates: Boolean = false,
)

/** `PullRequestCapabilities`: what the host can do at all, before viewer permissions. */
@Serializable
data class PullRequestCapabilitiesDto(
    val diff: Boolean = false,
    val comment: Boolean = false,
    val actions: List<String> = emptyList(),
    val mergeMethods: List<String> = emptyList(),
    val deleteBranch: Boolean? = null,
    val updateMethods: List<String>? = null,
    val search: Boolean = false,
    val reactions: Boolean? = null,
    /** host | environment — where viewed-file marks live. */
    val viewedFiles: String? = null,
    val review: PullRequestReviewCapabilitiesDto = PullRequestReviewCapabilitiesDto(),
    val reviewers: PullRequestReviewerCapabilitiesDto = PullRequestReviewerCapabilitiesDto(),
    val edit: PullRequestEditCapabilitiesDto? = null,
    val stacks: Boolean? = null,
    val stackActions: Boolean? = null,
    val labels: Boolean? = null,
)

/** `PullRequestViewerPermissions`: an unreported permission is granted, not withheld. */
@Serializable
data class PullRequestViewerPermissionsDto(
    val stackRebase: Boolean? = null,
    val actions: List<String> = emptyList(),
    val comment: Boolean = false,
    val resolve: Boolean = false,
    val verdicts: List<String> = emptyList(),
    val requestReviewers: Boolean = false,
    val updateMethods: List<String>? = null,
    val labels: Boolean? = null,
    val editChangeRequest: Boolean? = null,
    val maintainerEdits: Boolean? = null,
)

@Serializable
data class PullRequestMergeCapabilitiesDto(
    val merge: Boolean = false,
    val squash: Boolean = false,
    val rebase: Boolean = false,
)

/** `PullRequestStackMembership`: where a list row sits in its host stack. */
@Serializable
data class PullRequestStackMembershipDto(
    val number: Int = 0,
    val position: Int = 0,
    val size: Int = 0,
    val base: String = "",
)

/**
 * `PullRequestListEntry`. The composer `#` search reads the identity fields;
 * the Pull requests tab reads the rest.
 */
@Serializable
data class PullRequestListEntryDto(
    val stack: PullRequestStackMembershipDto? = null,
    val provider: String = "github",
    val host: String = "",
    val projectId: String = "",
    val projectTitle: String = "",
    val repository: String = "",
    val number: Int = 0,
    val title: String = "",
    val url: String = "",
    val author: PullRequestActorDto? = null,
    val headBranch: String = "",
    val baseBranch: String = "",
    /** `open` | `closed` | `merged`. */
    val state: String = "open",
    val isDraft: Boolean = false,
    /** mergeable | conflicting | unknown. */
    val mergeability: String = "unknown",
    /** Zero until `pullRequests.listStats` fills it in on GitHub. */
    val additions: Int = 0,
    val deletions: Int = 0,
    val createdAt: String = "",
    val updatedAt: String = "",
    val observedAt: Double? = null,
    val viewerReviewRequested: Boolean = false,
    val labels: List<PullRequestLabelDto> = emptyList(),
    /** approved | changes-requested | review-required. */
    val reviewDecision: String? = null,
    /** passing | failing | pending. */
    val checksState: String? = null,
)

/** `PullRequestProviderSummary`. */
@Serializable
data class PullRequestProviderSummaryDto(
    val host: String = "",
    val kind: String = "github",
    /** False means the host cannot search, so the client filters text itself. */
    val searchesOnHost: Boolean = true,
    val actions: List<String>? = null,
    val projectCount: Int = 0,
    val configured: Boolean = true,
    val detail: String? = null,
)

/** `PullRequestListProjectError`. */
@Serializable
data class PullRequestListProjectErrorDto(
    val projectId: String = "",
    val projectTitle: String = "",
    val message: String = "",
)

/** `PullRequestListResult`. [viewers] maps host → signed-in login. */
@Serializable
data class PullRequestListResultDto(
    val viewers: Map<String, String> = emptyMap(),
    val providers: List<PullRequestProviderSummaryDto> = emptyList(),
    val entries: List<PullRequestListEntryDto> = emptyList(),
    val errors: List<PullRequestListProjectErrorDto> = emptyList(),
    val truncated: Boolean = false,
    val nextCursors: Map<String, String> = emptyMap(),
)

/**
 * `PullRequestDetail`. The composer `#` search reads the identity fields;
 * the PR page reads everything.
 */
@Serializable
data class PullRequestDetailDto(
    val provider: String = "github",
    val capabilities: PullRequestCapabilitiesDto = PullRequestCapabilitiesDto(),
    val viewerPermissions: PullRequestViewerPermissionsDto = PullRequestViewerPermissionsDto(),
    val projectId: String = "",
    val projectTitle: String = "",
    val workspaceRoot: String = "",
    val repository: String = "",
    val number: Int = 0,
    val title: String = "",
    val body: String = "",
    val url: String = "",
    val author: PullRequestActorDto? = null,
    val state: String = "open",
    val isDraft: Boolean = false,
    val mergeability: String = "unknown",
    val additions: Int = 0,
    val deletions: Int = 0,
    val changedFiles: Int = 0,
    val headBranch: String = "",
    val headRepositoryNameWithOwner: String? = null,
    /** The head commit, where the host reports it with the detail. */
    val headSha: String? = null,
    val baseBranch: String = "",
    val createdAt: String = "",
    val updatedAt: String = "",
    val observedAt: Double? = null,
    val mergedAt: String? = null,
    val closedAt: String? = null,
    val reviewers: List<PullRequestActorDto> = emptyList(),
    val labels: List<PullRequestLabelDto> = emptyList(),
    val checks: List<PullRequestCheckDto> = emptyList(),
    val mergeCapabilities: PullRequestMergeCapabilitiesDto = PullRequestMergeCapabilitiesDto(),
    val viewer: String? = null,
    /** up-to-date | behind | unknown. */
    val baseComparison: String? = null,
    val behindBy: Int? = null,
    val autoMergeEnabled: Boolean? = null,
    val autoMergeMethod: String? = null,
    val workflowApprovalsRequired: Int? = null,
    val maintainerCanModify: Boolean? = null,
)

/** `PullRequestActivity`: the timeline a PR page's Activity segment draws. */
@Serializable
data class PullRequestActivityDto(
    val author: PullRequestActorDto? = null,
    val reviewers: List<PullRequestActorDto>? = null,
    val comments: List<PullRequestCommentDto> = emptyList(),
    val commentCount: Int = 0,
    val commentsTruncated: Boolean = false,
    val reviewThreadsTruncated: Boolean? = null,
    val reviewThreads: List<PullRequestReviewThreadDto> = emptyList(),
    val commits: List<PullRequestCommitDto> = emptyList(),
    val reactions: List<PullRequestReactionDto>? = null,
)

@Serializable
data class PullRequestStackLayerDto(
    val number: Int = 0,
    val title: String? = null,
    val isDraft: Boolean? = null,
    val headSha: String? = null,
    val headBranch: String = "",
    val state: String = "open",
)

/** `PullRequestStack`; the RPC answers null for a PR in no stack. Layers run bottom to top. */
@Serializable
data class PullRequestStackDto(
    val id: String = "",
    val number: Int = 0,
    val url: String = "",
    val base: String = "",
    val layers: List<PullRequestStackLayerDto> = emptyList(),
)

/** `PullRequestDiffStat`. */
@Serializable
data class PullRequestDiffStatDto(
    val projectId: String = "",
    val repository: String = "",
    val number: Int = 0,
    val additions: Int = 0,
    val deletions: Int = 0,
)

/** `PullRequestListStatsResult`. */
@Serializable
data class PullRequestListStatsResultDto(val stats: List<PullRequestDiffStatDto> = emptyList())

@Serializable
data class PullRequestLinkedThreadDto(
    val id: String = "",
    val projectId: String = "",
    val title: String = "",
    val archivedAt: String? = null,
)

/** `PullRequestLinkedThreadsResult`. */
@Serializable
data class PullRequestLinkedThreadsResultDto(
    val threads: List<PullRequestLinkedThreadDto> = emptyList(),
)

@Serializable
data class PullRequestOmittedFileStatDto(
    val path: String = "",
    val additions: Int = 0,
    val deletions: Int = 0,
)

/** `PullRequestDiffResult`, one slice of a paged diff (`POST /api/pull-requests/diff`). */
@Serializable
data class PullRequestDiffResultDto(
    val patch: String = "",
    val truncated: Boolean = false,
    val nextCursor: String? = null,
    val omittedFileStats: List<PullRequestOmittedFileStatDto>? = null,
)

/** `PullRequestFileViewed`. [state]: unviewed | viewed | dismissed. */
@Serializable
data class PullRequestFileViewedDto(val path: String = "", val state: String = "unviewed")

/** `PullRequestFilesViewedResult`; a file missing from [files] is unviewed. */
@Serializable
data class PullRequestFilesViewedResultDto(
    val files: List<PullRequestFileViewedDto> = emptyList(),
    val truncated: Boolean = false,
)

/** `PullRequestLineCount`. */
@Serializable
data class PullRequestLineCountDto(val additions: Int = 0, val deletions: Int = 0)

/**
 * `PullRequestChangeBreakdown`: line counts split by what the changed files
 * are. Servers from before the docs split send no [docs]; theirs sit in [code].
 */
@Serializable
data class PullRequestChangeBreakdownDto(
    val code: PullRequestLineCountDto = PullRequestLineCountDto(),
    val tests: PullRequestLineCountDto = PullRequestLineCountDto(),
    val mocks: PullRequestLineCountDto = PullRequestLineCountDto(),
    val generated: PullRequestLineCountDto = PullRequestLineCountDto(),
    val docs: PullRequestLineCountDto = PullRequestLineCountDto(),
)

/** `PullRequestStackHead`: a layer's revision a stack merge expects to land. */
@Serializable
data class PullRequestStackHeadDto(val number: Int = 0, val headSha: String = "")

/** `PullRequestThreadCommentsResult`: one more page of a review thread's replies. */
@Serializable
data class PullRequestThreadCommentsResultDto(
    val comments: List<PullRequestThreadCommentDto> = emptyList(),
    val nextCursor: String? = null,
)

/** `GitPreparePullRequestThreadResult`; only the checkout fields are read. */
@Serializable
data class GitPreparePullRequestThreadResultDto(
    val branch: String = "",
    val worktreePath: String? = null,
    val isOnPullRequestHead: Boolean = true,
)

/** `PullRequestChecks`: the `pullRequests.checks` answer, a head's checks without the rest of the detail. */
@Serializable
data class PullRequestChecksDto(
    val observedAt: Double? = null,
    val state: String = "open",
    val checks: List<PullRequestCheckDto> = emptyList(),
)

/** `PullRequestLabelCandidate`: a repository label and whether this PR carries it. */
@Serializable
data class PullRequestLabelCandidateDto(
    val name: String = "",
    val color: String? = null,
    val description: String? = null,
    val isApplied: Boolean = false,
)

/** `PullRequestLabelCandidateList`. */
@Serializable
data class PullRequestLabelCandidateListDto(
    val candidates: List<PullRequestLabelCandidateDto> = emptyList(),
    val truncated: Boolean = false,
)

/** `PullRequestFilterOptions`: label and author values the host found for a list filter search. */
@Serializable
data class PullRequestFilterOptionsDto(
    val labels: List<PullRequestLabelDto> = emptyList(),
    val authors: List<PullRequestActorDto> = emptyList(),
    val truncated: Boolean = false,
)

/** `PullRequestReviewerCandidate`: someone who can be asked to review. [kind]: user | team. */
@Serializable
data class PullRequestReviewerCandidateDto(
    val isBot: Boolean? = null,
    val login: String = "",
    val name: String? = null,
    val avatarUrl: String? = null,
    val id: String = "",
    val kind: String = "user",
    val isRequested: Boolean = false,
) {
    val actor: PullRequestActorDto get() = PullRequestActorDto(login = login, name = name, avatarUrl = avatarUrl)
}

/** `PullRequestReviewerCandidateList`. */
@Serializable
data class PullRequestReviewerCandidateListDto(
    val candidates: List<PullRequestReviewerCandidateDto> = emptyList(),
    val truncated: Boolean = false,
)

/* `packages/contracts/src/pullRequestReviewGuide.ts`: an agent-written tour of a PR. */

/** [kind]: tradeoff | contract | product | risk | accessibility. */
@Serializable
data class PullRequestReviewGuideAttentionDto(val kind: String = "risk", val note: String = "")

/** Lines [start]..[end] of [path] on [side] (new | old). */
@Serializable
data class PullRequestReviewGuideExcerptDto(
    val path: String = "",
    val side: String = "new",
    val start: Int = 1,
    val end: Int = 1,
    val caption: String = "",
)

@Serializable
data class PullRequestReviewGuideNoteDto(
    val path: String = "",
    val line: Int = 1,
    val title: String = "",
    val body: String = "",
)

@Serializable
data class PullRequestReviewGuideFileDto(val path: String = "", val note: String? = null)

@Serializable
data class PullRequestReviewGuideStopDto(
    val title: String = "",
    val summary: String = "",
    val attention: PullRequestReviewGuideAttentionDto? = null,
    val excerpts: List<PullRequestReviewGuideExcerptDto> = emptyList(),
    val files: List<PullRequestReviewGuideFileDto> = emptyList(),
    val notes: List<PullRequestReviewGuideNoteDto> = emptyList(),
    val questions: List<String> = emptyList(),
)

/** `PullRequestReviewGuide`; [modelSelection] is kept opaque, only round-tripped through the cache. */
@Serializable
data class PullRequestReviewGuideDto(
    val headSha: String = "",
    val generatedAt: String = "",
    val modelSelection: JsonElement? = null,
    val stops: List<PullRequestReviewGuideStopDto> = emptyList(),
)

/** `PullRequestReviewGuideRun`, tagged Running (with [startedAt]) or Failed (with [failedAt] and [detail]). */
@Serializable
data class PullRequestReviewGuideRunDto(
    @SerialName("_tag") val tag: String = "",
    val headSha: String = "",
    val startedAt: String? = null,
    val failedAt: String? = null,
    val detail: String? = null,
)

/** `PullRequestReviewGuideResult`: the saved guide, the run writing one, and the PR's head now. */
@Serializable
data class PullRequestReviewGuideResultDto(
    val guide: PullRequestReviewGuideDto? = null,
    val run: PullRequestReviewGuideRunDto? = null,
    val currentHeadSha: String? = null,
)
