package club.touchtech.s5code.kotlin.platform.notifications

import android.Manifest
import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.drawable.Icon
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import club.touchtech.s5code.kotlin.MainActivity
import club.touchtech.s5code.kotlin.R
import java.time.Instant
import org.json.JSONArray
import org.json.JSONObject

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
    val generationId: String?,
    val eventAt: String?,
    val delivery: String?,
)

/** Real API 36 promoted-ongoing notification implementation. */
object AndroidLiveUpdateNotifications {
    private const val NOTIFICATION_ID = 53_005
    private const val PREFERENCES = "s5code.android-live-updates"
    private const val KEY_GENERATION = "generation"
    private const val KEY_EVENT_AT = "event-at"
    private const val KEY_DELIVERY = "delivery"
    private const val MAX_PROGRESS_POINTS = 12

    fun arm(context: Context, generationId: String, threadTitle: String, projectTitle: String): Boolean {
        if (Build.VERSION.SDK_INT < 36 || generationId.isBlank() || !notificationsAllowed(context)) {
            return false
        }
        preferences(context).edit {
            putString(KEY_GENERATION, generationId)
            remove(KEY_EVENT_AT)
            putString(KEY_DELIVERY, "armed")
        }
        val aggregate =
            JSONObject().apply {
                put("title", "S5 Code")
                put("subtitle", "Agent work in progress")
                put("activeCount", 1)
                put("updatedAt", Instant.now().toString())
                put(
                    "activities",
                    JSONArray().put(
                        JSONObject().apply {
                            put("threadTitle", threadTitle.ifBlank { "Agent work" })
                            put("projectTitle", projectTitle.ifBlank { "S5 Code" })
                            put("phase", "starting")
                            put("status", "Connecting")
                            put("deepLink", "/")
                        }
                    )
                )
            }
        return post(context, aggregate)
    }

    fun receive(context: Context, payloadJson: String?) {
        if (Build.VERSION.SDK_INT < 36 || payloadJson.isNullOrBlank()) return
        val payload = runCatching { JSONObject(payloadJson) }.getOrNull() ?: return
        if (
            payload.optString("type") != "agent_activity_live_update" ||
                payload.optInt("version") != 1
        ) {
            return
        }
        val generationId = payload.optString("generationId")
        val eventAt = payload.optString("eventAt")
        val previousEventAt = preferences(context).getString(KEY_EVENT_AT, null)
        if (
            !shouldAcceptLiveUpdateEvent(
                currentGeneration = currentGeneration(context),
                receivedGeneration = generationId,
                eventAt = eventAt,
                previousEventAt = previousEventAt,
            )
        ) {
            return
        }
        preferences(context).edit { putString(KEY_EVENT_AT, eventAt) }
        if (payload.optString("event") == "end") {
            dismiss(context)
            return
        }
        val aggregate = payload.optJSONObject("aggregate") ?: return
        if (post(context, aggregate)) {
            preferences(context).edit { putString(KEY_DELIVERY, "delivered") }
        }
    }

    fun currentGeneration(context: Context): String? =
        preferences(context).getString(KEY_GENERATION, null)

    fun diagnostics(context: Context): AndroidLiveUpdateDiagnostics =
        AndroidLiveUpdateDiagnostics(
            apiLevel = Build.VERSION.SDK_INT,
            supported = Build.VERSION.SDK_INT >= 36,
            notificationPermission = notificationsAllowed(context),
            promotionPermission = canPostPromotedNotifications(context),
            generationId = currentGeneration(context),
            eventAt = preferences(context).getString(KEY_EVENT_AT, null),
            delivery = preferences(context).getString(KEY_DELIVERY, null),
        )

    fun dismiss(context: Context) {
        context.getSystemService(NotificationManager::class.java).cancel(NOTIFICATION_ID)
        preferences(context).edit { clear() }
    }

    private fun preferences(context: Context) =
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)

    private fun post(context: Context, aggregate: JSONObject): Boolean {
        if (Build.VERSION.SDK_INT < 36 || !notificationsAllowed(context)) return false
        createNotificationChannels(context)
        val activities = aggregate.optJSONArray("activities") ?: JSONArray()
        if (activities.length() == 0) {
            dismiss(context)
            return false
        }
        val first = activities.optJSONObject(0) ?: return false
        val phase = first.optString("phase")
        val status = first.optString("status", "Working")
        val title = first.optString("threadTitle", aggregate.optString("title", "S5 Code"))
        val project = first.optString("projectTitle")
        val progress = first.optJSONObject("planProgress")
        val totalSteps = progress?.optInt("totalSteps", 0)?.coerceAtLeast(0) ?: 0
        val completedSteps = progress?.optInt("completedSteps", 0)?.coerceIn(0, totalSteps) ?: 0
        val currentStep = progress?.optString("step")?.takeIf(String::isNotBlank)
        val content =
            when {
                currentStep != null && totalSteps > 0 -> "$currentStep · $completedSteps/$totalSteps"
                project.isNotBlank() -> "$status · $project"
                else -> status
            }
        val chip =
            when (phase) {
                    "waiting_for_approval" -> "Approve"
                    "waiting_for_input" -> "Input"
                    "completed" -> "Done"
                    "failed" -> "Failed"
                    else -> if (totalSteps > 0) "$completedSteps/$totalSteps" else "Working"
                }
                .take(7)
        val path = notificationPath(mapOf("deepLink" to first.optString("deepLink")))
        val contentIntent = path?.let { notificationPendingIntent(context, it, NOTIFICATION_ID) }
        val visualSteps = totalSteps.coerceIn(1, MAX_PROGRESS_POINTS)
        val visualProgress =
            if (totalSteps > 1) {
                ((completedSteps.toDouble() / totalSteps) * (visualSteps - 1)).toInt()
            } else {
                0
            }
        val style =
            Notification.ProgressStyle()
                .setStyledByProgress(true)
                .setProgress(visualProgress)
                .setProgressTrackerIcon(Icon.createWithResource(context, R.drawable.notification_icon))
        if (totalSteps > 0) {
            style.setProgressSegments(
                List(maxOf(visualSteps - 1, 1)) {
                    Notification.ProgressStyle.Segment(1).setColor(Color.rgb(120, 120, 128))
                }
            )
            style.setProgressPoints(
                (0 until visualSteps).map { position ->
                    val color =
                        when {
                            position < visualProgress -> Color.rgb(52, 199, 89)
                            position == visualProgress -> Color.rgb(0, 99, 155)
                            else -> Color.rgb(142, 142, 147)
                        }
                    Notification.ProgressStyle.Point(position).setColor(color)
                }
            )
        }
        val deleteIntent =
            PendingIntent.getBroadcast(
                context,
                NOTIFICATION_ID,
                Intent(context, AndroidLiveUpdateDismissReceiver::class.java),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        val notification =
            Notification.Builder(context, LIVE_UPDATE_CHANNEL_ID)
                .setSmallIcon(R.drawable.notification_icon)
                .setContentTitle(title.take(120))
                .setContentText(content.take(120))
                .setSubText(aggregate.optString("subtitle").take(120))
                .setShortCriticalText(chip)
                .setStyle(style)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .setShowWhen(false)
                .setContentIntent(contentIntent)
                .setDeleteIntent(deleteIntent)
                .addExtras(
                    android.os.Bundle().apply {
                        putBoolean("android.requestPromotedOngoing", true)
                    }
                )
                .build()
        context.getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification)
        return true
    }
}

internal fun shouldAcceptLiveUpdateEvent(
    currentGeneration: String?,
    receivedGeneration: String,
    eventAt: String,
    previousEventAt: String?,
): Boolean {
    if (receivedGeneration.isBlank() || receivedGeneration != currentGeneration) return false
    val next = runCatching { Instant.parse(eventAt) }.getOrNull() ?: return false
    val previous = previousEventAt?.let { runCatching { Instant.parse(it) }.getOrNull() } ?: return true
    return next.isAfter(previous)
}

fun postAgentAlert(context: Context, data: Map<String, String>, title: String?, body: String?) {
    if (!notificationsAllowed(context)) return
    val path = notificationPath(data) ?: return
    createNotificationChannels(context)
    val id = ("${data["environmentId"]}/${data["threadId"]}/${data["phase"]}").hashCode() and Int.MAX_VALUE
    val notification =
        NotificationCompat.Builder(context, ALERT_CHANNEL_ID)
            .setSmallIcon(R.drawable.notification_icon)
            .setContentTitle(title?.takeIf(String::isNotBlank) ?: "S5 Code")
            .setContentText(body?.takeIf(String::isNotBlank) ?: "Agent activity needs your attention")
            .setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
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
