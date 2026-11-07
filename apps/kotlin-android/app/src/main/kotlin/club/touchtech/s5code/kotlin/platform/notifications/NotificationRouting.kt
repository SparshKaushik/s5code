package club.touchtech.s5code.kotlin.platform.notifications

import club.touchtech.s5code.kotlin.app.DeepLink
import club.touchtech.s5code.kotlin.app.parseDeepLinkPath
import java.net.URLEncoder

/** Explicit MainActivity extra used by app-rendered notification PendingIntents. */
const val EXTRA_NOTIFICATION_PATH = "club.touchtech.s5code.kotlin.NOTIFICATION_PATH"

private const val MAX_NOTIFICATION_SEGMENT_LENGTH = 256

/**
 * Resolves an untrusted FCM data map to a validated in-app thread path.
 *
 * The relay's `agent_activity` payload carries the alert deep link as
 * `alert_path` and the ongoing-card link as `activity_path` (see
 * `fcmPayloads.ts` in `infra/relay`). Older payloads used `deepLink`, and the
 * oldest carried only ids. All paths pass through the same allowlist as
 * shortcuts and app links: notification taps may open a thread or a safe child
 * (review, files, terminal, git), never a stateful confirmation or a settings
 * route.
 */
fun notificationPath(data: Map<String, String>): String? {
    for (key in listOf("alert_path", "activity_path", "deepLink")) {
        val supplied = data[key] ?: continue
        if (supplied.trim() != supplied || '?' in supplied || '#' in supplied) continue
        if (parseDeepLinkPath(supplied) is DeepLink.Thread) return supplied.ensureLeadingSlash()
    }

    val environmentId = data["environmentId"]?.trim().orEmpty()
    val threadId = data["threadId"]?.trim().orEmpty()
    if (
        environmentId.isEmpty() ||
        threadId.isEmpty() ||
        environmentId.length > MAX_NOTIFICATION_SEGMENT_LENGTH ||
        threadId.length > MAX_NOTIFICATION_SEGMENT_LENGTH
    ) {
        return null
    }
    val path = "/threads/${encodeSegment(environmentId)}/${encodeSegment(threadId)}"
    return path.takeIf { parseDeepLinkPath(it) is DeepLink.Thread }
}

fun notificationPathFromExtras(read: (String) -> String?): String? =
    notificationPath(
        buildMap {
            listOf(
                    "alert_path",
                    "activity_path",
                    "deepLink",
                    "environmentId",
                    "threadId",
                    "phase",
                    "updatedAt",
                )
                .forEach { key -> read(key)?.let { put(key, it) } }
        }
    )

private fun String.ensureLeadingSlash(): String = if (startsWith('/')) this else "/$this"

private fun encodeSegment(value: String): String =
    URLEncoder.encode(value, Charsets.UTF_8.name()).replace("+", "%20")
