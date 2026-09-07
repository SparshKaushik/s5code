package club.touchtech.s5code.kotlin.app

/**
 * What an external entry point asks the app to open.
 *
 * Shortcuts, notifications, and `s5code://` links all resolve to one of these, so
 * there is a single place that decides whether an outside caller's request is
 * legitimate. Anything unrecognized becomes null rather than a route string: a
 * launcher shortcut persists across app updates, and a stale one must navigate
 * nowhere instead of somewhere unexpected.
 */
sealed interface DeepLink {
    data object Home : DeepLink

    data object NewTask : DeepLink

    data object Connections : DeepLink

    data object Settings : DeepLink

    data object Archive : DeepLink

    data object Usage : DeepLink

    data class Thread(val environmentId: String, val threadId: String, val child: String? = null) :
        DeepLink

    /** Text or images arriving from the system sharesheet. */
    data class Share(val text: String?, val imageUris: List<String>) : DeepLink

    /** The route this link opens. Deliberately built here, not by the caller. */
    val route: String
        get() =
            when (this) {
                Home -> Routes.Home
                NewTask -> Routes.NewTask
                Connections -> Routes.Connections
                Settings -> Routes.Settings
                Archive -> Routes.Archive
                Usage -> Routes.Usage
                is Thread ->
                    if (child == null) Routes.thread(environmentId, threadId)
                    else Routes.threadChild(environmentId, threadId, child)
                // A share lands in the new-task draft; the payload is applied to
                // the draft before navigation.
                is Share -> Routes.NewTaskDraft
            }
}

/**
 * Thread sub-routes an external link may open, matching the linking table in
 * `apps/mobile/src/Stack.tsx`. An allowlist rather than a passthrough: the route
 * table has destinations (git confirmation, review comment) that only make sense
 * with state the app itself set up, and letting a link push them straight would
 * present a confirmation for an action nobody chose.
 */
private val THREAD_CHILDREN =
    setOf("terminal", "review", "files", "git", "git/commit", "git/branches", "rewind")

private const val MAX_SEGMENT_LENGTH = 256

/**
 * Parses an in-app path (`/threads/env/thread`, `/new`, `settings`) into a link.
 *
 * The same shapes the React Native client registers, so a notification payload or
 * a shortcut created by either client resolves identically. Ids are length-capped
 * and must be single segments: a `..` or an embedded slash would let a link
 * assemble a route the graph never declared.
 */
fun parseDeepLinkPath(raw: String): DeepLink? {
    val path = raw.substringBefore('?').substringBefore('#')
    if (path.trim() != path) return null
    val decoded = path.split('/').filter { it.isNotEmpty() }.map(::decode)
    if (decoded.any { it == null }) return null
    val segments = decoded.filterNotNull()
    if (segments.any { it.length > MAX_SEGMENT_LENGTH || it == ".." || it == "." }) return null

    return when {
        segments.isEmpty() -> DeepLink.Home
        segments == listOf("new") -> DeepLink.NewTask
        segments == listOf("connections") || segments == listOf("environments") ->
            DeepLink.Connections
        segments == listOf("settings") -> DeepLink.Settings
        segments == listOf("archive") -> DeepLink.Archive
        segments == listOf("usage") -> DeepLink.Usage
        segments.size >= 3 && segments[0] == "threads" -> {
            val environmentId = segments[1]
            val threadId = segments[2]
            if (environmentId.isBlank() || threadId.isBlank()) return null
            val child = segments.drop(3).joinToString("/").takeIf { it.isNotEmpty() }
            if (child != null && child !in THREAD_CHILDREN) return null
            DeepLink.Thread(environmentId, threadId, child)
        }
        else -> null
    }
}

private fun decode(value: String): String? =
    runCatching { java.net.URLDecoder.decode(value, "UTF-8") }.getOrNull()

/**
 * Parses a full `s5code://` URI. The host is treated as the first path segment,
 * because `s5code://new` and `s5code:///new` both occur in the wild and a user
 * typing the former should not get a different answer.
 */
fun parseDeepLinkUri(uriString: String): DeepLink? {
    val uri = runCatching { java.net.URI(uriString) }.getOrNull() ?: return null
    if (uri.scheme?.lowercase() != "s5code") return null
    val host = uri.host.orEmpty()
    val path = uri.rawPath.orEmpty()
    return parseDeepLinkPath(if (host.isEmpty()) path else "$host$path")
}
