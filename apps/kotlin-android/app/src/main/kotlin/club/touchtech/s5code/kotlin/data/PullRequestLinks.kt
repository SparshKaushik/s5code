package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.RepositoryIdentity
import club.touchtech.s5code.kotlin.transport.ChangeRequestLink
import club.touchtech.s5code.kotlin.transport.canonicalRepositoryKey
import club.touchtech.s5code.kotlin.transport.parseChangeRequestUrl

/*
 * Where a change request link opens in the app, ported from client-runtime's
 * `changeRequestLinks.ts` and the per-host `isChangeRequestInRepository` /
 * `canReadChangeRequestOnHost` rules of the source-control definitions.
 */

/** A project a link may open under. */
data class ChangeRequestLinkProject(
    val environmentId: String,
    val projectId: String,
    val identity: RepositoryIdentity?,
)

/** `ChangeRequestLinkCapabilities`: what the project's environment can show. */
data class ChangeRequestLinkCapabilities(val pullRequests: Boolean, val threadPullRequests: Boolean)

data class ChangeRequestLinkTarget(val environmentId: String, val reference: PullRequestReference)

/** `isChangeRequestAuthorityOfProject`: an HTTP remote pins the port a Forgejo link names. */
private fun isChangeRequestAuthorityOfProject(identity: RepositoryIdentity, link: ChangeRequestLink): Boolean {
    val authority = link.authority ?: return true
    val remote = identity.remoteUrl?.let { runCatching { java.net.URI(it.trim()) }.getOrNull() }
    val scheme = remote?.scheme?.lowercase()
    // SSH remotes do not specify the server's HTTP port.
    if (scheme != "http" && scheme != "https") return true
    return remote.authority?.substringAfter('@')?.lowercase() == authority
}

private fun projectHost(identity: RepositoryIdentity): String? =
    identity.canonicalKey.substringBefore('/').trim().lowercase().takeIf(String::isNotEmpty)

private fun isOnProjectHost(identity: RepositoryIdentity, link: ChangeRequestLink): Boolean {
    if (!isChangeRequestAuthorityOfProject(identity, link)) return false
    val host = projectHost(identity) ?: return false
    return host == link.host.lowercase() || host == link.authority
}

/** `defaultRepositorySelector`: the repository path the host's RPCs take. */
fun repositorySelector(identity: RepositoryIdentity?): String? {
    identity ?: return null
    identity.displayName?.let { return it }
    return if (identity.owner != null && identity.name != null) "${identity.owner}/${identity.name}" else null
}

/** Forgejo's server-resolved web URL as (host, trimmed path), when the identity carries one. */
private fun forgejoWeb(identity: RepositoryIdentity): Pair<String, String>? {
    val web = identity.webUrl?.let { runCatching { java.net.URI(it.trim()) }.getOrNull() } ?: return null
    val host = web.authority?.substringAfter('@')?.lowercase() ?: return null
    return host to web.path.orEmpty().trim('/').lowercase()
}

/** `isChangeRequestInRepository`: the project's own repository holds the change request. */
fun isChangeRequestInRepository(identity: RepositoryIdentity, link: ChangeRequestLink): Boolean =
    when (identity.provider) {
        "azure-devops" -> {
            val projectKey = identity.canonicalKey.lowercase()
            isChangeRequestAuthorityOfProject(identity, link) &&
                canonicalRepositoryKey(projectKey) == canonicalRepositoryKey("${link.host}/${link.repository}".lowercase())
        }
        "forgejo" -> {
            val web = forgejoWeb(identity)
            if (web == null) {
                isGenericInRepository(identity, link)
            } else {
                isChangeRequestAuthorityOfProject(identity, link) &&
                    web.first == (link.authority ?: link.host).lowercase() && web.second == link.repository.lowercase()
            }
        }
        else -> isGenericInRepository(identity, link)
    }

private fun isGenericInRepository(identity: RepositoryIdentity, link: ChangeRequestLink): Boolean {
    val repository = repositorySelector(identity) ?: return false
    return repository.equals(link.repository, ignoreCase = true) && isOnProjectHost(identity, link)
}

/** `canReadChangeRequestOnHost`: the project's server can read other repositories on the link's host. */
fun canReadChangeRequestOnHost(identity: RepositoryIdentity, link: ChangeRequestLink): Boolean =
    when (identity.provider) {
        "azure-devops" -> false
        "forgejo" -> {
            val web = forgejoWeb(identity)
            if (web == null) {
                isOnProjectHost(identity, link)
            } else {
                // A server mounted below a path serves only repositories under that mount.
                val mount = web.second.split('/').dropLast(2).joinToString("/")
                web.first == (link.authority ?: link.host).lowercase() &&
                    (mount.isEmpty() || link.repository.lowercase().startsWith("$mount/"))
            }
        }
        else -> isOnProjectHost(identity, link)
    }

/**
 * `resolveChangeRequestLinkTarget`: where a change request link opens in the
 * app, or null for the browser. A link may borrow any project on its host when
 * the server reads change requests by host (`threadPullRequests`), so a
 * mirror's or a sibling repository's link still opens in the app.
 */
fun resolveChangeRequestLinkTarget(
    url: String,
    projects: List<ChangeRequestLinkProject>,
    capabilities: (environmentId: String) -> ChangeRequestLinkCapabilities?,
    /** The link's own environment; without one, any environment that reads change requests. */
    environmentId: String? = null,
    /** The project the link came from, tried first. */
    projectId: String? = null,
    primaryEnvironmentId: String? = null,
    lendHostProject: Boolean = true,
): ChangeRequestLinkTarget? {
    val link = parseChangeRequestUrl(url) ?: return null
    // Matching remotes do not make project ids usable outside the link's environment.
    val candidates =
        projects
            .filter { project ->
                if (environmentId == null) capabilities(project.environmentId)?.pullRequests == true
                else project.environmentId == environmentId
            }
            .sortedWith(
                compareByDescending<ChangeRequestLinkProject> { it.projectId == projectId }
                    .thenByDescending { it.environmentId == primaryEnvironmentId }
            )
    val project =
        candidates.firstOrNull { candidate -> candidate.identity?.let { isChangeRequestInRepository(it, link) } == true }
            ?: if (lendHostProject) {
                val lenders = candidates.filter { capabilities(it.environmentId)?.threadPullRequests == true }
                lenders.firstOrNull { candidate -> candidate.identity?.let { isChangeRequestInRepository(it, link) } == true }
                    ?: lenders.firstOrNull { candidate -> candidate.identity?.let { canReadChangeRequestOnHost(it, link) } == true }
            } else {
                null
            }
    project ?: return null
    val projectCapabilities = capabilities(project.environmentId)
    if (projectCapabilities?.pullRequests != true) return null
    val reference =
        if (projectCapabilities.threadPullRequests) {
            PullRequestReference(projectId = project.projectId, repository = link.repository, number = link.number, host = link.authority ?: link.host)
        } else {
            PullRequestReference(
                projectId = project.projectId,
                repository = repositorySelector(project.identity) ?: link.repository,
                number = link.number,
            )
        }
    return ChangeRequestLinkTarget(project.environmentId, reference)
}
