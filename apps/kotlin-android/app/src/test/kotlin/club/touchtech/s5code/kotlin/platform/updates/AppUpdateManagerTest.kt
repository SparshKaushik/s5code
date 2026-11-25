package club.touchtech.s5code.kotlin.platform.updates

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import okhttp3.OkHttpClient
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.File
import java.nio.file.Files

@OptIn(ExperimentalCoroutinesApi::class)
class AppUpdateManagerTest {

    private val server = MockWebServer()
    private val client = OkHttpClient()
    private lateinit var tempDir: File

    @Before
    fun setUp() {
        server.start()
        tempDir = Files.createTempDirectory("updates-test").toFile()
    }

    @After
    fun tearDown() {
        server.close()
        tempDir.deleteRecursively()
    }

    private fun createManager(testScope: TestScope, currentVersion: SemVer = SemVer(0, 1, 0, "alpha.1")): AppUpdateManager {
        val testDispatcher = StandardTestDispatcher(testScope.testScheduler)
        return AppUpdateManager(
            cacheDirectory = tempDir,
            client = client,
            scope = testScope,
            baseUrl = server.url("/").toString().removeSuffix("/"),
            ioDispatcher = testDispatcher,
            currentVersionOverride = currentVersion,
        )
    }

    @Test
    fun `checkForUpdates reports Available when newer release exists`() = runTest {
        val json = """
            [
              {
                "tag_name": "kotlin-android-v0.1.0-alpha.2-debug",
                "name": "Kotlin Android client 0.1.0-alpha.2",
                "body": "Bug fixes and improvements",
                "draft": false,
                "prerelease": true,
                "assets": [
                  {
                    "name": "s5code-kotlin-0.1.0-alpha.2-debug.apk",
                    "size": 68000000,
                    "browser_download_url": "${server.url("/download/app.apk")}"
                  }
                ]
              }
            ]
        """.trimIndent()

        server.enqueue(MockResponse.Builder().code(200).body(json).build())

        val manager = createManager(this)
        manager.checkForUpdates()
        advanceUntilIdle()

        val status = manager.status.value
        assertTrue(status is AppUpdateStatus.Available)
        val available = status as AppUpdateStatus.Available
        assertEquals("kotlin-android-v0.1.0-alpha.2-debug", available.release.tagName)
        assertEquals(SemVer(0, 1, 0, "alpha.2"), available.release.version)
    }

    @Test
    fun `checkForUpdates reports UpToDate when current version is latest`() = runTest {
        val json = """
            [
              {
                "tag_name": "kotlin-android-v0.1.0-alpha.1-debug",
                "name": "Kotlin Android client 0.1.0-alpha.1",
                "draft": false,
                "prerelease": true,
                "assets": [
                  {
                    "name": "s5code-kotlin-0.1.0-alpha.1-debug.apk",
                    "size": 68000000,
                    "browser_download_url": "${server.url("/download/app.apk")}"
                  }
                ]
              }
            ]
        """.trimIndent()

        server.enqueue(MockResponse.Builder().code(200).body(json).build())

        val manager = createManager(this)
        manager.checkForUpdates()
        advanceUntilIdle()

        assertTrue(manager.status.value is AppUpdateStatus.UpToDate)
    }

    @Test
    fun `checkForUpdates reports Failed on HTTP error`() = runTest {
        server.enqueue(MockResponse.Builder().code(500).body("Internal Error").build())

        val manager = createManager(this)
        manager.checkForUpdates()
        advanceUntilIdle()

        assertTrue(manager.status.value is AppUpdateStatus.Failed)
    }

    @Test
    fun `downloadUpdate writes APK and transitions to ReadyToInstall`() = runTest {
        val apkBytes = "fake-apk-content-bytes".toByteArray()
        server.enqueue(MockResponse.Builder().code(200).body(okio.Buffer().write(apkBytes)).build())

        val manager = createManager(this)
        val release = AppRelease(
            tagName = "kotlin-android-v0.1.0-alpha.2",
            version = SemVer(0, 1, 0, "alpha.2"),
            title = "v0.1.0-alpha.2",
            notes = "Notes",
            apkName = "s5code-kotlin-0.1.0-alpha.2.apk",
            apkDownloadUrl = server.url("/app.apk").toString(),
            apkSizeBytes = apkBytes.size.toLong(),
            sha256DownloadUrl = null,
        )

        manager.downloadUpdate(release)
        advanceUntilIdle()

        val status = manager.status.value
        assertTrue(status is AppUpdateStatus.ReadyToInstall)
        val ready = status as AppUpdateStatus.ReadyToInstall
        assertTrue(ready.apkFile.exists())
        assertEquals(apkBytes.size.toLong(), ready.apkFile.length())
    }

    @Test
    fun `dismiss resets status to Idle`() = runTest {
        val manager = createManager(this)
        manager.dismiss()
        assertEquals(AppUpdateStatus.Idle, manager.status.value)
    }
}
