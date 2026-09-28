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
 * The check walks GitHub's releases API newest-first because a busy nightly
 * train can push the newest stable past one page, and it answers on the same
 * release channel the server is already on — a stable server is never offered
 * a nightly tag.
 */

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

@Serializable private class ReleaseEntry(val tag_name: String = "", val draft: Boolean = false)

private val releasesJson = Json { ignoreUnknownKeys = true }

/**
 * The newest release on the server's channel that is newer than it, or null
 * when it is current. Throws when the release index itself cannot be read —
 * that is a different failure from "you are up to date".
 */
suspend fun findEnvironmentUpdate(http: OkHttpClient, currentVersion: String): String? {
    val channel = releaseChannelOf(currentVersion)
    var page = 1
    while (true) {
        val body =
            withContext(Dispatchers.IO) {
                http.newCall(Request.Builder().url("$RELEASES_API$page").build())
                    .execute()
                    .use { response ->
                        if (!response.isSuccessful) {
                            throw ReleaseCheckError(
                                "Could not check releases (${response.code}). Try again."
                            )
                        }
                        response.body?.string().orEmpty()
                    }
            }
        val releases =
            releasesJson.decodeFromString<List<ReleaseEntry>>(body)
        val version = newestCliReleaseVersion(releases, channel)
        if (version != null) {
            return if (compareSemverVersions(version, currentVersion) > 0) version else null
        }
        if (releases.size < 100) {
            throw ReleaseCheckError("No $channel release was found.")
        }
        page += 1
    }
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
