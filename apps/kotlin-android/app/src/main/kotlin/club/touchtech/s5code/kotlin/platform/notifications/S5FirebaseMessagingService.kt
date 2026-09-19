package club.touchtech.s5code.kotlin.platform.notifications

import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

/**
 * Receives token rotation and the relay's `agent_activity` data pushes.
 *
 * The relay sends exactly one kind: `t3_kind == "agent_activity"`, whose flat
 * keys carry both the alert and the ongoing card (see `fcmPayloads.ts` in
 * `infra/relay` and the RN client's `AgentMessagingService`). Anything else is
 * not ours and is ignored.
 */
class S5FirebaseMessagingService : FirebaseMessagingService() {
    @Suppress("OVERRIDE_DEPRECATION")
    override fun onNewToken(token: String) {
        PushRuntime.onToken(applicationContext, token)
    }

    override fun onMessageReceived(message: RemoteMessage) {
        if (message.data["t3_kind"] == "agent_activity") {
            AndroidLiveUpdateNotifications.receive(applicationContext, message.data)
        }
    }
}
