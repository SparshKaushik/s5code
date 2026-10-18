package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.RepositoryIdentity
import club.touchtech.s5code.kotlin.transport.canonicalRepositoryKey
import club.touchtech.s5code.kotlin.transport.parseChangeRequestUrl
import java.net.URI

/*
 * The "Link pull request" prompt, ported from
 * `packages/client-runtime/src/pullRequestLinkInput.ts` and the per-host
 * `changeRequestUrl`/`checkoutCommandArgument`/`isChangeRequestReference`
 * entries of the source-control client definitions.
 */

private val CHECKOUT_COMMANDS =
    listOf(
        Regex("^gh\\s+pr\\s+checkout\\s+(.+)$", RegexOption.IGNORE_CASE),
        Regex("^glab\\s+mr\\s+checkout\\s+(.+)$", RegexOption.IGNORE_CASE),
        Regex("^tea\\s+(?:pr|pulls)\\s+checkout\\s+(.+)$", RegexOption.IGNORE_CASE),
    )
private val AZURE_CHECKOUT_COMMAND = Regex("^az\\s+repos\\s+pr\\s+checkout\\s+(.+)$", RegexOption.IGNORE_CASE)

private val CHANGE_REQUEST_REFERENCES =
    listOf(
        Regex("^https://github\\.com/[^/\\s]+/[^/\\s]+/pull/(\\d+)(?:[/?#].*)?$", RegexOption.IGNORE_CASE),
        Regex("^https://[^/\\s]*gitlab[^/\\s]*/.+/-/merge_requests/(\\d+)(?:[/?#].*)?$", RegexOption.IGNORE_CASE),
        Regex("^https?://[^/\\s]+/(?:[^/\\s]+/)+[^/\\s]+/pulls/(\\d+)(?:[/?#].*)?$", RegexOption.IGNORE_CASE),
        Regex(
            "^https://(?:dev\\.azure\\.com/[^/\\s]+/[^/\\s]+|[^/\\s]+\\.visualstudio\\.com/[^/\\s]+)" +
                "/_git/[^/\\s]+/pullrequest/(\\d+)(?:[/?#].*)?$",
            RegexOption.IGNORE_CASE,
        ),
    )
private val PULL_REQUEST_NUMBER = Regex("^#?(\\d+)$")

/** `checkoutCommandId` for `az repos pr checkout`. */
private fun azureCheckoutId(args: String): String? {
    val parts = args.trim().split(Regex("\\s+")).filter(String::isNotEmpty)
    parts.forEachIndexed { index, part ->
        if (part == "--id" || part == "-i") return parts.getOrNull(index + 1)
        if (part.startsWith("--id=")) return part.removePrefix("--id=").ifEmpty { null }
    }
    return parts.firstOrNull { !it.startsWith("-") }
}

/**
 * `parsePullRequestReference`: a change-request URL, or a bare number, after
 * unwrapping a host CLI's checkout command.
 */
fun parsePullRequestReference(input: String): String? {
    val trimmed = input.trim()
    if (trimmed.isEmpty()) return null
    val normalized =
        CHECKOUT_COMMANDS.firstNotNullOfOrNull { it.find(trimmed)?.groupValues?.get(1)?.trim() }
            ?: AZURE_CHECKOUT_COMMAND.find(trimmed)?.groupValues?.get(1)?.let(::azureCheckoutId)
            ?: trimmed
    if (normalized.isEmpty()) return null
    if (CHANGE_REQUEST_REFERENCES.any { it.matches(normalized) }) return normalized
    return PULL_REQUEST_NUMBER.find(normalized)?.groupValues?.get(1)
}

private fun httpUri(value: String?): URI? =
    runCatching { URI(value.orEmpty()) }.getOrNull()?.takeIf {
        (it.scheme == "http" || it.scheme == "https") && !it.host.isNullOrEmpty()
    }

private fun URI.authorityHost(): String = if (port == -1) host else "$host:$port"

/** `pullRequestHostOf`: Forgejo keeps an HTTP remote's port; the rest use the canonical key's host. */
fun pullRequestHostOf(identity: RepositoryIdentity?, kind: String): String {
    if (kind == "forgejo") {
        httpUri(identity?.remoteUrl)?.let { return it.authorityHost().lowercase() }
    }
    val host = identity?.canonicalKey?.split("/")?.firstOrNull()?.trim()
    return if (host.isNullOrEmpty()) kind else host.lowercase()
}

/** Each host's `changeRequestUrl`; null for a host with no known pull request page. */
fun changeRequestUrlFor(
    kind: String,
    host: String,
    repository: String,
    number: Int,
    remoteUrl: String? = null,
    webUrl: String? = null,
): String? =
    when (kind) {
        "github" -> "https://$host/$repository/pull/$number"
        "gitlab" -> "https://$host/$repository/-/merge_requests/$number"
        "gitcafe" -> "https://$host/$repository/pulls/$number"
        "forgejo" -> {
            val remote = httpUri(remoteUrl)
            when {
                !webUrl.isNullOrEmpty() -> "${webUrl.trimEnd('/')}/pulls/$number"
                remote != null &&
                    (remote.host.equals(host, ignoreCase = true) || remote.authorityHost().equals(host, ignoreCase = true)) ->
                    "${remote.scheme}://${remote.authorityHost()}/$repository/pulls/$number"
                else -> "https://$host/$repository/pulls/$number"
            }
        }
        "bitbucket" -> "https://$host/$repository/pull-requests/$number"
        "azure-devops" -> "https://${canonicalRepositoryKey("$host/$repository".lowercase())}/pullrequest/$number"
        else -> null
    }

/** `pullRequestLinkProject`: the thread project's repository, so a bare number names one of its requests. */
data class PullRequestLinkProject(
    val host: String,
    val repository: String,
    val webUrl: (Int) -> String?,
)

private val KNOWN_PROVIDERS = setOf("github", "gitlab", "gitcafe", "forgejo", "bitbucket", "azure-devops")

fun pullRequestLinkProject(identity: RepositoryIdentity?): PullRequestLinkProject? {
    identity ?: return null
    val repository =
        identity.displayName
            ?: (if (identity.owner != null && identity.name != null) "${identity.owner}/${identity.name}" else null)
            ?: return null
    // An unreported or unknown provider has no definition, so no pull request URL.
    val kind = identity.provider?.takeIf { it in KNOWN_PROVIDERS } ?: "unknown"
    val host = pullRequestHostOf(identity, kind)
    return PullRequestLinkProject(host, repository) { number ->
        changeRequestUrlFor(kind, host, repository, number, identity.remoteUrl, identity.webUrl)
    }
}

data class ResolvedPullRequestLink(val host: String, val repository: String, val number: Int, val url: String)

sealed interface PullRequestLinkInputResult {
    data class Link(val link: ResolvedPullRequestLink) : PullRequestLinkInputResult

    data class Error(val message: String) : PullRequestLinkInputResult
}

/** `resolveLinkPullRequestInput`; null when the input names no pull request at all. */
fun resolveLinkPullRequestInput(
    reference: String,
    project: PullRequestLinkProject?,
    hasProject: (ResolvedPullRequestLink) -> Boolean = { true },
): PullRequestLinkInputResult? {
    val trimmed = reference.trim()
    val parsed = if (parseChangeRequestUrl(trimmed) != null) trimmed else parsePullRequestReference(reference)
    parsed ?: return null
    parseChangeRequestUrl(parsed)?.let { url ->
        val link = ResolvedPullRequestLink(url.host, url.repository, url.number, parsed)
        if (!hasProject(link)) {
            return PullRequestLinkInputResult.Error("No project in this environment can read ${url.host}/${url.repository}.")
        }
        return PullRequestLinkInputResult.Link(link)
    }
    val number = parsed.toIntOrNull()?.takeIf { it >= 1 } ?: return null
    project ?: return PullRequestLinkInputResult.Error("Paste a full URL to link a pull request from another repository.")
    val webUrl = project.webUrl(number)
    val webReference = webUrl?.let(::parseChangeRequestUrl)
    if (webUrl == null || webReference == null) {
        return PullRequestLinkInputResult.Error("Paste a full URL; this project's host has no known pull request URL.")
    }
    return PullRequestLinkInputResult.Link(
        ResolvedPullRequestLink(webReference.host, webReference.repository, webReference.number, webUrl),
    )
}
