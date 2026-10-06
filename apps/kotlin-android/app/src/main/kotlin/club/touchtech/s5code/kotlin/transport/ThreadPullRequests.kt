package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.transport.wire.ThreadLinkedPullRequestDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadPullRequestLinkDto

/**
 * Pull-request link identity, ported from
 * `packages/shared/src/threadPullRequests.ts` and
 * `packages/shared/src/changeRequestUrl.ts`.
 *
 * Links are compared at host level: the same PR linked from two projects (or
 * two checkouts) is one link, and `host`/`repository` are lowercase because the
 * server records them that way. These helpers live beside the wire DTOs rather
 * than in `data/` so the V2 reducers — which cannot depend on `data` — can
 * build `pull-request-watch:` task ids with the same normalization the
 * presentation resolvers use.
 */

/** The normalized `(host, repository, number)` of a link or legacy reference. */
data class ThreadPullRequestKey(
    val host: String,
    val repository: String,
    val number: Int,
)

/**
 * A change request named the way a thread link names one: the host below which
 * the repository is addressed, the repository path as that host writes it, and
 * the number. [authority] carries Forgejo's HTTP `host[:port]`, separate from
 * the portless repository identity.
 */
data class ChangeRequestLink(
    val host: String,
    val repository: String,
    val number: Int,
    val authority: String? = null,
)

/**
 * `canonicalRepositoryKey` in `packages/shared/src/sourceControl.ts`: folds
 * Azure DevOps's SSH and per-organisation remote forms onto the `dev.azure.com`
 * key so the same repository compares equal across remote spellings.
 */
fun canonicalRepositoryKey(key: String): String =
    key
        .replace(
            Regex("^(?:ssh\\.dev\\.azure\\.com|vs-ssh\\.visualstudio\\.com)/v3/([^/]+)/([^/]+)/([^/]+)$"),
            "dev.azure.com/$1/$2/_git/$3",
        )
        .replace(
            Regex("^([^.]+)\\.visualstudio\\.com/(?:defaultcollection/)?([^/]+)/_git/([^/]+)$"),
            "dev.azure.com/$1/$2/_git/$3",
        )

private fun isHostOf(hostname: String, apex: String, label: String? = null): Boolean =
    hostname == apex || hostname.endsWith(".$apex") ||
        (label != null && hostname.split(".").contains(label))

private class ParsedUrl(val host: String, val authority: String, val path: String)

private fun parseHttpUrl(targetUrl: String): ParsedUrl? {
    val uri = runCatching { java.net.URI(targetUrl.trim()) }.getOrNull() ?: return null
    val scheme = uri.scheme?.lowercase()
    if (scheme != "https" && scheme != "http") return null
    // `host` drops the port; `authority` keeps it (minus any userinfo).
    val host = uri.host?.lowercase() ?: return null
    val authority = (uri.authority ?: "").substringAfter('@').lowercase()
    return ParsedUrl(host, authority, uri.rawPath ?: "")
}

private fun claim(host: String, match: MatchResult?): ChangeRequestLink? {
    val repository = match?.groupValues?.getOrNull(1) ?: return null
    val number = match.groupValues.getOrNull(2)?.toIntOrNull() ?: return null
    return if (number > 0) ChangeRequestLink(host, repository.lowercase(), number) else null
}

/**
 * The repository and number behind a change-request URL on a host this can
 * read, or null for anything else — an issue, a commit, a host this cannot tell
 * apart from an ordinary link. A doubtful match is worse than no match, so
 * nothing here guesses. Ported pattern-for-pattern from `parseChangeRequestUrl`.
 */
fun parseChangeRequestUrl(targetUrl: String): ChangeRequestLink? {
    val url = parseHttpUrl(targetUrl) ?: return null
    val host = url.host
    val path = url.path

    // GitHub, and any Enterprise install: /{owner}/{repo}/pull/{n}
    if (isHostOf(host, "github.com", "github")) {
        claim(host, Regex("^/([^/]+/[^/]+)/pull/(\\d+)(?:/|$)").find(path))?.let { return it }
    }
    // Forgejo and Gitea use /pulls/ on arbitrary self-hosted domains.
    Regex("^/([^/]+(?:/[^/]+)+)/pulls/(\\d+)(?:/|$)").find(path)?.let { match ->
        val link = claim(host, match) ?: return null
        return link.copy(authority = url.authority)
    }
    // GitLab, self-hosted included: /{group}/[{subgroup}/...]{repo}/-/merge_requests/{n}.
    Regex("^/([^/]+(?:/[^/]+)+)/-/merge_requests/(\\d+)(?:/|$)").find(path)?.let { match ->
        return claim(host, match)
    }
    // Bitbucket Cloud: /{workspace}/{repo}/pull-requests/{n}
    if (isHostOf(host, "bitbucket.org", "bitbucket")) {
        return claim(host, Regex("^/([^/]+/[^/]+)/pull-requests/(\\d+)(?:/|$)").find(path))
    }
    // Azure DevOps, both the current host and the per-organisation one it
    // replaced. `_git` stays part of the repository path, as in the remote URL.
    if (isHostOf(host, "dev.azure.com") || host.endsWith(".visualstudio.com")) {
        return claim(host, Regex("^/((?:[^/]+/)*_git/[^/]+)/pullrequest/(\\d+)(?:/|$)").find(path))
    }
    return null
}

/**
 * `normalizeThreadPullRequestKey`: folds a stored link onto the canonical
 * `host/repository#number` key, recovering Forgejo HTTP ports from the URL the
 * link recorded — the stored `host` is the portless repository host, while the
 * watch and PR lookups address the web origin.
 */
fun normalizeThreadPullRequestKey(
    host: String,
    repository: String,
    number: Int,
    url: String? = null,
    authority: String? = null,
): ThreadPullRequestKey {
    val parsed = url?.let(::parseChangeRequestUrl)
    val resolvedAuthority = authority
        ?: parsed?.takeIf {
            it.repository == repository.trim().lowercase() && it.number == number
        }?.authority
    val canonical = canonicalRepositoryKey(
        "${(resolvedAuthority ?: host).trim().lowercase()}/${repository.trim().lowercase()}",
    )
    val separator = canonical.indexOf("/")
    return ThreadPullRequestKey(
        host = if (separator >= 0) canonical.substring(0, separator) else canonical,
        repository = if (separator >= 0) canonical.substring(separator + 1) else "",
        number = number,
    )
}

fun normalizeThreadPullRequestKey(link: ThreadPullRequestLinkDto): ThreadPullRequestKey =
    normalizeThreadPullRequestKey(link.host, link.repository, link.number, link.url)

/** `threadPullRequestKeyOf`: the `<host>/<repository>#<number>` comparison key. */
fun threadPullRequestKeyOf(
    host: String,
    repository: String,
    number: Int,
    url: String? = null,
    authority: String? = null,
): String = normalizeThreadPullRequestKey(host, repository, number, url, authority)
    .let { "${it.host}/${it.repository}#${it.number}" }

fun threadPullRequestKeyOf(link: ThreadPullRequestLinkDto): String =
    normalizeThreadPullRequestKey(link).let { "${it.host}/${it.repository}#${it.number}" }

/**
 * `legacyThreadPullRequestKey`: legacy single-link selectors omit the host
 * (and, on Azure, the organisation and project); recover them from the stored
 * URL, falling back to the URL's bare hostname when nothing else parses.
 */
fun legacyThreadPullRequestKey(
    linked: ThreadLinkedPullRequestDto,
    fallbackHost: String? = null,
): ThreadPullRequestKey {
    val parsed = parseChangeRequestUrl(linked.url)
    if (parsed != null && parsed.number == linked.number) {
        val canonical = canonicalRepositoryKey("${parsed.host}/${parsed.repository}")
        if (parsed.authority != null || canonical.startsWith("dev.azure.com/")) {
            return normalizeThreadPullRequestKey(
                parsed.host, parsed.repository, parsed.number,
                authority = parsed.authority,
            )
        }
    }
    val host = fallbackHost
        ?: runCatching { java.net.URI(linked.url).host }.getOrNull()
        ?: "unknown"
    return ThreadPullRequestKey(
        host = host.trim().lowercase().ifEmpty { "unknown" },
        repository = linked.repository.trim().lowercase(),
        number = linked.number,
    )
}

/**
 * `threadPullRequestsOf`: older V2 records stored one `linkedPullRequest` and
 * never carried the array; an explicit empty array means "unlinked", so the
 * fallback only applies when the array field was absent (null here).
 */
fun threadPullRequestsOf(
    pullRequests: List<ThreadPullRequestLinkDto>?,
    linkedPullRequest: ThreadLinkedPullRequestDto?,
): List<ThreadPullRequestLinkDto> {
    if (pullRequests != null) return pullRequests
    val linked = linkedPullRequest ?: return emptyList()
    val key = legacyThreadPullRequestKey(linked)
    return listOf(
        ThreadPullRequestLinkDto(
            host = key.host,
            repository = key.repository,
            number = linked.number,
            url = linked.url,
            source = "manual",
            linkedAt = "1970-01-01T00:00:00.000Z",
        ),
    )
}

/** `visibleThreadPullRequests`: tombstoned stack members never render. */
fun visibleThreadPullRequests(
    links: List<ThreadPullRequestLinkDto>,
): List<ThreadPullRequestLinkDto> = links.filter { it.source != "stack-dismissed" }
