package club.touchtech.s5code.kotlin.platform

import android.content.Intent
import android.net.Uri
import android.os.Build
import club.touchtech.s5code.kotlin.app.DeepLink
import club.touchtech.s5code.kotlin.app.parseDeepLinkPath
import club.touchtech.s5code.kotlin.app.parseDeepLinkUri
import club.touchtech.s5code.kotlin.platform.notifications.EXTRA_NOTIFICATION_PATH
import club.touchtech.s5code.kotlin.platform.notifications.notificationPathFromExtras

/** Extra a launcher shortcut carries: the in-app path it opens. */
const val EXTRA_SHORTCUT_PATH = "club.touchtech.s5code.kotlin.SHORTCUT_PATH"

/** Up to eight images, matching the send-turn attachment limit. */
private const val MAX_SHARED_IMAGES = 8

/**
 * Resolves what an incoming intent asks the app to open.
 *
 * Four entry points arrive as intents and all of them are untrusted input:
 *
 * - a **shortcut**, carrying a path this app wrote earlier (which may be stale
 *   after an update, so it is re-validated rather than trusted);
 * - a **deep link** (`VIEW` with an `s5code://` URI);
 * - a **share** of text or a URL;
 * - a **share** of one or more images.
 *
 * Returning null means "just show the app", which is the correct answer for a
 * plain launcher tap and for anything malformed.
 */
fun resolveIntentLink(intent: Intent?): DeepLink? {
    if (intent == null) return null

    intent.getStringExtra(EXTRA_SHORTCUT_PATH)?.let { path -> return parseDeepLinkPath(path) }
    intent.getStringExtra(EXTRA_NOTIFICATION_PATH)?.let { path -> return parseDeepLinkPath(path) }
    notificationPathFromExtras(intent::getStringExtra)?.let { path -> return parseDeepLinkPath(path) }

    return when (intent.action) {
        Intent.ACTION_VIEW -> intent.dataString?.let(::parseDeepLinkUri)
        Intent.ACTION_SEND -> sharedPayload(intent, multiple = false)
        Intent.ACTION_SEND_MULTIPLE -> sharedPayload(intent, multiple = true)
        else -> null
    }
}

/**
 * A share turns into a new-task draft. Text and images can arrive together (a
 * screenshot with a caption), so both are read rather than branching on the
 * intent's declared type, which lies often enough to matter.
 */
private fun sharedPayload(intent: Intent, multiple: Boolean): DeepLink? {
    val text =
        listOfNotNull(
                intent.getStringExtra(Intent.EXTRA_SUBJECT)?.takeIf { it.isNotBlank() },
                intent.getStringExtra(Intent.EXTRA_TEXT)?.takeIf { it.isNotBlank() },
            )
            .distinct()
            .joinToString("\n")
            .takeIf { it.isNotBlank() }

    val uris =
        if (multiple) {
            intent.parcelableUris(Intent.EXTRA_STREAM)
        } else {
            listOfNotNull(intent.parcelableUri(Intent.EXTRA_STREAM))
        }
    val images = uris.map(Uri::toString).take(MAX_SHARED_IMAGES)

    return if (text == null && images.isEmpty()) null else DeepLink.Share(text, images)
}

@Suppress("DEPRECATION")
private fun Intent.parcelableUri(key: String): Uri? =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        getParcelableExtra(key, Uri::class.java)
    } else {
        getParcelableExtra(key)
    }

@Suppress("DEPRECATION")
private fun Intent.parcelableUris(key: String): List<Uri> =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        getParcelableArrayListExtra(key, Uri::class.java).orEmpty()
    } else {
        getParcelableArrayListExtra<Uri>(key).orEmpty()
    }
