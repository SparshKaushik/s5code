package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ThreadSummary
import club.touchtech.s5code.kotlin.transport.wire.ThreadLinkedPullRequestDto

/**
 * This client's thread opens, mirroring `claimThreadOpen` and the local-open
 * half of `threadSearchSubject` in `packages/client-runtime/src/state/threadSearch.ts`.
 *
 * Opens are tracked by this device's clock: a server timestamp would be off by
 * any skew. The server merges opens closer together than
 * [THREAD_OPEN_DEDUPE_MS] anyway, so skipping them saves a command and a
 * broadcast while flipping between threads; the local record still lets
 * search follow quick switches.
 */
object ThreadOpens {
    private val sent = HashMap<String, Long>()
    private val local = HashMap<String, Long>()

    private fun key(environmentId: String, threadId: String) = "$environmentId:$threadId"

    /** Records that this client opened a thread, and says whether to send the open now. */
    @Synchronized
    fun claim(environmentId: String, threadId: String, nowMillis: Long): Boolean {
        val key = key(environmentId, threadId)
        local[key] = nowMillis
        val last = sent[key]
        if (last != null && nowMillis - last < THREAD_OPEN_DEDUPE_MS) return false
        sent[key] = nowMillis
        return true
    }

    @Synchronized
    fun localOpenedAt(environmentId: String, threadId: String): Long? = local[key(environmentId, threadId)]
}

private const val SEARCHED_PULL_REQUEST_TITLES = 3

/**
 * `threadSearchPullRequests` in `packages/shared/src/threadPullRequests.ts`:
 * every pull request matches by number and link, but only the newest few by
 * title, since a triage thread linked to dozens would match almost any word.
 */
fun threadSearchPullRequests(thread: ThreadSummary): List<ThreadSearchPullRequest> {
    fun entry(number: Int, repository: String, url: String, title: String?) =
        ThreadSearchPullRequest(number, title, listOf("$repository#$number", url))
    fun legacy(link: ThreadLinkedPullRequestDto) = entry(link.number, link.repository, link.url, null)
    val branch = listOfNotNull(thread.branchPullRequest?.let(::legacy))
    val links = thread.pullRequestLinks
    if (!links.isNullOrEmpty()) {
        val newestFirst =
            links
                .filter { it.source != "stack-dismissed" }
                .sortedByDescending { parseInstant(it.snapshot?.updatedAt ?: it.linkedAt) ?: 0 }
        return branch +
            newestFirst.mapIndexed { index, link ->
                entry(
                    link.number,
                    link.repository,
                    link.url,
                    link.snapshot?.title?.takeIf { index < SEARCHED_PULL_REQUEST_TITLES && it.isNotEmpty() },
                )
            }
    }
    return branch + listOfNotNull(thread.linkedPullRequest?.let(::legacy))
}

/**
 * `threadSearchSubject`: what the home list ranks a thread by. Activity is
 * what the user wrote, not agent wakes; a thread the user never wrote in counts
 * from its creation, and an older server that omits the field falls back to
 * the latest update. This client's own latest open counts too, since the
 * server merged it or has not recorded it yet.
 */
fun threadSearchSubject(
    thread: ThreadSummary,
    projectTitle: String? = null,
    environmentLabel: String? = null,
): ThreadSearchSubject {
    val activityAt =
        if (thread.latestUserAuthoredMessageAtKnown) {
            thread.latestUserAuthoredMessageAtMillis ?: thread.createdAtMillis
        } else {
            thread.updatedAtMillis
        }
    val localOpenedAt = ThreadOpens.localOpenedAt(thread.environmentId.value, thread.id.value) ?: 0
    val serverOpens = thread.opens
    return ThreadSearchSubject(
        id = thread.id.value,
        title = thread.title,
        branch = thread.branch,
        projectTitle = projectTitle,
        environmentLabel = environmentLabel,
        pullRequests = threadSearchPullRequests(thread),
        activityAtMillis = activityAt.takeIf { it > 0 },
        opens =
            if (localOpenedAt > (serverOpens?.openedAtMillis ?: 0)) {
                serverOpens?.copy(openedAtMillis = localOpenedAt)
                    ?: club.touchtech.s5code.kotlin.model.ThreadOpenStats(localOpenedAt, 1.0)
            } else {
                serverOpens
            },
    )
}
