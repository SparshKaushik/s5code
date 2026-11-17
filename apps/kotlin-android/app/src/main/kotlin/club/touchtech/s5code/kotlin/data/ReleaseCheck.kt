package club.touchtech.s5code.kotlin.data

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import okhttp3.OkHttpClient
import okhttp3.Request
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * `cliRelease.ts` + `semver.ts` from `packages/shared`: the GitHub release
 * check behind "Check for updates" on the environment detail screen.
 *
 * The check reads the public release pages first and only walks GitHub's
 * releases API newest-first when they miss the channel, and it answers on the
 * same release channel the server is already on — a stable server is never
 * offered a nightly tag.
 */

private const val RELEASES_PAGE = "https://github.com/SparshKaushik/s5code/releases"

private const val RELEASES_API =
    "https://api.github.com/repos/SparshKaushik/s5code/releases?per_page=100&page="

/** The release train a version was published on, from its prerelease tag. */
private fun releaseChannelOf(version: String): String {
    val channel =
        Regex("^[^-+]+-(nightly|preview)\\.\\d{8}\\.\\d+")
            .find(version)
            ?.groupValues?.get(1)
    return if (channel == "nightly" || channel == "preview") channel else "stable"
}

/** `compareSemverVersions`: numeric major/minor/patch, semver prerelease order. */
fun compareSemverVersions(left: String, right: String): Int {
    val parsedLeft = parseSemver(left)
    val parsedRight = parseSemver(right)
    if (parsedLeft == null || parsedRight == null) return left.compareTo(right)
    if (parsedLeft.major != parsedRight.major) return parsedLeft.major - parsedRight.major
    if (parsedLeft.minor != parsedRight.minor) return parsedLeft.minor - parsedRight.minor
    if (parsedLeft.patch != parsedRight.patch) return parsedLeft.patch - parsedRight.patch
    val a = parsedLeft.prerelease
    val b = parsedRight.prerelease
    if (a.isEmpty() && b.isEmpty()) return 0
    if (a.isEmpty()) return 1
    if (b.isEmpty()) return -1
    val length = maxOf(a.size, b.size)
    for (index in 0 until length) {
        val x = a.getOrNull(index) ?: return -1
        val y = b.getOrNull(index) ?: return 1
        val comparison = comparePrereleaseIdentifier(x, y)
        if (comparison != 0) return comparison
    }
    return 0
}

private class ParsedSemver(
    val major: Int,
    val minor: Int,
    val patch: Int,
    val prerelease: List<String>,
)

private fun parseSemver(value: String): ParsedSemver? {
    // `normalizeSemverVersion`: pad shorthand versions ("20" or "20.1") to
    // three segments; anything else keeps its shape so garbage still fails.
    val unprefixed = value.trim().removePrefix("v")
    val dashIndex = unprefixed.indexOf('-')
    val main = if (dashIndex >= 0) unprefixed.substring(0, dashIndex) else unprefixed
    val prerelease = if (dashIndex >= 0) unprefixed.substring(dashIndex + 1) else null
    var segments = main.split(".").map { it.trim() }.filter { it.isNotEmpty() }
    if (segments.isEmpty()) return null
    if (segments.size < 3) segments = segments + List(3 - segments.size) { "0" }
    if (segments.size != 3) return null
    return parseSemverParts(segments, prerelease)
}

private fun parseSemverParts(segments: List<String>, prerelease: String?): ParsedSemver? {
    if (segments.any { !it.all(Char::isDigit) }) return null
    val (major, minor, patch) = segments.map { it.toIntOrNull() ?: return null }
    return ParsedSemver(
        major = major,
        minor = minor,
        patch = patch,
        prerelease =
            prerelease?.split(".")?.map { it.trim() }?.filter { it.isNotEmpty() } ?: emptyList(),
    )
}

private fun comparePrereleaseIdentifier(left: String, right: String): Int {
    val leftNumeric = left.all(Char::isDigit)
    val rightNumeric = right.all(Char::isDigit)
    return when {
        leftNumeric && rightNumeric -> (left.toLongOrNull() ?: 0).compareTo(right.toLongOrNull() ?: 0)
        leftNumeric -> -1
        rightNumeric -> 1
        else -> left.compareTo(right)
    }
}

@Serializable internal class ReleaseEntry(val tag_name: String = "", val draft: Boolean = false)

private val releasesJson = Json { ignoreUnknownKeys = true }

private val ATOM_RELEASE_TAG = Regex("<id>tag:github\\.com,2008:Repository/\\d+/([^<]+)</id>")

/** Release tags from the public releases Atom feed, newest first. */
internal fun releaseTagsFromAtom(feed: String): List<ReleaseEntry> =
    ATOM_RELEASE_TAG.findAll(feed).map { ReleaseEntry(tag_name = it.groupValues[1]) }.toList()

/**
 * The newest release on the server's channel that is newer than it, or null
 * when it is current. Throws when the release index itself cannot be read —
 * that is a different failure from "you are up to date".
 */
suspend fun findEnvironmentUpdate(http: OkHttpClient, currentVersion: String): String? =
    findEnvironmentUpdate(currentVersion) { url, accept ->
        withContext(Dispatchers.IO) {
            http.newCall(Request.Builder().url(url).header("Accept", accept).build())
                .execute()
                .use { response ->
                    if (!response.isSuccessful) {
                        throw ReleaseCheckError("Could not check releases (${response.code}). Try again.")
                    }
                    response.body?.string().orEmpty()
                }
        }
    }

internal suspend fun findEnvironmentUpdate(
    currentVersion: String,
    fetch: suspend (url: String, accept: String) -> String,
): String? {
    val channel = releaseChannelOf(currentVersion)
    // Public release pages avoid the anonymous API quota shared by devices on
    // the same network.
    val releases =
        if (channel == "stable") {
            listOf(releasesJson.decodeFromString<ReleaseEntry>(fetch("$RELEASES_PAGE/latest", "application/json")))
        } else {
            releaseTagsFromAtom(fetch("$RELEASES_PAGE.atom", "application/atom+xml"))
        }
    var version = newestCliReleaseVersion(releases, channel)
    // The feed only includes ten releases, so later nightlies can displace a
    // published preview.
    var page = 1
    while (version == null) {
        val pageReleases =
            releasesJson.decodeFromString<List<ReleaseEntry>>(
                fetch("$RELEASES_API$page", "application/vnd.github+json")
            )
        version = newestCliReleaseVersion(pageReleases, channel)
        if (pageReleases.size < 100) break
        page += 1
    }
    if (version == null) throw ReleaseCheckError("No $channel release was found.")
    return if (compareSemverVersions(version, currentVersion) > 0) version else null
}

/** `newestCliReleaseVersion`: first non-draft `v<semver>` tag on the channel. */
private fun newestCliReleaseVersion(releases: List<ReleaseEntry>, channel: String): String? {
    for (release in releases) {
        if (release.draft) continue
        val version =
            Regex("^v(\\d+\\.\\d+\\.\\d+(?:-[0-9A-Za-z.-]+)?)")
                .find(release.tag_name)
                ?.groupValues?.get(1)
                ?: continue
        if (releaseChannelOf(version) == channel) return version
    }
    return null
}

class ReleaseCheckError(message: String) : Exception(message)
