package club.touchtech.s5code.kotlin.platform.updates

import android.content.Context
import club.touchtech.s5code.kotlin.BuildConfig
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.io.FileOutputStream
import java.security.MessageDigest

sealed interface AppUpdateStatus {
    data object Idle : AppUpdateStatus

    data object Checking : AppUpdateStatus

    data class UpToDate(val checkedAtMillis: Long, val currentVersion: String) : AppUpdateStatus

    data class Available(val release: AppRelease, val currentVersion: String) : AppUpdateStatus

    data class Downloading(
        val release: AppRelease,
        val progress: Float,
        val bytesDownloaded: Long,
        val totalBytes: Long,
    ) : AppUpdateStatus

    data class ReadyToInstall(val release: AppRelease, val apkFile: File) : AppUpdateStatus

    data class Failed(val message: String, val canRetry: Boolean = true) : AppUpdateStatus
}

/**
 * Manages checking, downloading, verifying, and launching updates for the native Android app
 * directly from GitHub releases without Google Play Store.
 */
class AppUpdateManager(
    private val cacheDirectory: File,
    private val client: OkHttpClient,
    private val scope: CoroutineScope,
    private val repoOwner: String = DEFAULT_REPO_OWNER,
    private val repoName: String = DEFAULT_REPO_NAME,
    private val baseUrl: String = "https://api.github.com",
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
    private val currentVersionOverride: SemVer? = null,
    private val json: Json = Json { ignoreUnknownKeys = true },
) {
    constructor(
        context: Context,
        client: OkHttpClient,
        scope: CoroutineScope,
        repoOwner: String = DEFAULT_REPO_OWNER,
        repoName: String = DEFAULT_REPO_NAME,
        baseUrl: String = "https://api.github.com",
        ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
        json: Json = Json { ignoreUnknownKeys = true },
    ) : this(
        cacheDirectory = context.cacheDir,
        client = client,
        scope = scope,
        repoOwner = repoOwner,
        repoName = repoName,
        baseUrl = baseUrl,
        ioDispatcher = ioDispatcher,
        currentVersionOverride = null,
        json = json,
    )

    private val _status = MutableStateFlow<AppUpdateStatus>(AppUpdateStatus.Idle)
    val status: StateFlow<AppUpdateStatus> = _status.asStateFlow()

    private val currentVersion: SemVer by lazy {
        currentVersionOverride ?: (SemVer.parse(BuildConfig.VERSION_NAME) ?: SemVer(0, 1, 0, "alpha.1"))
    }

    private val updatesDir: File
        get() = cacheDirectory.resolve("updates").apply { if (!exists()) mkdirs() }

    /**
     * Checks GitHub releases for a newer version of the Kotlin Android app.
     */
    fun checkForUpdates(manual: Boolean = false) {
        if (_status.value is AppUpdateStatus.Checking || _status.value is AppUpdateStatus.Downloading) {
            return
        }

        _status.value = AppUpdateStatus.Checking
        scope.launch {
            try {
                val releases = fetchReleases()
                val latest = findLatestAppRelease(releases, currentVersion)

                if (latest != null) {
                    _status.value = AppUpdateStatus.Available(
                        release = latest,
                        currentVersion = currentVersion.toString(),
                    )
                } else {
                    _status.value = AppUpdateStatus.UpToDate(
                        checkedAtMillis = System.currentTimeMillis(),
                        currentVersion = currentVersion.toString(),
                    )
                }
            } catch (cancellation: CancellationException) {
                throw cancellation
            } catch (e: Exception) {
                _status.value = AppUpdateStatus.Failed(
                    message = e.message ?: "Failed to check for updates.",
                    canRetry = true,
                )
            }
        }
    }

    /**
     * Downloads the APK file for an available release with progress updates and SHA-256 verification.
     */
    fun downloadUpdate(release: AppRelease) {
        if (_status.value is AppUpdateStatus.Downloading) return

        _status.value = AppUpdateStatus.Downloading(
            release = release,
            progress = 0f,
            bytesDownloaded = 0L,
            totalBytes = release.apkSizeBytes,
        )

        scope.launch {
            try {
                val downloadedFile = withContext(ioDispatcher) {
                    val targetFile = updatesDir.resolve("s5code-${release.version}.apk")
                    if (targetFile.exists()) targetFile.delete()

                    val request = Request.Builder()
                        .url(release.apkDownloadUrl)
                        .header("Accept", "application/octet-stream")
                        .build()

                    val response = client.newCall(request).execute()
                    if (!response.isSuccessful) {
                        throw IllegalStateException("Download failed with HTTP ${response.code}")
                    }

                    val body = response.body
                    val totalBytes = if (body.contentLength() > 0) body.contentLength() else release.apkSizeBytes
                    val digest = MessageDigest.getInstance("SHA-256")

                    body.byteStream().use { input ->
                        FileOutputStream(targetFile).use { output ->
                            val buffer = ByteArray(8192)
                            var bytesRead: Int
                            var totalRead = 0L
                            var lastReportTime = 0L

                            while (input.read(buffer).also { bytesRead = it } != -1) {
                                output.write(buffer, 0, bytesRead)
                                digest.update(buffer, 0, bytesRead)
                                totalRead += bytesRead

                                val now = System.currentTimeMillis()
                                if (now - lastReportTime > 100 || totalRead == totalBytes) {
                                    lastReportTime = now
                                    val progress = if (totalBytes > 0) (totalRead.toFloat() / totalBytes).coerceIn(0f, 1f) else 0f
                                    _status.value = AppUpdateStatus.Downloading(
                                        release = release,
                                        progress = progress,
                                        bytesDownloaded = totalRead,
                                        totalBytes = totalBytes,
                                    )
                                }
                            }
                        }
                    }

                    // Optional SHA-256 verification if companion asset exists
                    if (release.sha256DownloadUrl != null) {
                        val computedSha = digest.digest().joinToString("") { "%02x".format(it) }
                        verifySha256(release.sha256DownloadUrl, computedSha)
                    }

                    targetFile
                }

                _status.value = AppUpdateStatus.ReadyToInstall(release, downloadedFile)
            } catch (cancellation: CancellationException) {
                throw cancellation
            } catch (e: Exception) {
                _status.value = AppUpdateStatus.Failed(
                    message = e.message ?: "Failed to download update.",
                    canRetry = true,
                )
            }
        }
    }

    /**
     * Launches the system package installer.
     */
    fun installUpdate(context: Context, apkFile: File) {
        try {
            InstallApk.startInstall(context, apkFile)
        } catch (e: Exception) {
            _status.value = AppUpdateStatus.Failed(
                message = e.message ?: "Unable to launch system package installer.",
                canRetry = false,
            )
        }
    }

    fun dismiss() {
        _status.value = AppUpdateStatus.Idle
    }

    private suspend fun fetchReleases(): List<GitHubReleaseDto> = withContext(ioDispatcher) {
        val url = "$baseUrl/repos/$repoOwner/$repoName/releases"
        val request = Request.Builder()
            .url(url)
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "S5Code-Android-Updater")
            .build()

        val response = client.newCall(request).execute()
        if (!response.isSuccessful) {
            throw IllegalStateException("GitHub releases returned HTTP ${response.code}")
        }

        val bodyString = response.body.string()
        json.decodeFromString<List<GitHubReleaseDto>>(bodyString)
    }

    private fun verifySha256(sha256Url: String, computedHash: String) {
        val request = Request.Builder().url(sha256Url).build()
        val response = client.newCall(request).execute()
        if (response.isSuccessful) {
            val expectedText = response.body.string().trim()
            val expectedHash = expectedText.split("\\s+".toRegex()).firstOrNull()?.lowercase()
            if (expectedHash != null && expectedHash.isNotEmpty() && expectedHash != computedHash.lowercase()) {
                throw IllegalStateException("Checksum mismatch! Expected: $expectedHash, Computed: $computedHash")
            }
        }
    }

    companion object {
        const val DEFAULT_REPO_OWNER = "SparshKaushik"
        const val DEFAULT_REPO_NAME = "s5code"
    }
}
