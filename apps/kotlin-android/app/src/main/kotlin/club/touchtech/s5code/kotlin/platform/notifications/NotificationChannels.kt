package club.touchtech.s5code.kotlin.platform.notifications

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.provider.Settings
import androidx.core.net.toUri
import androidx.core.content.ContextCompat

const val ALERT_CHANNEL_ID = "agent-alerts"
const val LIVE_UPDATE_CHANNEL_ID = "agent-live-updates"

fun createNotificationChannels(context: Context) {
    val manager = context.getSystemService(NotificationManager::class.java)
    manager.createNotificationChannels(
        listOf(
            NotificationChannel(
                ALERT_CHANNEL_ID,
                "Agent alerts",
                NotificationManager.IMPORTANCE_HIGH,
            ).apply {
                description = "Approvals, input requests, completed turns, and failures"
            },
            NotificationChannel(
                LIVE_UPDATE_CHANNEL_ID,
                "Agent live updates",
                NotificationManager.IMPORTANCE_DEFAULT,
            ).apply {
                description = "Ongoing agent progress on the lock screen and status bar"
                setShowBadge(false)
            },
        )
    )
}

fun notificationsAllowed(context: Context): Boolean {
    val manager = context.getSystemService(NotificationManager::class.java)
    if (!manager.areNotificationsEnabled()) return false
    return Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
        ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED
}

fun canPostPromotedNotifications(context: Context): Boolean =
    Build.VERSION.SDK_INT >= 36 &&
        notificationsAllowed(context) &&
        context.getSystemService(NotificationManager::class.java).canPostPromotedNotifications()

fun openNotificationSettings(context: Context, promotion: Boolean = false) {
    val primary =
        Intent(
            if (promotion && Build.VERSION.SDK_INT >= 36) {
                Settings.ACTION_APP_NOTIFICATION_PROMOTION_SETTINGS
            } else {
                Settings.ACTION_APP_NOTIFICATION_SETTINGS
            }
        ).apply {
            data = "package:${context.packageName}".toUri()
            putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
    runCatching { context.startActivity(primary) }.onFailure {
        context.startActivity(
            Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).apply {
                putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
        )
    }
}
