package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.PullRequestActivityDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDetailDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDiffResultDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDiffStatDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestFilesViewedResultDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestLinkedThreadsResultDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestListEntryDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestListResultDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestStackDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestStackHeadDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestThreadCommentsResultDto
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.StateFlow

/**
 * One cached pull request read. [value] survives a failed refetch, so a
 * screen keeps drawing last-known data and shows [error] beside it.
 */
data class PullRequestQuery<T>(
    val value: T? = null,
    val hasValue: Boolean = false,
    val error: String? = null,
    val loading: Boolean = false,
    val fetchedAt: Long? = null,
    /** Restored from disk and not yet refetched this session. */
    val fromCache: Boolean = false,
)

/** One environment's listing, as the Pull requests tab asks for it. */
data class PullRequestListTarget(val environmentId: String, val input: PullRequestListInput) {
    /** Cursors never take part: the head query is what refreshes. */
    val key: String
        get() = "list:" + input.copy(cursors = null).toJson().toString()
}

/** A target's head query plus every page loaded beneath it. */
data class PullRequestListSource(
    val target: PullRequestListTarget,
    val query: PullRequestQuery<PullRequestListResultDto>,
    val pages: PullRequestListPages?,
)

/**
 * Pull request reads for the Pull requests tab and PR pages, kept apart from
 * [WorkspaceGateway] because they cache and refresh on their own schedule.
 * Phase 2 (mutations, files, review) extends this interface; reads stay
 * here so every screen shares one cache.
 */
interface PullRequestGateway {
    /** Saved list controls by scope: an environment id, or "" for all environments. */
    val listControls: StateFlow<Map<String, PullRequestListControls>>

    fun setListControls(scope: String, controls: PullRequestListControls)

    /** The environment the tab is scoped to; null for all. */
    val listEnvironment: StateFlow<String?>

    fun setListEnvironment(environmentId: String?)

    /** Environments seen serving pull requests, remembered for offline launches. */
    val knownEnvironments: StateFlow<Set<String>>

    /** True while the environment's socket is up. */
    fun connected(environmentId: String): Flow<Boolean>

    fun list(targets: List<PullRequestListTarget>): Flow<List<PullRequestListSource>>

    /**
     * Loads the next page for every target that has one. Throws the first
     * failure, including "Couldn't advance the list. Try again." when a host
     * hands back the cursors it was given.
     */
    suspend fun loadMore(targets: List<PullRequestListTarget>, retry: Boolean)

    /** Drops the server's cached listings and reloads each head, forgetting loaded pages. */
    suspend fun refreshList(targets: List<PullRequestListTarget>)

    /** `projectId:repository:number` → diff counts the listing did not carry. */
    fun stats(environmentId: String): StateFlow<Map<String, PullRequestDiffStatDto>>

    /** Fetches counts for [references] not already known or in flight. */
    fun loadStats(environmentId: String, references: List<PullRequestReference>)

    fun detail(environmentId: String, reference: PullRequestReference): Flow<PullRequestQuery<PullRequestDetailDto>>

    fun activity(environmentId: String, reference: PullRequestReference): Flow<PullRequestQuery<PullRequestActivityDto>>

    /** Null value: the PR sits in no stack. */
    fun stack(environmentId: String, reference: PullRequestReference): Flow<PullRequestQuery<PullRequestStackDto?>>

    fun linkedThreads(
        environmentId: String,
        reference: PullRequestReference,
    ): Flow<PullRequestQuery<PullRequestLinkedThreadsResultDto>>

    fun filesViewed(
        environmentId: String,
        reference: PullRequestReference,
    ): Flow<PullRequestQuery<PullRequestFilesViewedResultDto>>

    /** One slice of the PR's patch over HTTP (`POST /api/pull-requests/diff`). */
    suspend fun diff(
        environmentId: String,
        reference: PullRequestReference,
        cursor: String? = null,
        commit: String? = null,
    ): PullRequestDiffResultDto

    /** Refetches the PR page's reads: detail, activity and stack. */
    suspend fun refresh(environmentId: String, reference: PullRequestReference)

    /** Refetches only the detail, unless it is final (merged). */
    suspend fun refreshDetail(environmentId: String, reference: PullRequestReference)

    /** The listing row a PR page can draw before its detail loads. */
    fun summary(environmentId: String, reference: PullRequestReference): PullRequestListEntryDto?

    /* ── Writes ──────────────────────────────────────────────────────────
     * One write runs at a time per environment, in call order, matching the
     * client-runtime's serial-per-environment commands. Each throws with a
     * message fit for an alert when the host refuses.
     */

    /**
     * `pullRequests.runAction`. A single merge passes [mergeMethod] and is
     * re-checked against a fresh read first: refused when the PR can no longer
     * merge or sits in a stack. A stack merge passes [stackNumber] with
     * [expectedStackHeads] instead. Detail, activity and stack refetch after.
     */
    suspend fun runAction(
        environmentId: String,
        reference: PullRequestReference,
        action: String,
        mergeMethod: String? = null,
        updateMethod: String? = null,
        stackNumber: Int? = null,
        expectedStackHeads: List<PullRequestStackHeadDto>? = null,
    )

    /** `pullRequests.comment`: a top-level comment. Activity refetches after. */
    suspend fun comment(environmentId: String, reference: PullRequestReference, body: String)

    /** `pullRequests.submitReview`. Detail and activity refetch after. */
    suspend fun submitReview(
        environmentId: String,
        reference: PullRequestReference,
        verdict: String,
        body: String,
        comments: List<PendingReviewComment>,
    )

    /** `pullRequests.replyToThread`. Activity refetches after. */
    suspend fun replyToThread(environmentId: String, reference: PullRequestReference, threadId: String, body: String)

    /** `pullRequests.setThreadResolution`. Activity refetches after. */
    suspend fun setThreadResolution(
        environmentId: String,
        reference: PullRequestReference,
        threadId: String,
        resolved: Boolean,
    )

    /** `pullRequests.threadComments`: the next page of a review thread's replies. */
    suspend fun threadComments(
        environmentId: String,
        reference: PullRequestReference,
        threadId: String,
        cursor: String,
    ): PullRequestThreadCommentsResultDto

    /**
     * Viewed marks pressed but not yet confirmed by a fresh read, by path.
     * Screens draw these over [filesViewed].
     */
    fun filesViewedOverlay(environmentId: String, reference: PullRequestReference): StateFlow<Map<String, Boolean>>

    /**
     * `pullRequests.setFilesViewed` for one file, serial per pull request.
     * The mark shows at once; a failure reverts it and throws.
     */
    suspend fun setFileViewed(environmentId: String, reference: PullRequestReference, path: String, viewed: Boolean)

    /**
     * `pullRequests.requestReviewers`; patches the cached detail and activity
     * reviewers for [reviewers] whose identity is known, refetching otherwise.
     */
    suspend fun requestReviewers(
        environmentId: String,
        reference: PullRequestReference,
        reviewers: List<PullRequestReviewerRequest>,
        requested: Boolean,
    )

    /** `pullRequests.invalidate`: drops the server's host cache, then refetches the PR page. */
    suspend fun invalidate(environmentId: String, reference: PullRequestReference?)
}

/** One reviewer to (un)request; [login] lets the cached reviewer lists update without a refetch. */
data class PullRequestReviewerRequest(
    val id: String,
    /** user | team */
    val kind: String,
    val login: String? = null,
    val name: String? = null,
    val avatarUrl: String? = null,
)
