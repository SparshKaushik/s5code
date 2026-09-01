package club.touchtech.s5code.kotlin.platform.notifications

import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

/** Receives token rotation, ordinary alert pushes, and data-only Live Updates. */
class S5FirebaseMessagingService : FirebaseMessagingService() {
    @Suppress("OVERRIDE_DEPRECATION")
    override fun onNewToken(token: String) {
        PushRuntime.onToken(applicationContext, token)
    }

    override fun onMessageReceived(message: RemoteMessage) {
        if (message.data["t3Type"] == "android_live_update") {
            AndroidLiveUpdateNotifications.receive(applicationContext, message.data["liveUpdate"])
            return
        }
        postAgentAlert(
            context = applicationContext,
            data = message.data,
            title = message.notification?.title,
            body = message.notification?.body,
        )
    }
}
