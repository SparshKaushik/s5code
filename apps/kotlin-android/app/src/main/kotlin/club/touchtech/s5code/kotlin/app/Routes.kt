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
    const val PairUrl = "pair/url?onboarding={onboarding}&host={host}&code={code}&scan={scan}"
    const val ConnectSignIn = "connect/sign-in?onboarding={onboarding}"

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
    const val AddProjectRepository =
        "new/add-project/repository?environmentId={environmentId}&source={source}"
    const val AddProjectDestination =
        "new/add-project/destination?environmentId={environmentId}&source={source}" +
            "&remoteUrl={remoteUrl}&repositoryTitle={repositoryTitle}&repositoryName={repositoryName}"
    const val AddProjectLocal = "new/add-project/local?environmentId={environmentId}"

    const val Thread = "threads/{environmentId}/{threadId}"
    const val ThreadRewind = "threads/{environmentId}/{threadId}/rewind"
    const val ThreadFiles = "threads/{environmentId}/{threadId}/files"
    // File viewers carry the path as a query parameter so a deep link (or a
    // notification) can open one file directly rather than a tree.
    const val ThreadFile = "threads/{environmentId}/{threadId}/files/source?path={path}"
    const val ThreadFileMarkdown = "threads/{environmentId}/{threadId}/files/markdown?path={path}"
    const val ThreadFileImage = "threads/{environmentId}/{threadId}/files/image?path={path}"
    const val ThreadFileWeb = "threads/{environmentId}/{threadId}/files/web?path={path}"
    // The attachment viewer carries name/mime/size as query params: there is no
    // metadata RPC for an attachment, so the route is the record (RN does the
    // same in `AttachmentFileRouteParams`).
    const val ThreadAttachment =
        "threads/{environmentId}/{threadId}/attachments/{attachmentId}?name={name}&mimeType={mimeType}&sizeBytes={sizeBytes}"
    const val ThreadReview = "threads/{environmentId}/{threadId}/review"
    const val ThreadReviewComment =
        "threads/{environmentId}/{threadId}/review-comment?filePath={filePath}&startIndex={startIndex}&endIndex={endIndex}"
    // `terminalId` is a query param, not a path segment, matching the RN
    // route's `terminalId?` param — a bare `/terminal` means the default shell.
    const val ThreadTerminal = "threads/{environmentId}/{threadId}/terminal?terminalId={terminalId}"
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
    const val SettingsClientStorage = "settings/client-storage"
    const val Usage = "usage"
    const val Archive = "archive"
    const val NotFound = "not-found"

    fun pairUrl(
        onboarding: Boolean,
        host: String = "",
        code: String = "",
        scan: Boolean = false,
    ) = "pair/url?onboarding=$onboarding" +
        "&host=${android.net.Uri.encode(host)}" +
        "&code=${android.net.Uri.encode(code)}" +
        "&scan=$scan"

    fun connectSignIn(onboarding: Boolean) = "connect/sign-in?onboarding=$onboarding"

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

    fun addProjectRepository(environmentId: String, source: String) =
        "new/add-project/repository?environmentId=${android.net.Uri.encode(environmentId)}" +
            "&source=${android.net.Uri.encode(source)}"

    fun addProjectLocal(environmentId: String) =
        "new/add-project/local?environmentId=${android.net.Uri.encode(environmentId)}"

    fun addProjectDestination(
        environmentId: String,
        source: String,
        remoteUrl: String,
        repositoryTitle: String,
        repositoryName: String,
    ) = "new/add-project/destination?environmentId=${android.net.Uri.encode(environmentId)}" +
        "&source=${android.net.Uri.encode(source)}" +
        "&remoteUrl=${android.net.Uri.encode(remoteUrl)}" +
        "&repositoryTitle=${android.net.Uri.encode(repositoryTitle)}" +
        "&repositoryName=${android.net.Uri.encode(repositoryName)}"

    /** One terminal session; null id means the default shell (`term-1`). */
    fun threadTerminal(environmentId: String, threadId: String, terminalId: String? = null) =
        threadChild(
            environmentId,
            threadId,
            if (terminalId == null) "terminal"
            else "terminal?terminalId=${android.net.Uri.encode(terminalId)}",
        )

    /** Source viewer for one file, used when a preview cannot render it. */
    fun threadFileSource(environmentId: String, threadId: String, path: String) =
        threadChild(environmentId, threadId, "files/source?path=${android.net.Uri.encode(path)}")

    /** The viewer a sent or answer-staged attachment opens in. */
    fun threadAttachment(
        environmentId: String,
        threadId: String,
        attachmentId: String,
        name: String,
        mimeType: String,
        sizeBytes: Long,
    ) = threadChild(
        environmentId,
        threadId,
        "attachments/${routeSegment(attachmentId)}" +
            "?name=${android.net.Uri.encode(name)}" +
            "&mimeType=${android.net.Uri.encode(mimeType)}" +
            "&sizeBytes=$sizeBytes",
    )

    /** The same type-sensitive destination used by the workspace file tree. */
    fun threadFilePreview(environmentId: String, threadId: String, path: String) =
        threadChild(environmentId, threadId, "${fileRouteSuffix(path)}?path=${android.net.Uri.encode(path)}")

    /**
     * The viewer a workspace file opens in, following `defaultViewMode` in
     * `ThreadFilesRouteScreen`: browser/image/video/audio are previews and
     * everything else is source. Video and audio share the web viewer because
     * the WebView's own media element plays them.
     */
    fun fileRouteSuffix(path: String): String {
        val lower = path.substringBefore('?').substringBefore('#').lowercase()
        return when {
            MARKDOWN_PREVIEW_EXTENSIONS.any(lower::endsWith) -> "files/markdown"
            IMAGE_PREVIEW_EXTENSIONS.any(lower::endsWith) -> "files/image"
            BROWSER_PREVIEW_EXTENSIONS.any(lower::endsWith) ||
                AUDIO_PREVIEW_EXTENSIONS.any(lower::endsWith) ||
                VIDEO_PREVIEW_EXTENSIONS.any(lower::endsWith) -> "files/web"
            else -> "files/source"
        }
    }

    private fun routeSegment(value: String): String =
        java.net.URLEncoder.encode(value, Charsets.UTF_8.name()).replace("+", "%20")

    private val IMAGE_PREVIEW_EXTENSIONS =
        listOf(".avif", ".gif", ".ico", ".jpeg", ".jpg", ".png", ".svg", ".webp")

    // `WORKSPACE_BROWSER_PREVIEW_EXTENSIONS` in packages/shared/src/filePreview.ts.
    // PDF lands here to match: Android WebView cannot render it inline, so the
    // preview screen offers the signed URL to an external viewer instead.
    private val BROWSER_PREVIEW_EXTENSIONS = listOf(".htm", ".html", ".pdf")

    // `AUDIO_MIME_TYPE_BY_EXTENSION` keys in the same file.
    private val AUDIO_PREVIEW_EXTENSIONS =
        listOf(".mp3", ".wav", ".ogg", ".oga", ".flac", ".aac", ".m4a", ".opus", ".aiff")

    // `VIDEO_MIME_TYPE_BY_EXTENSION` keys in packages/shared/src/video.ts.
    private val VIDEO_PREVIEW_EXTENSIONS =
        listOf(".avi", ".m4v", ".mkv", ".mov", ".mp4", ".ogv", ".webm")

    private val MARKDOWN_PREVIEW_EXTENSIONS =
        listOf(".md", ".markdown", ".mdown", ".mkd", ".mdx")
}
