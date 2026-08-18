package expo.modules.t3nativecontrols

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.graphics.drawable.Icon
import android.net.Uri
import android.os.Build
import android.provider.Settings
import com.google.firebase.messaging.RemoteMessage
import expo.modules.notifications.service.ExpoFirebaseMessagingService
import org.json.JSONArray
import org.json.JSONObject
import java.time.Instant

class T3FirebaseMessagingService : ExpoFirebaseMessagingService() {
  override fun onMessageReceived(remoteMessage: RemoteMessage) {
    if (remoteMessage.data["t3Type"] == "android_live_update") {
      AndroidLiveUpdateNotifications.receive(applicationContext, remoteMessage.data["liveUpdate"])
      return
    }
    super.onMessageReceived(remoteMessage)
  }
}

class AndroidLiveUpdateDismissReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent?) {
    AndroidLiveUpdateNotifications.dismiss(context)
  }
}

object AndroidLiveUpdateNotifications {
  private const val CHANNEL_ID = "agent-live-updates"
  private const val CHANNEL_NAME = "Agent live updates"
  private const val NOTIFICATION_ID = 53005
  private const val PREFERENCES = "t3-android-live-updates"
  private const val KEY_GENERATION = "generation"
  private const val KEY_EVENT_AT = "event-at"
  private const val MAX_PROGRESS_POINTS = 12

  fun arm(context: Context, generationId: String, seedJson: String) {
    if (Build.VERSION.SDK_INT < 36 || generationId.isBlank()) return
    val seed = runCatching { JSONObject(seedJson) }.getOrNull() ?: JSONObject()
    preferences(context).edit()
      .putString(KEY_GENERATION, generationId)
      .remove(KEY_EVENT_AT)
      .apply()
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
              put("threadTitle", seed.optString("threadTitle", "Agent work"))
              put("projectTitle", seed.optString("projectTitle", "S5 Code"))
              put("phase", "starting")
              put("status", "Connecting")
              put("deepLink", "/")
            },
          ),
        )
      }
    post(context, aggregate)
  }

  @Suppress("ReturnCount")
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
    if (generationId.isBlank() || generationId != currentGeneration(context)) return
    val eventAt = payload.optString("eventAt")
    if (eventAt.isBlank() || !isNewerEvent(context, eventAt)) return
    preferences(context).edit().putString(KEY_EVENT_AT, eventAt).apply()
    if (payload.optString("event") == "end") {
      dismiss(context)
      return
    }
    val aggregate = payload.optJSONObject("aggregate") ?: return
    post(context, aggregate)
  }

  fun currentGeneration(context: Context): String? =
    preferences(context).getString(KEY_GENERATION, null)

  fun dismiss(context: Context) {
    context.getSystemService(NotificationManager::class.java).cancel(NOTIFICATION_ID)
    preferences(context).edit().clear().apply()
  }

  fun canPostPromoted(context: Context): Boolean =
    Build.VERSION.SDK_INT >= 36 &&
      context.getSystemService(NotificationManager::class.java).canPostPromotedNotifications()

  fun openSettings(context: Context) {
    val intent =
      Intent(
        if (Build.VERSION.SDK_INT >= 36) {
          Settings.ACTION_APP_NOTIFICATION_PROMOTION_SETTINGS
        } else {
          Settings.ACTION_APP_NOTIFICATION_SETTINGS
        },
      ).apply {
        data = Uri.parse("package:${context.packageName}")
        putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
      }
    runCatching { context.startActivity(intent) }.onFailure {
      context.startActivity(
        Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).apply {
          putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)
          addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        },
      )
    }
  }

  private fun preferences(context: Context) =
    context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)

  @Suppress("ReturnCount")
  private fun isNewerEvent(context: Context, eventAt: String): Boolean {
    val previous = preferences(context).getString(KEY_EVENT_AT, null) ?: return true
    val nextInstant = runCatching { Instant.parse(eventAt) }.getOrNull() ?: return false
    val previousInstant = runCatching { Instant.parse(previous) }.getOrNull() ?: return true
    return nextInstant.isAfter(previousInstant)
  }

  @Suppress("CyclomaticComplexMethod", "LongMethod", "ReturnCount")
  private fun post(context: Context, aggregate: JSONObject) {
    if (Build.VERSION.SDK_INT < 36) return
    val manager = context.getSystemService(NotificationManager::class.java)
    manager.createNotificationChannel(
      NotificationChannel(CHANNEL_ID, CHANNEL_NAME, NotificationManager.IMPORTANCE_DEFAULT),
    )
    val activities = aggregate.optJSONArray("activities") ?: JSONArray()
    if (activities.length() == 0) {
      dismiss(context)
      return
    }
    val first = activities.optJSONObject(0) ?: return
    val phase = first.optString("phase")
    val status = first.optString("status", "Working")
    val title = first.optString("threadTitle", aggregate.optString("title", "S5 Code"))
    val project = first.optString("projectTitle")
    val progress = first.optJSONObject("planProgress")
    val totalSteps = progress?.optInt("totalSteps", 0)?.coerceAtLeast(0) ?: 0
    val completedSteps = progress?.optInt("completedSteps", 0)?.coerceIn(0, totalSteps) ?: 0
    val currentStep = progress?.optString("step")?.takeIf { it.isNotBlank() }
    val content = when {
      currentStep != null && totalSteps > 0 -> "$currentStep · $completedSteps/$totalSteps"
      project.isNotBlank() -> "$status · $project"
      else -> status
    }
    val chip = when (phase) {
      "waiting_for_approval" -> "Approve"
      "waiting_for_input" -> "Input"
      "completed" -> "Done"
      "failed" -> "Failed"
      else -> if (totalSteps > 0) "$completedSteps/$totalSteps" else "Working"
    }.take(7)

    val deepLink = first.optString("deepLink")
      .takeIf { it.startsWith("/") && !it.startsWith("//") }
      ?.let { "s5code://${it.removePrefix("/")}" }
    val contentIntent = deepLink?.let {
      PendingIntent.getActivity(
        context,
        NOTIFICATION_ID,
        Intent(Intent.ACTION_VIEW, Uri.parse(it)).setPackage(context.packageName),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
      )
    }
    val visualSteps = totalSteps.coerceIn(1, MAX_PROGRESS_POINTS)
    val visualProgress = if (totalSteps > 1) {
      ((completedSteps.toDouble() / totalSteps) * (visualSteps - 1)).toInt()
    } else {
      0
    }
    val style = Notification.ProgressStyle()
      .setStyledByProgress(true)
      .setProgress(visualProgress)
      .setProgressTrackerIcon(Icon.createWithResource(context, context.applicationInfo.icon))
    if (totalSteps > 0) {
      style.setProgressSegments(
        List(maxOf(visualSteps - 1, 1)) {
          Notification.ProgressStyle.Segment(1).setColor(Color.rgb(120, 120, 128))
        },
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
        },
      )
    }

    val iconId =
      context.resources
        .getIdentifier("notification_icon", "drawable", context.packageName)
        .takeIf { it != 0 } ?: context.applicationInfo.icon
    val deleteIntent = PendingIntent.getBroadcast(
      context,
      NOTIFICATION_ID,
      Intent(context, AndroidLiveUpdateDismissReceiver::class.java),
      PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )
    val notification = Notification.Builder(context, CHANNEL_ID)
      .setSmallIcon(iconId)
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
        },
      )
      .build()
    manager.notify(NOTIFICATION_ID, notification)
  }
}
