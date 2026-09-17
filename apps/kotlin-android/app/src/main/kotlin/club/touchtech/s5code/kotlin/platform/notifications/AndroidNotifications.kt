package club.touchtech.s5code.kotlin.platform.notifications

import android.Manifest
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.text.TextPaint
import android.text.TextUtils
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ProcessLifecycleOwner
import club.touchtech.s5code.kotlin.MainActivity
import club.touchtech.s5code.kotlin.R

class AndroidLiveUpdateDismissReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        AndroidLiveUpdateNotifications.dismiss(context)
    }
}

data class AndroidLiveUpdateDiagnostics(
    val apiLevel: Int,
    val supported: Boolean,
    val notificationPermission: Boolean,
    val promotionPermission: Boolean,
    /** Last relay delivery timestamp this device consumed, epoch millis. */
    val lastUpdateAtMillis: Long?,
    val cardVisible: Boolean,
    val delivery: String?,
)

/**
 * Consumes the relay's `t3_kind: agent_activity` data pushes.
 *
 * The wire format is flat snake_case keys produced by `fcmPayloads.ts` /
 * `FcmDeliveries.ts` in `infra/relay`: `alert_id`/`alert_title`/`alert_body`/
 * `alert_path` for attention alerts, and `active`/`activity_title`/
 * `activity_body`/`activity_line_N` (tab-separated `status \t threadTitle \t
 * projectTitle`)/`activity_path`/`activity_expires_at` for the ongoing card.
 * Both ride in one payload; identity keys `device_id`/`user_id` must match what
 * registration configured or the push belongs to a different device/account.
 *
 * This is a deliberate port of `AgentNotifications` in
 * `apps/mobile/modules/t3-agent-notifications` — the React Native app's Android
 * path — so both clients present the same card for the same payload.
 */
object AndroidLiveUpdateNotifications {
    private const val NOTIFICATION_ID = 53_005
    private const val PREFERENCES = "s5code.android-live-updates"
    private const val KEY_DEVICE_ID = "device-id"
    private const val KEY_USER_ID = "user-id"
    private const val KEY_ONGOING = "ongoing"
    private const val KEY_DISMISSED = "dismissed"
    private const val KEY_LAST_UPDATE = "last-update"
    private const val KEY_LAST_ACTIVE = "last-active"
    private const val KEY_SEEN_ALERTS = "seen-alerts"
    private const val KEY_DELIVERY = "delivery"

    /** Pushes older than this are stale queue replays, not news. */
    private const val MAX_MESSAGE_AGE_MS = 10 * 60 * 1000L

    /** Fallback lifetime for relays that omit `activity_expires_at`. */
    private const val RUNNING_LIFETIME_MS = 2 * 60 * 60 * 1000L
    private const val MAX_LIFETIME_MS = 24 * 60 * 60 * 1000L
    private const val MAX_ACTIVITY_LINES = 5
    private const val MAX_SEEN_ALERTS = 63

    /**
     * Records the registered identity once per registration so a headless FCM
     * service can accept or ignore pushes without touching account state.
     * Identity changes wipe delivery history: a different account's cards and
     * seen-alert set must not survive a sign-out/sign-in.
     */
    @Synchronized
    fun configure(context: Context, deviceId: String, userId: String, ongoingEnabled: Boolean) {
        val prefs = preferences(context)
        if (
            prefs.getString(KEY_USER_ID, null) != userId ||
                prefs.getString(KEY_DEVICE_ID, null) != deviceId
        ) {
            clear(context)
        }
        val wasEnabled = prefs.getBoolean(KEY_ONGOING, false)
        prefs.edit {
            putString(KEY_DEVICE_ID, deviceId)
            putString(KEY_USER_ID, userId)
            putBoolean(KEY_ONGOING, ongoingEnabled)
        }
        if (ongoingEnabled && !wasEnabled) prefs.edit { putBoolean(KEY_DISMISSED, false) }
        if (!ongoingEnabled) cancelCard(context)
        createNotificationChannels(context)
    }

    /** Sign-out: cancel every card this object owns and forget the identity. */
    @Synchronized
    fun clear(context: Context) {
        cancelCard(context)
        preferences(context).edit { clear() }
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.activeNotifications.forEach {
            if (it.id == NOTIFICATION_ID) manager.cancel(it.id)
        }
    }

    /** Swiping the card away keeps it hidden until a new run starts. */
    @Synchronized
    fun dismiss(context: Context) {
        preferences(context).edit { putBoolean(KEY_DISMISSED, true) }
        cancelCard(context)
    }

    /**
     * Starting new agent work un-dismisses the card so the next relay update is
     * visible again. There is no relay registration for this — the relay drives
     * the card from the aggregate it already publishes.
     */
    @Synchronized
    fun arm(context: Context) {
        preferences(context).edit {
            putBoolean(KEY_DISMISSED, false)
            putString(KEY_DELIVERY, "armed")
        }
    }

    @Synchronized
    fun receive(context: Context, data: Map<String, String>) {
        val prefs = preferences(context)
        val updatedAt = data["updated_at"]?.toLongOrNull() ?: return
        val registered =
            prefs.getString(KEY_DEVICE_ID, null) != null &&
                data["device_id"] == prefs.getString(KEY_DEVICE_ID, null) &&
                data["user_id"] == prefs.getString(KEY_USER_ID, null)
        val fresh =
            System.currentTimeMillis() - updatedAt in -MAX_MESSAGE_AGE_MS..MAX_MESSAGE_AGE_MS
        if (!registered || !fresh || !notificationsAllowed(context)) return
        createNotificationChannels(context)
        showAlert(context, prefs, data)
        updateActivity(context, prefs, data, updatedAt)
    }

    fun diagnostics(context: Context): AndroidLiveUpdateDiagnostics =
        AndroidLiveUpdateDiagnostics(
            apiLevel = Build.VERSION.SDK_INT,
            supported = Build.VERSION.SDK_INT >= 36,
            notificationPermission = notificationsAllowed(context),
            promotionPermission = canPostPromotedNotifications(context),
            lastUpdateAtMillis = preferences(context).getLong(KEY_LAST_UPDATE, 0L)
                .takeIf { it > 0 },
            cardVisible =
                context
                    .getSystemService(NotificationManager::class.java)
                    .activeNotifications
                    .any { it.id == NOTIFICATION_ID },
            delivery = preferences(context).getString(KEY_DELIVERY, null),
        )

    private fun showAlert(
        context: Context,
        prefs: android.content.SharedPreferences,
        data: Map<String, String>,
    ) {
        // Queue retries carry the same alert id; a bounded seen-set keeps a
        // redelivery from re-alerting after a newer alert already landed.
        val alertId = data["alert_id"]
        val seen =
            prefs.getString(KEY_SEEN_ALERTS, null)?.split('\n').orEmpty()
        if (alertId == null || alertId in seen) return
        // Match iOS foreground presentation: suppressed alerts are still marked
        // seen so a retry cannot surface them after the app backgrounds.
        if (!ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) {
            postAgentAlert(context, data)
        }
        prefs.edit {
            putString(
                KEY_SEEN_ALERTS,
                (seen.takeLast(MAX_SEEN_ALERTS) + alertId).joinToString("\n"),
            )
        }
    }

    private fun updateActivity(
        context: Context,
        prefs: android.content.SharedPreferences,
        data: Map<String, String>,
        updatedAt: Long,
    ) {
        // Reordered deliveries must not resurrect an older card.
        if (updatedAt < prefs.getLong(KEY_LAST_UPDATE, 0)) return
        prefs.edit {
            putLong(KEY_LAST_UPDATE, updatedAt)
            putString(KEY_DELIVERY, "delivered")
        }
        val active = data["active"] == "true"
        val expiresAt =
            data["activity_expires_at"]?.toLongOrNull()
                ?: if (active) updatedAt + RUNNING_LIFETIME_MS else 0L
        val remainingMs = (expiresAt - System.currentTimeMillis()).coerceAtMost(MAX_LIFETIME_MS)
        val wasActive = prefs.getBoolean(KEY_LAST_ACTIVE, false)
        prefs.edit { putBoolean(KEY_LAST_ACTIVE, active) }
        if (remainingMs <= 0 || !prefs.getBoolean(KEY_ONGOING, false)) {
            cancelCard(context)
            prefs.edit { putBoolean(KEY_DISMISSED, false) }
            return
        }
        // A new run re-arms a dismissed card; terminal replays stay dismissed.
        if (active && !wasActive) prefs.edit { putBoolean(KEY_DISMISSED, false) }
        if (!prefs.getBoolean(KEY_DISMISSED, false)) {
            showCard(context, data, active, remainingMs)
        }
    }

    private fun showCard(
        context: Context,
        data: Map<String, String>,
        active: Boolean,
        remainingMs: Long,
    ) {
        val body = data["activity_body"].orEmpty().take(240)
        val dismissIntent =
            PendingIntent.getBroadcast(
                context,
                NOTIFICATION_ID,
                Intent(context, AndroidLiveUpdateDismissReceiver::class.java),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        val lines =
            (0 until MAX_ACTIVITY_LINES).mapNotNull { index ->
                data["activity_line_$index"]?.let { activityLine(context, it) }
            }
        // BigTextStyle stays eligible for Live Update promotion; the custom
        // ProgressStyle the old payload imagined is gone with that payload.
        val style =
            NotificationCompat.BigTextStyle()
                .bigText(if (lines.isEmpty()) body else lines.joinToString("\n"))
        val notification =
            NotificationCompat.Builder(context, LIVE_UPDATE_CHANNEL_ID)
                .setSmallIcon(R.drawable.notification_icon)
                .setContentTitle(data["activity_title"].orEmpty().take(120))
                .setContentText(body)
                .setStyle(style)
                .setOngoing(active)
                .setOnlyAlertOnce(true)
                .setSilent(true)
                .setShowWhen(false)
                .setTimeoutAfter(remainingMs)
                // Live Updates must remain uncolorized to qualify for promotion.
                .setColorized(false)
                .setRequestPromotedOngoing(active)
                .setPriority(NotificationCompat.PRIORITY_LOW)
                .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
                .setContentIntent(
                    notificationPath(mapOf("deepLink" to data["activity_path"].orEmpty()))?.let {
                        notificationPendingIntent(context, it, NOTIFICATION_ID)
                    }
                )
                .setDeleteIntent(dismissIntent)
                .addAction(0, "Dismiss", dismissIntent)
                .build()
        context.getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification)
    }

    /**
     * Fitted `status: title · project` row for the expanded card, matching the
     * RN module's width math so the narrow shade column never clips the status.
     */
    private fun activityLine(context: Context, value: String): String {
        val parts = value.split('\t', limit = 3)
        if (parts.size != 3) return value.take(300)
        val metrics = context.resources.displayMetrics
        val paint = TextPaint().apply { textSize = 14 * metrics.scaledDensity }
        val prefix = "${parts[0]}: "
        val separator = " · "
        val width =
            (metrics.widthPixels - 152 * metrics.density)
                .coerceIn(120 * metrics.density, 280 * metrics.density)
        val available = (width - paint.measureText(prefix + separator)).coerceAtLeast(0f)
        val projectWidth = paint.measureText(parts[2]).coerceAtMost(available * 0.4f)
        val titleWidth = paint.measureText(parts[1]).coerceAtMost(available - projectWidth)
        val title = TextUtils.ellipsize(parts[1], paint, titleWidth, TextUtils.TruncateAt.END)
        val project =
            TextUtils.ellipsize(
                parts[2],
                paint,
                available - titleWidth,
                TextUtils.TruncateAt.END,
            )
        return "$prefix$title$separator$project"
    }

    private fun cancelCard(context: Context) {
        context.getSystemService(NotificationManager::class.java).cancel(NOTIFICATION_ID)
    }

    private fun preferences(context: Context) =
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
}

/**
 * One relay alert push. `alert_path` carries the validated deep link the card
 * opens; the ids remain the fallback for older payload shapes.
 */
fun postAgentAlert(context: Context, data: Map<String, String>) {
    if (!notificationsAllowed(context)) return
    val path = notificationPath(data) ?: return
    createNotificationChannels(context)
    val title = data["alert_title"].orEmpty().take(120)
    // Grouped alerts list up to five 120-character thread titles.
    val body = data["alert_body"].orEmpty().take(608)
    val id =
        (data["alert_id"] ?: "${data["environmentId"]}/${data["threadId"]}/${data["phase"]}")
            .hashCode() and Int.MAX_VALUE
    val notification =
        NotificationCompat.Builder(context, ALERT_CHANNEL_ID)
            .setSmallIcon(R.drawable.notification_icon)
            .setContentTitle(title.ifBlank { "S5 Code" })
            .setContentText(body.ifBlank { "Agent activity needs your attention" })
            .setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setShowWhen(false)
            .setAutoCancel(true)
            .setOnlyAlertOnce(true)
            .setContentIntent(notificationPendingIntent(context, path, id))
            .build()
    context.getSystemService(NotificationManager::class.java).notify(id, notification)
}

private fun notificationPendingIntent(context: Context, path: String, id: Int): PendingIntent =
    PendingIntent.getActivity(
        context,
        id,
        Intent(context, MainActivity::class.java).apply {
            action = Intent.ACTION_VIEW
            flags = Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
            putExtra(EXTRA_NOTIFICATION_PATH, path)
        },
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )
