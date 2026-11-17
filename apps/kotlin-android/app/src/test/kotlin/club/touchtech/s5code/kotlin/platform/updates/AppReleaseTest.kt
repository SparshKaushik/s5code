package club.touchtech.s5code.kotlin.platform.updates

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class AppReleaseTest {

    private val currentVersion = SemVer(0, 1, 0, "alpha.1")

    @Test
    fun `finds latest Kotlin release ignoring RN only releases and drafts`() {
        val releases = listOf(
            GitHubReleaseDto(
                tag_name = "v0.2.12",
                name = "S5 Code v0.2.12",
                assets = listOf(
                    GitHubAssetDto(name = "s5code-0.2.12.apk", browser_download_url = "https://example.com/s5code-0.2.12.apk", size = 190000000),
                ),
            ),
            GitHubReleaseDto(
                tag_name = "kotlin-android-v0.1.0-alpha.2-debug",
                name = "Kotlin Android client 0.1.0-alpha.2 (debug)",
                body = "Second alpha release with bug fixes",
                assets = listOf(
                    GitHubAssetDto(
                        name = "s5code-kotlin-0.1.0-alpha.2-debug.apk",
                        browser_download_url = "https://example.com/s5code-kotlin-0.1.0-alpha.2-debug.apk",
                        size = 68000000,
                    ),
                    GitHubAssetDto(
                        name = "s5code-kotlin-0.1.0-alpha.2-debug.apk.sha256",
                        browser_download_url = "https://example.com/s5code-kotlin-0.1.0-alpha.2-debug.apk.sha256",
                        size = 104,
                    ),
                ),
            ),
            GitHubReleaseDto(
                tag_name = "kotlin-android-v0.1.0-alpha.1-debug",
                name = "Kotlin Android client 0.1.0-alpha.1",
                assets = listOf(
                    GitHubAssetDto(
                        name = "s5code-kotlin-0.1.0-alpha.1-debug.apk",
                        browser_download_url = "https://example.com/s5code-kotlin-0.1.0-alpha.1-debug.apk",
                        size = 68000000,
                    ),
                ),
            ),
        )

        val latest = findLatestAppRelease(releases, currentVersion)
        assertNotNull(latest)
        assertEquals("kotlin-android-v0.1.0-alpha.2-debug", latest?.tagName)
        assertEquals(SemVer(0, 1, 0, "alpha.2"), latest?.version)
        assertEquals("s5code-kotlin-0.1.0-alpha.2-debug.apk", latest?.apkName)
        assertEquals("https://example.com/s5code-kotlin-0.1.0-alpha.2-debug.apk", latest?.apkDownloadUrl)
        assertEquals("https://example.com/s5code-kotlin-0.1.0-alpha.2-debug.apk.sha256", latest?.sha256DownloadUrl)
    }

    @Test
    fun `returns null if no newer release exists`() {
        val releases = listOf(
            GitHubReleaseDto(
                tag_name = "kotlin-android-v0.1.0-alpha.1-debug",
                name = "Kotlin Android client 0.1.0-alpha.1",
                assets = listOf(
                    GitHubAssetDto(
                        name = "s5code-kotlin-0.1.0-alpha.1-debug.apk",
                        browser_download_url = "https://example.com/s5code-kotlin-0.1.0-alpha.1-debug.apk",
                        size = 68000000,
                    ),
                ),
            ),
        )

        val latest = findLatestAppRelease(releases, currentVersion)
        assertNull(latest)
    }

    @Test
    fun `discards draft releases`() {
        val releases = listOf(
            GitHubReleaseDto(
                tag_name = "kotlin-android-v0.2.0",
                draft = true,
                assets = listOf(
                    GitHubAssetDto(
                        name = "s5code-kotlin-0.2.0.apk",
                        browser_download_url = "https://example.com/s5code-kotlin-0.2.0.apk",
                        size = 68000000,
                    ),
                ),
            ),
        )

        val latest = findLatestAppRelease(releases, currentVersion)
        assertNull(latest)
    }
}
