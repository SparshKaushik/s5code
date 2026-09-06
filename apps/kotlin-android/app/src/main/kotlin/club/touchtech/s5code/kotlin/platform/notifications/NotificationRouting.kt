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
 * Relay payloads normally carry `deepLink`; older jobs carry only ids. Both paths
 * pass through the same allowlist as shortcuts and app links. Notification taps
 * may open a safe thread child (review, files, terminal, git), but can never open
 * stateful confirmations or arbitrary settings routes.
 */
fun notificationPath(data: Map<String, String>): String? {
    val supplied = data["deepLink"]
    if (supplied != null && supplied.trim() == supplied && '?' !in supplied && '#' !in supplied) {
        val link = parseDeepLinkPath(supplied)
        if (link is DeepLink.Thread) return supplied.ensureLeadingSlash()
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
            listOf("deepLink", "environmentId", "threadId", "phase", "updatedAt").forEach { key ->
                read(key)?.let { put(key, it) }
            }
        }
    )

private fun String.ensureLeadingSlash(): String = if (startsWith('/')) this else "/$this"

private fun encodeSegment(value: String): String =
    URLEncoder.encode(value, Charsets.UTF_8.name()).replace("+", "%20")
