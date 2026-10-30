package club.touchtech.s5code.kotlin.platform.updates

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.core.content.FileProvider
import androidx.core.net.toUri
import java.io.File

object InstallApk {

    /**
     * Installs a downloaded APK file via the system package installer.
     *
     * On Android 8.0+ (API 26+), verifies that [Context.getPackageManager] allows installing
     * packages from unknown sources for this app. If not allowed, navigates the user to the
     * system settings toggle with [Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES].
     */
    fun startInstall(context: Context, apkFile: File) {
        if (!apkFile.exists() || apkFile.length() <= 0) {
            throw IllegalArgumentException("APK file does not exist or is empty: ${apkFile.absolutePath}")
        }

        // Verify unknown app installation permission.
        if (!context.packageManager.canRequestPackageInstalls()) {
            val settingsIntent = Intent(
                Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                "package:${context.packageName}".toUri(),
            ).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(settingsIntent)
            return
        }

        val apkUri: Uri = FileProvider.getUriForFile(
            context,
            "${context.packageName}.fileprovider",
            apkFile,
        )

        val installIntent = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(apkUri, "application/vnd.android.package-archive")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }

        context.startActivity(installIntent)
    }
}
