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

    /** RN's `environment-new` linking path — the add-environment form. */
    data object EnvironmentNew : DeepLink

    /**
     * RN's `connect-onboarding` linking path. RN has no managed-machines add
     * flow — connect devices appear on the environments list itself — so the
     * link lands there.
     */
    data object ConnectOnboarding : DeepLink

    data object Settings : DeepLink

    /** A settings sub-screen by its registered sub-route (`settings/auth`, …). */
    data class SettingsChild(val page: String) : DeepLink

    data object Archive : DeepLink

    data object Usage : DeepLink

    /**
     * RN's `add-project` linking paths. The environment and clone params are
     * optional on RN's side, so a bare step resolves the first creatable
     * environment the same way.
     */
    data class AddProject(val step: String) : DeepLink

    data class Thread(val environmentId: String, val threadId: String, val child: String? = null) :
        DeepLink

    /**
     * Payloads arriving from the system sharesheet. [uris] are the raw
     * `content:` stream URIs — the inbox copies them before the draft exists,
     * so nothing here holds a lapsed grant.
     */
    data class Share(val text: String?, val mimeType: String?, val uris: List<String>) : DeepLink

    /** The route this link opens. Deliberately built here, not by the caller. */
    val route: String
        get() =
            when (this) {
                Home -> Routes.Home
                NewTask -> Routes.NewTask
                Connections -> Routes.Connections
                EnvironmentNew -> Routes.ConnectionsNew
                ConnectOnboarding -> Routes.Connections
                Settings -> Routes.Settings
                is SettingsChild -> "settings/$page"
                Archive -> Routes.Archive
                Usage -> Routes.Usage
                is AddProject ->
                    when (step) {
                        "local" -> Routes.AddProjectLocal
                        "repository" -> Routes.AddProjectRepository
                        "destination" -> Routes.AddProjectDestination
                        else -> Routes.AddProjectSource
                    }
                is Thread ->
                    if (child == null) Routes.thread(environmentId, threadId)
                    else Routes.threadChild(environmentId, threadId, child)
                // A share lands on the project picker, matching the RN client:
                // the draft screen reserves the inbox entry once a project is
                // chosen, and "back" must return to that picker.
                is Share -> Routes.NewTask
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
    setOf(
        "terminal",
        "review",
        // RN registers `review-comment` as a linked route; the composer handles
        // absent params the same way RN's sheet does.
        "review-comment",
        "files",
        "git",
        "git/commit",
        "git/branches",
        "rewind",
    )

/**
 * Settings sub-routes an external link may open, matching the linking table in
 * `apps/mobile/src/Stack.tsx`. RN's `settings/legal` and
 * `settings/open-source-licenses` have no screen here yet, so they are not in
 * the allowlist.
 */
private val SETTINGS_CHILDREN =
    mapOf(
        "auth" to "auth",
        "waitlist" to "auth",
        "environments" to "environments",
        "appearance" to "appearance",
        "project-grouping" to "project-grouping",
        "client-storage" to "client-storage",
    )

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
        segments == listOf("environment-new") -> DeepLink.EnvironmentNew
        // RN registers `connect-onboarding` for the S5 Connect setup screen.
        segments == listOf("connect-onboarding") -> DeepLink.ConnectOnboarding
        segments == listOf("settings") -> DeepLink.Settings
        segments.size == 2 && segments[0] == "settings" ->
            SETTINGS_CHILDREN[segments[1]]?.let(DeepLink::SettingsChild)
        segments == listOf("archive") -> DeepLink.Archive
        segments == listOf("usage") -> DeepLink.Usage
        segments.firstOrNull() == "add-project" &&
            segments.getOrElse(1) { "" } in setOf("", "repository", "destination", "local") &&
            segments.size <= 2 ->
            DeepLink.AddProject(segments.getOrElse(1) { "" })
        segments.size >= 3 && segments[0] == "threads" -> {
            val environmentId = segments[1]
            val threadId = segments[2]
            if (environmentId.isBlank() || threadId.isBlank()) return null
            val rest = segments.drop(3)
            // A file deep link (`files/<path>`) carries the path as the route's
            // query parameter; a bare `files` is the tree. `attachments/<id>` is
            // the only parameterized child — the viewer defaults the missing
            // name/mime/size params rather than refusing the link.
            val child =
                if (rest.firstOrNull() == "files" && rest.size > 1) {
                    "files/source?path=${android.net.Uri.encode(rest.drop(1).joinToString("/"))}"
                } else {
                    rest.joinToString("/").takeIf { it.isNotEmpty() }
                }
            if (child != null && child !in THREAD_CHILDREN &&
                !child.startsWith("files/source?path=") &&
                !(rest.size == 2 && rest[0] == "attachments")
            ) {
                return null
            }
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
