package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.design.text.MdBlock
import club.touchtech.s5code.kotlin.design.text.parseMarkdown
import club.touchtech.s5code.kotlin.transport.wire.PullRequestActivityDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestActorDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestCheckDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestCommentDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestCommitDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDetailDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewThreadDto

/*
 * What a PR page says about a pull request, ported from
 * `apps/mobile/src/features/pull-requests/` (`pullRequestStatus.ts`,
 * `pullRequestActivityFilter.ts`, `botBadgeImages.ts`,
 * `pullRequestEnvironmentFallback.ts`, `PullRequestTimeline.tsx`). Tones are
 * semantic so the screens pick theme colors.
 */

enum class PullRequestTone { Success, Danger, Warning, Muted, Merged, Accent }

/* ── Status ──────────────────────────────────────────────────────────── */

enum class PullRequestStatus(val label: String, val tone: PullRequestTone) {
    Open("Open", PullRequestTone.Success),
    Merged("Merged", PullRequestTone.Merged),
    Closed("Closed", PullRequestTone.Danger),
    Draft("Draft", PullRequestTone.Muted),
}

/** `pullRequestStatus`: an open draft reads as Draft. */
fun pullRequestStatus(state: String, isDraft: Boolean): PullRequestStatus =
    when {
        isDraft && state == "open" -> PullRequestStatus.Draft
        state == "merged" -> PullRequestStatus.Merged
        state == "closed" -> PullRequestStatus.Closed
        else -> PullRequestStatus.Open
    }

/** `pullRequestConflict`: only an open, ready PR with a reported conflict. */
fun pullRequestConflict(state: String, isDraft: Boolean, mergeability: String?, baseBranch: String?): String? {
    if (state != "open" || isDraft || mergeability != "conflicting") return null
    return if (!baseBranch.isNullOrEmpty()) "Conflicts with $baseBranch" else "Has conflicts"
}

data class PullRequestCheckStatus(val label: String, val tone: PullRequestTone)

fun checkStatus(check: PullRequestCheckDto): PullRequestCheckStatus =
    when (check.status) {
        "success" -> PullRequestCheckStatus("Passed", PullRequestTone.Success)
        "failure" -> PullRequestCheckStatus("Failed", PullRequestTone.Danger)
        "action-required" -> PullRequestCheckStatus("Action required", PullRequestTone.Warning)
        "pending" -> PullRequestCheckStatus("Pending", PullRequestTone.Warning)
        "cancelled" -> PullRequestCheckStatus("Cancelled", PullRequestTone.Muted)
        "skipped" -> PullRequestCheckStatus("Skipped", PullRequestTone.Muted)
        else -> PullRequestCheckStatus("Neutral", PullRequestTone.Muted)
    }

private val CHECK_PRIORITY =
    mapOf("failure" to 0, "action-required" to 1, "pending" to 2, "success" to 3)

/** Failing first, then what needs attention, running, passed, and the rest; stable by name. */
fun sortedPullRequestChecks(checks: List<PullRequestCheckDto>): List<PullRequestCheckDto> =
    checks.sortedWith(compareBy<PullRequestCheckDto> { CHECK_PRIORITY[it.status] ?: 4 }.thenBy { it.name.lowercase() })

data class PullRequestChecksSummary(
    val title: String,
    val label: String,
    val count: String?,
    val tone: PullRequestTone,
)

fun pullRequestChecksSummary(checks: List<PullRequestCheckDto>): PullRequestChecksSummary {
    val failed = checks.count { it.status == "failure" }
    val required = checks.count { it.status == "action-required" }
    val pending = checks.count { it.status == "pending" }
    val passed = checks.count { it.status == "success" }
    val count = if (checks.isNotEmpty()) "$passed of ${checks.size}" else null
    return when {
        failed > 0 -> {
            val text = "$failed ${if (failed == 1) "check" else "checks"} failing"
            PullRequestChecksSummary(text, text, count, PullRequestTone.Danger)
        }
        required > 0 ->
            PullRequestChecksSummary(
                "Checks need attention",
                "$required need attention",
                "$required awaiting action",
                PullRequestTone.Warning,
            )
        pending > 0 ->
            PullRequestChecksSummary("Checks running", "$pending pending", "$pending pending", PullRequestTone.Warning)
        passed > 0 ->
            PullRequestChecksSummary(
                if (passed == checks.size) "All checks passed" else "Checks completed",
                "$passed of ${checks.size} passed",
                count,
                PullRequestTone.Success,
            )
        else ->
            PullRequestChecksSummary(
                if (checks.isNotEmpty()) "Checks completed" else "No checks reported",
                if (checks.isNotEmpty()) "${checks.size} completed" else "No checks reported",
                count,
                PullRequestTone.Muted,
            )
    }
}

/** A list row's rolled-up checks state. */
fun pullRequestChecksStateTone(state: String): PullRequestTone =
    when (state) {
        "passing" -> PullRequestTone.Success
        "failing" -> PullRequestTone.Danger
        else -> PullRequestTone.Warning
    }

data class PullRequestReviewSummary(
    val title: String,
    val count: String,
    val tone: PullRequestTone,
    /** Each reviewer's standing review, `reviewState` normalized to `APPROVED`-style. */
    val reviews: List<PullRequestCommentDto>,
)

private fun epoch(value: String): Long = parseInstant(value) ?: 0L

/**
 * `pullRequestReviewSummary`: each reviewer's latest verdict counts; a comment
 * review counts only for someone with no verdict, and an approval older than
 * the newest commit no longer approves. With [canRequest], the empty state
 * offers a request rather than just reporting none.
 */
fun pullRequestReviewSummary(
    reviewers: List<PullRequestActorDto>,
    activity: PullRequestActivityDto?,
    canRequest: Boolean = false,
): PullRequestReviewSummary {
    val latest = LinkedHashMap<String, PullRequestCommentDto>()
    val comments = LinkedHashMap<String, PullRequestCommentDto>()
    for (review in activity?.comments.orEmpty()) {
        val rawState = review.reviewState
        if (review.kind != "review" || rawState.isNullOrEmpty()) continue
        val state = rawState.uppercase().replace("-", "_")
        val key = review.author?.login ?: review.id
        val target =
            when (state) {
                "COMMENTED" -> comments
                "APPROVED", "CHANGES_REQUESTED", "DISMISSED" -> latest
                else -> continue
            }
        val previous = target[key]
        if (previous == null || epoch(review.createdAt) >= epoch(previous.createdAt)) {
            target[key] = review.copy(reviewState = state)
        }
    }
    val reviews =
        latest.values.filter { it.reviewState != "DISMISSED" } +
            comments.filterKeys { it !in latest }.values
    val requested = reviews.filter { it.reviewState == "CHANGES_REQUESTED" }
    val newestCommit = activity?.commits.orEmpty().maxOfOrNull { epoch(it.committedDate) }?.coerceAtLeast(0) ?: 0L
    val approved = reviews.filter { it.reviewState == "APPROVED" && epoch(it.createdAt) >= newestCommit }
    if (requested.isNotEmpty()) {
        return PullRequestReviewSummary(
            "Changes requested",
            "${requested.size} ${if (requested.size == 1) "reviewer" else "reviewers"}",
            PullRequestTone.Danger,
            reviews,
        )
    }
    if (approved.isNotEmpty()) {
        val only = approved.singleOrNull()?.author
        return PullRequestReviewSummary(
            if (only != null) "Approved by ${only.login}" else "Reviews approved",
            "${approved.size} approved",
            PullRequestTone.Success,
            reviews,
        )
    }
    val commented = reviews.count { it.reviewState == "COMMENTED" }
    if (commented > 0 && reviewers.isEmpty()) {
        return PullRequestReviewSummary("Reviews", "$commented commented", PullRequestTone.Muted, reviews)
    }
    return PullRequestReviewSummary(
        when {
            reviewers.isNotEmpty() -> "Awaiting review"
            canRequest -> "Request a review"
            else -> "No reviewers"
        },
        if (reviewers.isNotEmpty()) "${reviewers.size} requested" else "None assigned",
        PullRequestTone.Muted,
        reviews,
    )
}

fun pullRequestBranchSummary(detail: PullRequestDetailDto): String =
    when {
        detail.mergeability == "conflicting" -> "Has conflicts"
        detail.baseComparison == "behind" -> "${detail.behindBy ?: ""} behind ${detail.baseBranch}".trim()
        detail.baseComparison == "up-to-date" -> "Up to date"
        detail.mergeability == "mergeable" -> "No conflicts"
        else -> "Not reported"
    }

/* ── Activity ────────────────────────────────────────────────────────── */

enum class PullRequestActivityKind(val label: String) {
    All("All activity"),
    Comments("Comments"),
    Commits("Commits"),
}

data class PullRequestActivityFilter(
    val kind: PullRequestActivityKind = PullRequestActivityKind.All,
    val bots: Boolean = false,
    val resolved: Boolean = false,
    val newestFirst: Boolean = true,
)

data class FilteredPullRequestActivity(val activity: PullRequestActivityDto, val hiddenCount: Int)

private fun isBot(actor: PullRequestActorDto?) = actor?.isBot == true || actor?.login?.endsWith("[bot]") == true

private fun discussionCommentIds(activity: PullRequestActivityDto) =
    activity.reviewThreads.flatMapTo(HashSet()) { thread -> thread.comments.map { it.id } }

private fun eventCount(activity: PullRequestActivityDto): Int {
    val ids = discussionCommentIds(activity)
    return activity.comments.count { it.id !in ids } + activity.reviewThreads.size + activity.commits.size
}

/** `filterPullRequestActivity`; [FilteredPullRequestActivity.hiddenCount] is what the filter took out. */
fun filterPullRequestActivity(
    activity: PullRequestActivityDto,
    filter: PullRequestActivityFilter,
): FilteredPullRequestActivity {
    val commitsOnly = filter.kind == PullRequestActivityKind.Commits
    val threads =
        if (commitsOnly) emptyList()
        else
            activity.reviewThreads.filter { thread ->
                (filter.resolved || !thread.isResolved) &&
                    (filter.bots ||
                        (thread.commentCount ?: 0) > thread.comments.size ||
                        thread.nextCommentsCursor != null ||
                        thread.comments.any { !isBot(it.author) })
            }
    val ids = discussionCommentIds(activity)
    val comments =
        if (commitsOnly) emptyList()
        else
            activity.comments.filter { comment ->
                comment.id !in ids &&
                    (filter.bots || !isBot(comment.author)) &&
                    (filter.resolved || comment.reviewState?.trim()?.lowercase() != "dismissed")
            }
    val filtered =
        activity.copy(
            comments = comments,
            reviewThreads = threads,
            commits = if (filter.kind == PullRequestActivityKind.Comments) emptyList() else activity.commits,
            commentsTruncated = !commitsOnly && activity.commentsTruncated,
            reviewThreadsTruncated = !commitsOnly && activity.reviewThreadsTruncated == true,
        )
    return FilteredPullRequestActivity(filtered, eventCount(activity) - eventCount(filtered))
}

sealed interface PullRequestTimelineEvent {
    val key: String
    val date: String

    data class Comment(val comment: PullRequestCommentDto) : PullRequestTimelineEvent {
        override val key = "comment:${comment.id}"
        override val date = comment.createdAt
    }

    data class Discussion(val thread: PullRequestReviewThreadDto) : PullRequestTimelineEvent {
        override val key = "discussion:${thread.id}"
        override val date = thread.comments.firstOrNull()?.createdAt.orEmpty()
    }

    data class Commit(val commit: PullRequestCommitDto) : PullRequestTimelineEvent {
        override val key = "commit:${commit.oid}"
        override val date = commit.committedDate
    }
}

data class PullRequestTimeline(val events: List<PullRequestTimelineEvent>, val truncated: Boolean)

/** `pullRequestTimelineRows`: comments outside discussions, discussions and commits by date. */
fun pullRequestTimeline(activity: PullRequestActivityDto, newestFirst: Boolean): PullRequestTimeline {
    val ids = discussionCommentIds(activity)
    val events =
        (activity.comments.filter { it.id !in ids }.map(PullRequestTimelineEvent::Comment) +
                activity.reviewThreads.map(PullRequestTimelineEvent::Discussion) +
                activity.commits.map(PullRequestTimelineEvent::Commit))
            .sortedBy { it.date }
    return PullRequestTimeline(
        events = if (newestFirst) events.asReversed() else events,
        truncated = activity.commentsTruncated || activity.reviewThreadsTruncated == true,
    )
}

/** A comment collapses past this much text, matching the RN timeline. */
fun isLongPullRequestComment(body: String) = body.length > 450 || body.lines().size > 8

/* ── Description ─────────────────────────────────────────────────────── */

private val BOT_BADGE_PREFIXES =
    listOf(
        "static.devin.ai/assets/",
        "cursor.com/assets/images/",
        "storage.googleapis.com/coderabbit_public_assets/",
        "greptile-static-assets.s3.amazonaws.com/badges/",
        "static.graphite.dev/",
        "qodo.ai/wp-content/uploads/",
        "cubic.dev/buttons/",
        "github.githubassets.com/static/images/icons/copilot-code-review/",
        "gstatic.com/codereviewagent/",
        "vercel.com/vercel-agent-view-suggestion-",
        "agents-vade-review.vercel.sh/",
        "new-codeant-butcket.s3.us-west-1.amazonaws.com/badges/",
        "pr-comments-assets.blacksmith.sh/",
        "app.roocode.com/cloud-agents/github/",
        "app.parameter.ai/_static/icons/",
        "img.shields.io/",
        "raster.shields.io/",
        "dependabot-badges.githubapp.com/",
        "developer.mend.io/api/mc/badges/",
        "api.securityscorecards.dev/projects/",
        "github-app-statics.socket.dev/",
        "static.deepsource.com/comment_artifacts/",
        "sonarsource.github.io/sonarcloud-github-static-resources/",
        "codescene.io/imgs/svg/",
        "app.codecov.io/gh/",
        "github.com/codecov/engineering-team/assets/",
        "codspeed.io/pr-report/",
        "static.trunk.io/assets/",
        "res.cloudinary.com/snyk/",
        "api.segment.io/v1/pixel/",
        "cla-assistant.io/pull/badge/",
        "vercel.com/static/status/",
        "vercel.com/api/www/avatar",
        "app.netlify.com/qr-code/",
        "workers.cloudflare.com/logo.svg",
        // Cloudflare Pages reuses one old GitHub upload as its logo.
        "user-images.githubusercontent.com/23264/106598434-9e719e00-654f-11eb-9e59-6167043cfa01.png",
        "qr.expo.dev/",
    )

private val URL_SCHEME_PREFIX = Regex("^https?://(?:www\\.)?", RegexOption.IGNORE_CASE)

/**
 * `isBotBadgeImage`: review-bot and CI badges, which only the description's
 * Attachments strip skips; the markdown still renders them inline.
 */
fun isBotBadgeImage(href: String): Boolean {
    val path = href.replace(URL_SCHEME_PREFIX, "").lowercase()
    return BOT_BADGE_PREFIXES.any { path.startsWith(it) }
}

data class PullRequestAttachment(val uri: String, val caption: String)

/** The description's image attachments, first occurrence each, without bot badges. */
fun pullRequestAttachments(body: String): List<PullRequestAttachment> {
    val attachments = LinkedHashMap<String, PullRequestAttachment>()
    fun visit(blocks: List<MdBlock>) {
        for (block in blocks) {
            when (block) {
                is MdBlock.Paragraph ->
                    block.images.forEach { image ->
                        val uri = image.source
                        if (uri.startsWith("http://", true) || uri.startsWith("https://", true)) {
                            attachments.getOrPut(uri) {
                                PullRequestAttachment(uri, image.alt.ifEmpty { image.title ?: "Image" })
                            }
                        }
                    }
                is MdBlock.Quote -> visit(block.blocks)
                else -> Unit
            }
        }
    }
    visit(parseMarkdown(body))
    return attachments.values.filterNot { isBotBadgeImage(it.uri) }
}

/* ── Environments ────────────────────────────────────────────────────── */

/** A project as the offline-fallback lookup needs it. */
data class PullRequestProjectIdentity(
    val environmentId: String,
    val projectId: String,
    /** `host/owner/repo`, where the project has a repository identity. */
    val canonicalKey: String?,
)

data class PullRequestEnvironmentTarget(val environmentId: String, val projectId: String)

/**
 * `pullRequestFallbackEnvironment`: while the PR's own environment is offline,
 * a connected environment with a project on the same repository reads it.
 */
fun pullRequestFallbackEnvironment(
    environmentId: String,
    projectId: String,
    repository: String,
    host: String?,
    projects: List<PullRequestProjectIdentity>,
    connectedEnvironmentIds: Set<String>,
): PullRequestEnvironmentTarget? {
    if (environmentId in connectedEnvironmentIds) return null
    val own = projects.firstOrNull { it.environmentId == environmentId && it.projectId == projectId }
    val resolvedHost = host ?: own?.canonicalKey?.substringBefore('/') ?: return null
    val key = "$resolvedHost/$repository".lowercase()
    val copy =
        projects.firstOrNull {
            it.environmentId != environmentId &&
                it.environmentId in connectedEnvironmentIds &&
                it.canonicalKey?.lowercase() == key
        } ?: return null
    return PullRequestEnvironmentTarget(copy.environmentId, copy.projectId)
}

/* ── Providers ───────────────────────────────────────────────────────── */

fun pullRequestProviderLabel(provider: String?): String =
    when (provider) {
        "github" -> "GitHub"
        "gitlab" -> "GitLab"
        "gitcafe" -> "GitCafe"
        "forgejo" -> "Forgejo"
        "bitbucket" -> "Bitbucket"
        "azure-devops" -> "Azure DevOps"
        else -> "source control"
    }

fun openOnProviderLabel(provider: String?) = "Open on ${pullRequestProviderLabel(provider)}"

/** GitHub serves a PR's checks at `/checks`; other hosts land on the PR. */
fun pullRequestChecksUrl(provider: String, url: String) =
    if (provider == "github" && url.isNotEmpty()) "${url.trimEnd('/')}/checks" else url

/* ── Freshness ───────────────────────────────────────────────────────── */

/**
 * Whether a cached PR read should refetch: past its TTL, or the server's
 * refresh revision moved since it was read. A final read (a merged PR) never
 * changes again, so only a missing value refetches it.
 */
fun isPullRequestQueryStale(
    fetchedAt: Long?,
    now: Long,
    ttlMs: Long,
    fetchedRevision: Int?,
    currentRevision: Int?,
    final: Boolean,
): Boolean {
    if (fetchedAt == null) return true
    if (final) return false
    // A read made before the first revision arrived is judged by age alone.
    if (currentRevision != null && fetchedRevision != null && fetchedRevision != currentRevision) return true
    return now - fetchedAt >= ttlMs
}
