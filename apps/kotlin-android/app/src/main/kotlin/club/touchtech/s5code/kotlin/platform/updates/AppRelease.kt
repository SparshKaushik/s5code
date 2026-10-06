package club.touchtech.s5code.kotlin.platform.updates

import kotlinx.serialization.Serializable

/**
 * Resolved update artifact for the native Android app.
 */
data class AppRelease(
    val tagName: String,
    val version: SemVer,
    val title: String,
    val notes: String?,
    val apkName: String,
    val apkDownloadUrl: String,
    val apkSizeBytes: Long,
    val sha256DownloadUrl: String? = null,
    val publishedAt: String? = null,
)

@Serializable
data class GitHubReleaseDto(
    val tag_name: String = "",
    val name: String? = null,
    val body: String? = null,
    val draft: Boolean = false,
    val prerelease: Boolean = false,
    val published_at: String? = null,
    val assets: List<GitHubAssetDto> = emptyList(),
)

@Serializable
data class GitHubAssetDto(
    val name: String = "",
    val size: Long = 0,
    val browser_download_url: String = "",
    val content_type: String? = null,
)

/**
 * Filters and parses a list of GitHub releases to find the latest valid release
 * for the Kotlin Android app (`club.touchtech.s5code.kotlin`).
 */
fun findLatestAppRelease(
    releases: List<GitHubReleaseDto>,
    currentVersion: SemVer,
): AppRelease? {
    val candidates = mutableListOf<AppRelease>()

    for (release in releases) {
        if (release.draft) continue

        // Look for the Kotlin APK asset, e.g. "s5code-kotlin-0.1.0-alpha.1-debug.apk"
        val apkAsset = release.assets.firstOrNull { asset ->
            asset.name.endsWith(".apk", ignoreCase = true) &&
                (asset.name.contains("kotlin", ignoreCase = true) || release.tag_name.contains("kotlin", ignoreCase = true))
        } ?: continue

        // Extract version from tag (e.g. "kotlin-android-v0.1.0-alpha.1-debug") or asset name
        val version = SemVer.parse(release.tag_name)
            ?: SemVer.parse(apkAsset.name)
            ?: continue

        // Find companion checksum asset if present (e.g. "s5code-kotlin-...apk.sha256")
        val sha256Asset = release.assets.firstOrNull { asset ->
            asset.name.equals("${apkAsset.name}.sha256", ignoreCase = true) ||
                asset.name.endsWith(".sha256", ignoreCase = true) && asset.name.contains("kotlin", ignoreCase = true)
        }

        candidates += AppRelease(
            tagName = release.tag_name,
            version = version,
            title = release.name ?: release.tag_name,
            notes = release.body,
            apkName = apkAsset.name,
            apkDownloadUrl = apkAsset.browser_download_url,
            apkSizeBytes = apkAsset.size,
            sha256DownloadUrl = sha256Asset?.browser_download_url,
            publishedAt = release.published_at,
        )
    }

    // Sort descending by SemVer
    return candidates
        .sortedByDescending { it.version }
        .firstOrNull { it.version > currentVersion }
}
