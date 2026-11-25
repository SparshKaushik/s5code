package club.touchtech.s5code.kotlin.platform

import android.content.Context
import android.content.Intent
import androidx.core.content.pm.ShortcutInfoCompat
import androidx.core.content.pm.ShortcutManagerCompat
import androidx.core.graphics.drawable.IconCompat
import club.touchtech.s5code.kotlin.MainActivity
import club.touchtech.s5code.kotlin.R
import club.touchtech.s5code.kotlin.data.StoredRecentThread

/**
 * Publishes launcher shortcuts: a static "New task" plus recently opened threads.
 *
 * The path travels in an extra rather than as the intent's data URI so it cannot
 * be confused with a deep link from outside the app — and it is still re-validated
 * on the way in, because a shortcut persists in the launcher across app updates
 * and an old one may name a route this build no longer has.
 *
 * Dynamic shortcuts are replaced wholesale on every publish. Reconciling them
 * individually would need the same ordering logic twice, and the list is at most
 * four entries.
 */
fun publishShortcuts(context: Context, recents: List<StoredRecentThread>) {
    val icon = IconCompat.createWithResource(context, R.drawable.ic_launcher_foreground)
    val shortcuts =
        listOf(
            ShortcutInfoCompat.Builder(context, "new-task")
                .setShortLabel("New task")
                .setLongLabel("Start a new task")
                .setIcon(icon)
                .setIntent(shortcutIntent(context, "/new"))
                .build()
        ) +
            recents.map { thread ->
                val path = "/threads/${encode(thread.environmentId)}/${encode(thread.threadId)}"
                ShortcutInfoCompat.Builder(context, "thread:$path")
                    .setShortLabel(thread.title.ifBlank { "Thread" }.take(SHORT_LABEL_LENGTH))
                    .setLongLabel(thread.title.ifBlank { "Thread" }.take(LONG_LABEL_LENGTH))
                    .setIcon(icon)
                    .setIntent(shortcutIntent(context, path))
                    .build()
            }
    runCatching { ShortcutManagerCompat.setDynamicShortcuts(context, shortcuts) }
}

private fun shortcutIntent(context: Context, path: String): Intent =
    Intent(context, MainActivity::class.java)
        .setAction(Intent.ACTION_MAIN)
        .putExtra(EXTRA_SHORTCUT_PATH, path)

private fun encode(value: String): String = java.net.URLEncoder.encode(value, "UTF-8")

// Launcher limits, from the platform's own guidance: short labels are truncated
// hard on the home screen, long ones in the long-press sheet.
private const val SHORT_LABEL_LENGTH = 10
private const val LONG_LABEL_LENGTH = 25
