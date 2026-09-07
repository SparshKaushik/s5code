package club.touchtech.s5code.kotlin.app

/**
 * Route table. Paths intentionally mirror the React Native client's deep links
 * (`apps/mobile/src/Stack.tsx`) so notification, widget, and shortcut payloads
 * resolve identically once platform integrations land.
 */
object Routes {
    const val Bootstrap = "bootstrap"
    const val Onboarding = "onboarding"
    // Pairing is reachable from first run and from Connections, and the two want
    // different exits. The origin travels in the route rather than being sniffed
    // from the back stack, which is a restricted API and a lie once a deep link
    // lands straight on the pair screen.
    const val PairUrl = "pair/url?onboarding={onboarding}"
    const val PairQr = "pair/qr?onboarding={onboarding}"
    const val ConnectSignIn = "connect/sign-in?onboarding={onboarding}"
    const val ConnectSetup = "connect-onboarding?onboarding={onboarding}"

    const val Home = "home"
    /** Detail-pane placeholder used only on expanded widths. */
    const val WorkspaceEmpty = "workspace-empty"
    const val Connections = "connections"
    const val ConnectionsNew = "connections/new"
    const val ConnectionDetail = "connections/{environmentId}"

    const val NewTask = "new"
    const val NewTaskDraft = "new/draft"
    const val NewTaskEnvironment = "new/draft/environment"
    const val NewTaskBranch = "new/draft/branch"
    const val AddProjectSource = "new/add-project"
    const val AddProjectRepository = "new/add-project/repository"
    const val AddProjectDestination = "new/add-project/destination"
    const val AddProjectLocal = "new/add-project/local"

    const val Thread = "threads/{environmentId}/{threadId}"
    const val ThreadRewind = "threads/{environmentId}/{threadId}/rewind"
    const val ThreadFiles = "threads/{environmentId}/{threadId}/files"
    // File viewers carry the path as a query parameter so a deep link (or a
    // notification) can open one file directly rather than a tree.
    const val ThreadFile = "threads/{environmentId}/{threadId}/files/source?path={path}"
    const val ThreadFileMarkdown = "threads/{environmentId}/{threadId}/files/markdown?path={path}"
    const val ThreadFileImage = "threads/{environmentId}/{threadId}/files/image?path={path}"
    const val ThreadFileWeb = "threads/{environmentId}/{threadId}/files/web?path={path}"
    const val ThreadReview = "threads/{environmentId}/{threadId}/review"
    const val ThreadReviewComment =
        "threads/{environmentId}/{threadId}/review-comment?filePath={filePath}&startIndex={startIndex}&endIndex={endIndex}"
    const val ThreadTerminal = "threads/{environmentId}/{threadId}/terminal"
    const val Git = "threads/{environmentId}/{threadId}/git"
    const val GitCommit = "threads/{environmentId}/{threadId}/git/commit"
    const val GitBranches = "threads/{environmentId}/{threadId}/git/branches"
    const val GitConfirm = "threads/{environmentId}/{threadId}/git-confirm"
    const val SourceControl = "threads/{environmentId}/{threadId}/source-control"
    const val PullRequests = "threads/{environmentId}/{threadId}/pull-requests"

    const val Settings = "settings"
    const val SettingsAccount = "settings/auth"
    const val SettingsEnvironments = "settings/environments"
    const val SettingsAppearance = "settings/appearance"
    const val SettingsProjectGrouping = "settings/project-grouping"
    const val SettingsNotifications = "settings/notifications"
    const val SettingsLiveUpdates = "settings/live-updates"
    const val SettingsClientStorage = "settings/client-storage"
    const val Usage = "usage"
    const val Archive = "archive"
    const val NotFound = "not-found"

    fun pairUrl(onboarding: Boolean) = "pair/url?onboarding=$onboarding"

    fun pairQr(onboarding: Boolean) = "pair/qr?onboarding=$onboarding"

    fun connectSignIn(onboarding: Boolean) = "connect/sign-in?onboarding=$onboarding"

    fun connectSetup(onboarding: Boolean) = "connect-onboarding?onboarding=$onboarding"

    fun thread(environmentId: String, threadId: String) =
        "threads/${routeSegment(environmentId)}/${routeSegment(threadId)}"

    fun threadChild(environmentId: String, threadId: String, suffix: String) =
        "threads/$environmentId/$threadId/$suffix"

    fun threadReviewComment(
        environmentId: String,
        threadId: String,
        filePath: String,
        startIndex: Int,
        endIndex: Int,
    ) = threadChild(
        environmentId,
        threadId,
        "review-comment?filePath=${android.net.Uri.encode(filePath)}&startIndex=$startIndex&endIndex=$endIndex",
    )

    fun connectionDetail(environmentId: String) = "connections/$environmentId"

    /** Source viewer for one file, used when a preview cannot render it. */
    fun threadFileSource(environmentId: String, threadId: String, path: String) =
        threadChild(environmentId, threadId, "files/source?path=${android.net.Uri.encode(path)}")

    /** The same type-sensitive destination used by the workspace file tree. */
    fun threadFilePreview(environmentId: String, threadId: String, path: String) =
        threadChild(environmentId, threadId, "${fileRouteSuffix(path)}?path=${android.net.Uri.encode(path)}")

    fun fileRouteSuffix(path: String): String {
        val lower = path.substringBefore('?').substringBefore('#').lowercase()
        return when {
            lower.endsWith(".md") || lower.endsWith(".mdx") -> "files/markdown"
            IMAGE_PREVIEW_EXTENSIONS.any(lower::endsWith) -> "files/image"
            BROWSER_PREVIEW_EXTENSIONS.any(lower::endsWith) -> "files/web"
            else -> "files/source"
        }
    }

    private fun routeSegment(value: String): String =
        java.net.URLEncoder.encode(value, Charsets.UTF_8.name()).replace("+", "%20")

    private val IMAGE_PREVIEW_EXTENSIONS =
        listOf(".avif", ".gif", ".ico", ".jpeg", ".jpg", ".png", ".svg", ".webp")
    private val BROWSER_PREVIEW_EXTENSIONS = listOf(".htm", ".html", ".pdf")
}
