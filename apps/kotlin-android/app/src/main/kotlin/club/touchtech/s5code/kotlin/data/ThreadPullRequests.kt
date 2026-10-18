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
            .map { chain -> chain.layersFromTop().filter { it.isOpen() } }
            .filter { it.isNotEmpty() }
            .sortedByDescending { layers ->
                layers.maxOf { parseInstant(it.linkedAt) ?: 0 }
            }
            .flatten()
        return ThreadCurrentPullRequest.Stack(open = ordered, top = ordered.first())
    }
    if (chains.size == 1) {
        return ThreadCurrentPullRequest.Single(chains[0].layersFromTop().first())
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

/** `ThreadPullRequestChain`: bases first, each parent before the links built on it. */
data class ThreadPullRequestChain(
    /** native | derived */
    val kind: String,
    val layers: List<ThreadPullRequestLinkDto>,
    /** One per layer: 0 for the base, parent's depth + 1 above it. Siblings share a depth. */
    val depths: List<Int> = layers.indices.toList(),
)

/** `layersFromTop`: deepest first; among equals, the later layer first. */
private fun ThreadPullRequestChain.layersFromTop(): List<ThreadPullRequestLinkDto> =
    layers.indices
        .sortedWith(compareByDescending<Int> { depths[it] }.thenByDescending { it })
        .map { layers[it] }

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
        chains.add(ThreadPullRequestChain(kind = "native", layers = members, depths = members.indices.toList()))
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
    fun parentOf(link: ThreadPullRequestLinkDto): ThreadPullRequestLinkDto? {
        val snapshot = link.snapshot ?: return null
        val parent = byHead[branchKey(link, snapshot.baseBranch)]
        return if (parent == null || parent === link) null else parent
    }
    val children = mutableMapOf<String, MutableList<ThreadPullRequestLinkDto>>()
    for (link in remaining) {
        val parent = parentOf(link) ?: continue
        children.getOrPut(threadPullRequestKeyOf(parent)) { mutableListOf() }.add(link)
    }
    // Walk up from each base (a link that builds on nothing here), depth-first,
    // so siblings branching off one parent share a depth.
    for (base in remaining) {
        if (parentOf(base) != null) continue
        val layers = mutableListOf<ThreadPullRequestLinkDto>()
        val depths = mutableListOf<Int>()
        val pending = ArrayDeque(listOf(base to 0))
        while (pending.isNotEmpty()) {
            val (link, depth) = pending.removeLast()
            val key = threadPullRequestKeyOf(link)
            // Two links with one identity would otherwise share a child list and expand forever.
            if (!placed.add(key)) continue
            layers.add(link)
            depths.add(depth)
            children[key].orEmpty().asReversed().forEach { pending.addLast(it to depth + 1) }
        }
        if (layers.isNotEmpty()) chains.add(ThreadPullRequestChain(kind = "derived", layers = layers, depths = depths))
    }
    // Cycles have no base. Keep those links visible without inventing a stack order.
    for (link in remaining) {
        if (threadPullRequestKeyOf(link) !in placed) {
            chains.add(ThreadPullRequestChain(kind = "derived", layers = listOf(link), depths = listOf(0)))
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

/** `PullRequestListLine`: one row of a thread's linked pull request list. */
data class PullRequestListLine(
    val link: ThreadPullRequestLinkDto,
    val depth: Int,
    /** The chain's bottom link, `host/repository#number`. */
    val chainKey: String,
    /** Set on a multi-layer chain's first row: (kind, size). */
    val stack: Pair<String, Int>?,
)

/** `pullRequestListLines`: most recently updated chains first, each in its own layer order. */
fun pullRequestListLines(chains: List<ThreadPullRequestChain>): List<PullRequestListLine> =
    chains
        .sortedByDescending { chain -> chain.layers.maxOf { it.latestUpdatedAtMillis() } }
        .flatMap { chain ->
            val bottom = chain.layers.first()
            val chainKey = "${bottom.host}/${bottom.repository}#${bottom.number}"
            chain.layers.mapIndexed { index, link ->
                PullRequestListLine(
                    link = link,
                    depth = chain.depths[index],
                    chainKey = chainKey,
                    stack = if (index == 0 && chain.layers.size > 1) chain.kind to chain.layers.size else null,
                )
            }
        }

/**
 * `useLinkedPullRequestLines`: chains with open work first, then finished
 * ones, each group ordered by [pullRequestListLines].
 */
fun linkedPullRequestLines(links: List<ThreadPullRequestLinkDto>): List<PullRequestListLine> {
    val chains = resolveThreadPullRequestChains(links)
    val (open, done) = chains.partition { chain -> chain.layers.any { it.isOpen() } }
    return pullRequestListLines(open) + pullRequestListLines(done)
}

/** The model link back on the wire shape the chain resolvers read. */
fun ThreadPullRequestLink.toDto(): ThreadPullRequestLinkDto =
    ThreadPullRequestLinkDto(
        host = host,
        repository = repository,
        number = number,
        url = url,
        source = source,
        linkedAt = linkedAt,
        snapshot =
            snapshot?.let {
                ThreadPullRequestSnapshotDto(
                    state = it.state,
                    title = it.title,
                    headBranch = it.headBranch,
                    baseBranch = it.baseBranch,
                    isDraft = it.isDraft,
                    updatedAt = it.updatedAt,
                    syncedAt = it.syncedAt,
                    closedAt = it.closedAt,
                    mergedAt = it.mergedAt,
                    additions = it.additions,
                    deletions = it.deletions,
                    changedFiles = it.changedFiles,
                    reviewDecision = it.reviewDecision,
                    checksState = it.checksState,
                )
            },
        stack =
            stack?.let { stack ->
                ThreadPullRequestStackDto(
                    id = stack.id,
                    number = stack.number,
                    url = stack.url,
                    base = stack.base,
                    layers =
                        stack.layers.map {
                            club.touchtech.s5code.kotlin.transport.wire.ThreadPullRequestStackLayerDto(
                                number = it.number,
                                headBranch = it.headBranch,
                                state = it.state,
                            )
                        },
                )
            },
        watch = if (watched) kotlinx.serialization.json.JsonObject(emptyMap()) else null,
    )
