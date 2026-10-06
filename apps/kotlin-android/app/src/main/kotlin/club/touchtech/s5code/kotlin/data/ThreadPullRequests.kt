package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.PullRequestRef
import club.touchtech.s5code.kotlin.model.PullRequestRefKind
import club.touchtech.s5code.kotlin.model.PullRequestState
import club.touchtech.s5code.kotlin.model.ThreadLinkedPullRequest
import club.touchtech.s5code.kotlin.model.ThreadPullRequestLink
import club.touchtech.s5code.kotlin.model.ThreadPullRequestSnapshot
import club.touchtech.s5code.kotlin.model.ThreadPullRequestStack
import club.touchtech.s5code.kotlin.model.ThreadPullRequestStackLayer
import club.touchtech.s5code.kotlin.transport.normalizeThreadPullRequestKey
import club.touchtech.s5code.kotlin.transport.threadPullRequestKeyOf
import club.touchtech.s5code.kotlin.transport.visibleThreadPullRequests
import club.touchtech.s5code.kotlin.transport.wire.ThreadLinkedPullRequestDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadPullRequestLinkDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadPullRequestSnapshotDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadPullRequestStackDto

/**
 * Chain and badge resolution over a thread's pull-request links, ported from
 * `packages/shared/src/threadPullRequests.ts` and the presentation half of
 * `apps/mobile/src/state/thread-pr-presentation.ts`.
 *
 * The resolvers run on the wire DTOs — `ThreadPullRequestLink.watch` stays
 * opaque there and never reaches these — while [toModel] produces the
 * `model/` shapes `ThreadDetail.pullRequests` carries.
 */

fun ThreadPullRequestLinkDto.toModel(): ThreadPullRequestLink =
    ThreadPullRequestLink(
        host = host,
        repository = repository,
        number = number,
        url = url,
        source = source,
        linkedAt = linkedAt,
        snapshot = snapshot?.toModel(),
        stack = stack?.toModel(),
        watched = watch != null,
    )

fun ThreadPullRequestSnapshotDto.toModel(): ThreadPullRequestSnapshot =
    ThreadPullRequestSnapshot(
        state = state,
        title = title,
        headBranch = headBranch,
        baseBranch = baseBranch,
        isDraft = isDraft,
        updatedAt = updatedAt,
        syncedAt = syncedAt,
        closedAt = closedAt,
        mergedAt = mergedAt,
        additions = additions,
        deletions = deletions,
        changedFiles = changedFiles,
        reviewDecision = reviewDecision,
        checksState = checksState,
    )

fun ThreadPullRequestStackDto.toModel(): ThreadPullRequestStack =
    ThreadPullRequestStack(
        id = id,
        number = number,
        url = url,
        base = base,
        layers = layers.map { ThreadPullRequestStackLayer(it.number, it.headBranch, it.state) },
    )

fun ThreadLinkedPullRequestDto.toModel(): ThreadLinkedPullRequest =
    ThreadLinkedPullRequest(
        projectId = projectId,
        repository = repository,
        number = number,
        url = url,
    )

/** A link is open until its host snapshot says otherwise — unsynced links are just-linked. */
private fun ThreadPullRequestLinkDto.isOpen(): Boolean = snapshot == null || snapshot.state == "open"

private fun ThreadPullRequestLinkDto.latestUpdatedAtMillis(): Long =
    parseInstant(snapshot?.updatedAt ?: linkedAt) ?: 0

/** The link one-slot surfaces (sidebar badge, copy link) present. */
sealed interface ThreadCurrentPullRequest {
    data class Single(val link: ThreadPullRequestLinkDto) : ThreadCurrentPullRequest

    data class Stack(
        val open: List<ThreadPullRequestLinkDto>,
        /** Highest layer of the open set; the one "View PR" and copy-link target. */
        val top: ThreadPullRequestLinkDto,
    ) : ThreadCurrentPullRequest
}

/**
 * `resolveThreadCurrentPullRequest`: prefers open work and the highest open
 * layer within a chain. A completed chain still points at its top; unrelated
 * terminal links use the most recently updated request.
 */
fun resolveThreadCurrentPullRequest(
    links: List<ThreadPullRequestLinkDto>,
): ThreadCurrentPullRequest? {
    val visible = visibleThreadPullRequests(links)
    if (visible.isEmpty()) return null
    val open = visible.filter { it.isOpen() }
    if (open.size == 1) return ThreadCurrentPullRequest.Single(open[0])
    val chains = resolveThreadPullRequestChains(visible)
    if (open.size > 1) {
        val ordered = chains
            .map { chain -> chain.layers.asReversed().filter { it.isOpen() } }
            .filter { it.isNotEmpty() }
            .sortedByDescending { layers ->
                layers.maxOf { parseInstant(it.linkedAt) ?: 0 }
            }
            .flatten()
        return ThreadCurrentPullRequest.Stack(open = ordered, top = ordered.first())
    }
    if (chains.size == 1) {
        return ThreadCurrentPullRequest.Single(chains[0].layers.last())
    }
    val terminal = visible.sortedByDescending { it.latestUpdatedAtMillis() }
    return ThreadCurrentPullRequest.Single(terminal[0])
}

/** The one link a legacy `linkedPullRequest` consumer should see, or null. */
fun resolveThreadCurrentPullRequestLink(
    links: List<ThreadPullRequestLinkDto>,
): ThreadPullRequestLinkDto? =
    when (val current = resolveThreadCurrentPullRequest(links)) {
        null -> null
        is ThreadCurrentPullRequest.Single -> current.link
        is ThreadCurrentPullRequest.Stack -> current.top
    }

/** `ThreadPullRequestChain`: bottom to top. */
data class ThreadPullRequestChain(
    /** native | derived */
    val kind: String,
    val layers: List<ThreadPullRequestLinkDto>,
)

/**
 * Groups a thread's links into stacks. Native stacks come from the host and
 * win; the rest chain by matching one link's base branch to another's head
 * branch within the same repository. A link that chains to nothing is a
 * one-layer chain.
 */
fun resolveThreadPullRequestChains(
    links: List<ThreadPullRequestLinkDto>,
): List<ThreadPullRequestChain> {
    val visible = visibleThreadPullRequests(links)
    val chains = mutableListOf<ThreadPullRequestChain>()
    val placed = mutableSetOf<String>()

    val nativeStacks = mutableMapOf<String, MutableList<ThreadPullRequestLinkDto>>()
    for (link in visible) {
        val stack = link.stack ?: continue
        val key = normalizeThreadPullRequestKey(link)
        nativeStacks.getOrPut("${key.host}/${key.repository}#stack:${stack.id}") {
            mutableListOf()
        }.add(link)
    }
    for (members in nativeStacks.values) {
        val order = members[0].stack!!.layers
            .mapIndexed { index, layer -> layer.number to index }
            .toMap()
        members.sortBy { order[it.number] ?: 0 }
        for (member in members) placed.add(threadPullRequestKeyOf(member))
        chains.add(ThreadPullRequestChain(kind = "native", layers = members))
    }

    val remaining = visible.filter { threadPullRequestKeyOf(it) !in placed }
    fun branchKey(link: ThreadPullRequestLinkDto, branch: String): String {
        val key = normalizeThreadPullRequestKey(link)
        return "${key.host}/${key.repository}:$branch"
    }
    // Reused head names cannot identify a parent unambiguously.
    val byHead = mutableMapOf<String, ThreadPullRequestLinkDto?>()
    for (link in remaining) {
        val snapshot = link.snapshot ?: continue
        val key = branchKey(link, snapshot.headBranch)
        byHead[key] = if (key in byHead) null else link
    }
    val hasChild = mutableSetOf<String>()
    for (link in remaining) {
        val snapshot = link.snapshot ?: continue
        val parent = byHead[branchKey(link, snapshot.baseBranch)]
        if (parent != null && parent !== link) hasChild.add(threadPullRequestKeyOf(parent))
    }
    // Walk from each top (a link nothing builds on) down its base chain.
    for (top in remaining) {
        if (threadPullRequestKeyOf(top) in hasChild) continue
        val layers = mutableListOf<ThreadPullRequestLinkDto>()
        var cursor: ThreadPullRequestLinkDto? = top
        while (cursor != null && threadPullRequestKeyOf(cursor) !in placed) {
            placed.add(threadPullRequestKeyOf(cursor))
            layers.add(0, cursor)
            val parent = cursor.snapshot?.let { byHead[branchKey(cursor, it.baseBranch)] }
            cursor = parent
        }
        if (layers.isNotEmpty()) chains.add(ThreadPullRequestChain(kind = "derived", layers = layers))
    }
    // Cycles have no top. Keep those links visible without inventing a stack order.
    for (link in remaining) {
        if (threadPullRequestKeyOf(link) !in placed) {
            chains.add(ThreadPullRequestChain(kind = "derived", layers = listOf(link)))
        }
    }
    return chains
}

/**
 * The aggregate badge over a thread's visible links: stack height when every
 * visible link shares one chain, otherwise a linked count
 * (`resolveThreadPullRequestBadge`).
 */
sealed interface ThreadPullRequestBadge {
    /** open | closed | merged | draft */
    val state: String

    data class Stack(val layers: Int, override val state: String) : ThreadPullRequestBadge

    data class PullRequest(val others: Int, override val state: String) : ThreadPullRequestBadge
}

fun resolveThreadPullRequestBadge(
    pullRequests: List<ThreadPullRequestLinkDto>?,
): ThreadPullRequestBadge? {
    val visible = visibleThreadPullRequests(pullRequests.orEmpty())
    if (visible.isEmpty()) return null
    val states = visible.map { it.snapshot?.state ?: "open" }
    val state =
        if (visible.all { it.snapshot?.state == "open" && it.snapshot?.isDraft == true }) "draft"
        else if ("open" in states) "open"
        else if (states.all { it == "merged" }) "merged"
        else "closed"
    val chains = resolveThreadPullRequestChains(visible)
    if (visible.size > 1 && chains.size == 1) {
        return ThreadPullRequestBadge.Stack(layers = visible.size, state = state)
    }
    return ThreadPullRequestBadge.PullRequest(others = visible.size - 1, state = state)
}

/**
 * `threadPullRequestSearchTerms`: the strings a `#` or repository-qualified
 * search matches against, over both the link array and the legacy single link.
 */
fun threadPullRequestSearchTerms(
    pullRequests: List<ThreadPullRequestLinkDto>?,
    linkedPullRequest: ThreadLinkedPullRequestDto?,
): List<String> {
    if (!pullRequests.isNullOrEmpty()) {
        return visibleThreadPullRequests(pullRequests).flatMap { link ->
            listOf(
                "#${link.number}",
                "${link.repository}#${link.number}",
                link.url,
                link.snapshot?.title.orEmpty(),
            )
        }
    }
    return linkedPullRequest?.let {
        listOf("#${it.number}", "${it.repository}#${it.number}", it.url)
    }.orEmpty()
}

private fun pullRequestStateOf(state: String?, isDraft: Boolean): PullRequestState =
    when {
        state == null -> PullRequestState.Unknown
        isDraft && state == "open" -> PullRequestState.Draft
        // The aggregate badge's "draft" — every visible link is a draft — folds
        // into the model's single draft state here.
        state == "draft" -> PullRequestState.Draft
        state == "merged" -> PullRequestState.Merged
        state == "closed" -> PullRequestState.Closed
        else -> PullRequestState.Open
    }

/**
 * `presentThreadLinkedPullRequests`: the row badge over the persisted link
 * array. Stack height and "+N" counts replace the bare number, and the state
 * is the aggregate's — a stack whose base merged but top is open reads open.
 * An aggregate "draft" collapses to open-plus-draft here, the same fold RN
 * makes (`isMultiple` reads the aggregate state, `isDraft` keeps the flag);
 * with no separate draft flag on [PullRequestRef], the model's `Draft` state
 * carries it.
 */
fun presentThreadLinkedPullRequests(links: List<ThreadPullRequestLinkDto>): PullRequestRef? {
    val link = resolveThreadCurrentPullRequestLink(links) ?: return null
    val badge = resolveThreadPullRequestBadge(links) ?: return null
    val snapshot = link.snapshot
    val isMultiple = badge is ThreadPullRequestBadge.Stack || badge.othersOrZero() > 0
    val isDraft =
        if (isMultiple) badge.state == "draft"
        else snapshot?.isDraft == true && (snapshot.state == "open")
    return PullRequestRef(
        number = link.number,
        state =
            pullRequestStateOf(
                if (isMultiple) badge.state else snapshot?.state,
                isDraft,
            ),
        title = snapshot?.title?.takeIf { it.isNotBlank() } ?: "#${link.number}",
        url = link.url,
        kind =
            when (badge) {
                is ThreadPullRequestBadge.Stack -> PullRequestRefKind.Stack
                is ThreadPullRequestBadge.PullRequest -> PullRequestRefKind.PullRequest
            },
        layers = (badge as? ThreadPullRequestBadge.Stack)?.layers ?: 0,
        others = (badge as? ThreadPullRequestBadge.PullRequest)?.others ?: 0,
        pendingSync = snapshot == null,
    )
}

private fun ThreadPullRequestBadge.othersOrZero(): Int =
    (this as? ThreadPullRequestBadge.PullRequest)?.others ?: 0
