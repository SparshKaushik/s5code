package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.PullRequestCheckDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestChecksDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDetailDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReactionDto

/*
 * Editing, labels, reactions and live checks on a PR page, ported from
 * client-runtime's `pullRequestEditing.ts`, `pullRequestReactions.ts` and
 * `pullRequestChecks.ts`.
 */

private fun sameLogin(one: String?, other: String?): Boolean =
    one != null && other != null && one.trim().equals(other.trim(), ignoreCase = true)

/** `canEditPullRequestChangeRequest`: the host's answer wins; otherwise the author or a merger may edit. */
fun canEditPullRequestChangeRequest(detail: PullRequestDetailDto): Boolean {
    if (detail.capabilities.edit?.changeRequest != true) return false
    detail.viewerPermissions.editChangeRequest?.let { return it }
    return sameLogin(detail.viewer, detail.author?.login) || "merge" in detail.viewerPermissions.actions
}

/** `canEditPullRequestMaintainerEdits`: open GitHub PRs whose author may toggle maintainer pushes. */
fun canEditPullRequestMaintainerEdits(detail: PullRequestDetailDto): Boolean =
    detail.state == "open" &&
        detail.capabilities.edit?.maintainerEdits == true &&
        detail.viewerPermissions.maintainerEdits == true &&
        detail.maintainerCanModify != null

/** A maintainer-edits toggle not yet confirmed by a host reading, keyed by PR url. */
data class MaintainerEditsChoice(val key: String, val observedAt: Double?, val value: Boolean)

/**
 * `maintainerEditsChoiceInForce`: a saved choice ends at the first host reading
 * delivered after the save. The save refetches, so that reading started after the write.
 */
fun maintainerEditsChoiceInForce(
    choice: MaintainerEditsChoice?,
    key: String,
    observedAt: Double?,
    saving: Boolean,
): MaintainerEditsChoice? {
    if (choice == null || choice.key != key) return null
    if (saving) return if (choice.observedAt == observedAt) choice else choice.copy(observedAt = observedAt)
    return if (observedAt != null && choice.observedAt == observedAt) choice else null
}

/**
 * `canEditPullRequestComment`. Review summaries cannot be rewritten through
 * comment mutations, so only issue and review comments qualify.
 */
fun canEditPullRequestComment(
    detail: PullRequestDetailDto,
    kind: String,
    authorLogin: String?,
    canEdit: Boolean?,
): Boolean {
    if (detail.capabilities.edit?.comment != true) return false
    if (kind != "issue-comment" && kind != "review-comment") return false
    canEdit?.let { return it }
    return sameLogin(detail.viewer, authorLogin)
}

/** `PullRequestLabels` editable rule: an unreported viewer permission is granted. */
fun canManagePullRequestLabels(detail: PullRequestDetailDto): Boolean =
    detail.capabilities.labels == true && detail.viewerPermissions.labels != false

/* ── Reactions ───────────────────────────────────────────────────────── */

val PULL_REQUEST_REACTION_ORDER =
    listOf("thumbs-up", "thumbs-down", "laugh", "hooray", "confused", "heart", "rocket", "eyes")

fun pullRequestReactionEmoji(content: String): String =
    when (content) {
        "thumbs-up" -> "👍"
        "thumbs-down" -> "👎"
        "laugh" -> "😄"
        "hooray" -> "🎉"
        "confused" -> "😕"
        "heart" -> "❤️"
        "rocket" -> "🚀"
        "eyes" -> "👀"
        else -> content
    }

fun pullRequestReactionName(content: String): String = content.replace('-', ' ')

private const val NAMED_ACTOR_LIMIT = 3

private fun joinNames(parts: List<String>): String =
    when (parts.size) {
        0 -> ""
        1 -> parts[0]
        2 -> "${parts[0]} and ${parts[1]}"
        else -> parts.dropLast(1).joinToString(", ") + ", and " + parts.last()
    }

/**
 * `pullRequestReactionTooltip`. Hosts omit the viewer from actors, so "You"
 * joins only when the count has room for it.
 */
fun pullRequestReactionTooltip(reaction: PullRequestReactionDto): String {
    val viewerHasRoom = reaction.actors.size < reaction.count
    val names = if (reaction.viewerHasReacted && viewerHasRoom) listOf("You") + reaction.actors else reaction.actors
    val shown = names.take(minOf(NAMED_ACTOR_LIMIT, reaction.count))
    val others = maxOf(0, reaction.count - shown.size)
    val remainder =
        when {
            others == 0 -> null
            shown.isNotEmpty() -> "$others ${if (others == 1) "other" else "others"}"
            else -> "$others ${if (others == 1) "person" else "people"}"
        }
    return "${joinNames(shown + listOfNotNull(remainder))} reacted with ${pullRequestReactionName(reaction.content)} emoji"
}

/** Changes whenever the host reports different reactions, so a pending overlay can reset. */
fun pullRequestReactionsSignature(reactions: List<PullRequestReactionDto>): String =
    reactions.joinToString(" ") { "${it.content}:${it.count}:${if (it.viewerHasReacted) 1 else 0}" }

/** `applyPendingPullRequestReactions`: taps not yet confirmed by a fresh read, drawn over the host's. */
fun applyPendingPullRequestReactions(
    reactions: List<PullRequestReactionDto>,
    pending: Map<String, Boolean>,
): List<PullRequestReactionDto> {
    if (pending.isEmpty()) return reactions
    val byContent = reactions.associateByTo(LinkedHashMap()) { it.content }
    for ((content, reacted) in pending) {
        val current = byContent[content]
        if (current == null) {
            if (reacted) byContent[content] = PullRequestReactionDto(content, 1, emptyList(), true)
            continue
        }
        if (current.viewerHasReacted == reacted) continue
        val count = current.count + if (reacted) 1 else -1
        if (count <= 0) byContent.remove(content) else byContent[content] = current.copy(count = count, viewerHasReacted = reacted)
    }
    return PULL_REQUEST_REACTION_ORDER.mapNotNull(byContent::get)
}

/* ── Live checks ─────────────────────────────────────────────────────── */

/**
 * `resolvePullRequestDetailChecks`: when both reads carry `observedAt` the
 * newer wins; otherwise [checksAreNewer] decides. A merged PR stays merged.
 */
fun resolvePullRequestDetailChecks(
    detail: PullRequestDetailDto?,
    checks: PullRequestChecksDto?,
    checksAreNewer: Boolean,
): PullRequestDetailDto? {
    if (detail == null || checks == null) return detail
    val detailObserved = detail.observedAt
    val checksObserved = checks.observedAt
    val newer = if (detailObserved != null && checksObserved != null) checksObserved >= detailObserved else checksAreNewer
    if (!newer) return detail
    return detail.copy(state = if (detail.state == "merged") "merged" else checks.state, checks = checks.checks)
}

/** `pullRequestChecksRefreshIntervalMs`: running checks poll tighter than a settled head. */
fun pullRequestChecksRefreshIntervalMs(checks: List<PullRequestCheckDto>): Long =
    if (checks.isEmpty() || checks.any { it.status == "pending" || it.status == "action-required" }) 45_000L else 60_000L
