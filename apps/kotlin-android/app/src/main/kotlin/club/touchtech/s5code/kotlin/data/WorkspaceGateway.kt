package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.BranchRef
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.Environment
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.FileNode
import club.touchtech.s5code.kotlin.model.GitStatus
import club.touchtech.s5code.kotlin.model.PendingApproval
import club.touchtech.s5code.kotlin.model.PendingUserInput
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.ProviderCatalogEntry
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.RepositoryRef
import club.touchtech.s5code.kotlin.model.ReviewFile
import club.touchtech.s5code.kotlin.model.SlashCommand
import club.touchtech.s5code.kotlin.model.SourceFile
import club.touchtech.s5code.kotlin.model.TerminalSession
import club.touchtech.s5code.kotlin.model.ThreadDetail
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadSearchMatch
import club.touchtech.s5code.kotlin.model.ThreadSettings
import club.touchtech.s5code.kotlin.model.ThreadSummary
import club.touchtech.s5code.kotlin.model.ThreadSyncPhase
import club.touchtech.s5code.kotlin.model.Usage
import club.touchtech.s5code.kotlin.model.UsageWindow
import club.touchtech.s5code.kotlin.model.WorkspaceAsset
import club.touchtech.s5code.kotlin.model.UserInputAnswer
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.StateFlow

/**
 * Everything the UI needs from the environments a device is paired with.
 *
 * Three shapes appear here, and the difference is deliberate:
 *
 * - **`StateFlow` for the merged workspace.** Environments, projects, and threads
 *   arrive over live subscriptions and change without being asked, so screens
 *   observe them.
 * - **`suspend` for tool reads.** A file tree, a diff, or a git status is fetched
 *   on demand and can fail. Making these suspend is what forces every screen to
 *   have a loading and an error state instead of pretending the data is local.
 * - **`suspend` for writes**, which return once the server has accepted the
 *   command, not once the UI has guessed at the result.
 *
 * Tool reads take an [EnvironmentId] as well as a [ThreadId] because thread ids
 * are only unique within one environment, and a device can be paired with
 * several.
 */
data class GitActionProgress(
    val action: String,
    val label: String,
    val description: String? = null,
)

data class GitActionResult(
    val action: String,
    val title: String,
    val description: String? = null,
    val pullRequestUrl: String? = null,
)

interface WorkspaceGateway {
    val environments: StateFlow<List<Environment>>
    val projects: StateFlow<List<Project>>
    val threads: StateFlow<List<ThreadSummary>>
    val archived: StateFlow<List<ThreadSummary>>

    /**
     * Called when the app becomes interactive after backgrounding. Implementations
     * should invalidate half-open mobile transports and re-establish live streams.
     */
    fun refreshConnections() = Unit

    /**
     * Actionable approval and structured-input requests from live threads.
     *
     * The shell only carries booleans, so the gateway subscribes to the small set
     * of threads whose rows say they need the user and projects their actual
     * request payloads here. This lets Home render real controls without opening a
     * transcript or subscribing to every thread in the workspace.
     */
    val pendingRequests: StateFlow<List<HomePendingRequest>>

    /**
     * Provider instances configured on the connected environments, merged. Empty
     * while nothing is connected, which is why the settings sheet shows the
     * current selection rather than an empty list in that state.
     */
    val providerCatalog: StateFlow<List<ProviderCatalogEntry>>

    /**
     * Slash commands the connected environments advertise for a provider instance,
     * merged. Not suspend: these ride along with the provider config the session
     * already holds, so the composer can filter them while the user types.
     */
    fun slashCommands(provider: ProviderInstance): List<SlashCommand>

    /**
     * The thread's live detail. Subscribing is what starts the per-thread stream,
     * so this is called only by screens that render a transcript.
     */
    fun thread(environmentId: EnvironmentId, id: ThreadId): StateFlow<ThreadDetail?>

    /**
     * Per-thread stream reconciliation phase. A cached transcript can be rendered
     * while this is [ThreadSyncPhase.Syncing], rather than moving the status up to
     * a screen-level wait notice.
     */
    fun threadSyncPhase(environmentId: EnvironmentId, id: ThreadId): StateFlow<ThreadSyncPhase>

    /**
     * Publishes the exact client-generated creation keys still owned by the
     * durable outbox. A thread screen may subscribe before bootstrap reaches the
     * server; gateways use this to treat that initial not-found as pending rather
     * than as a fatal stream failure.
     */
    fun setPendingThreadCreations(keys: Set<String>) = Unit

    /** Stops treating a rejected queued bootstrap as expected pre-creation latency. */
    fun discardPendingThreadCreation(key: String) = Unit

    /** Starts a turn. Returns once the command is accepted, not when it finishes. */
    suspend fun sendMessage(
        environmentId: EnvironmentId,
        id: ThreadId,
        text: String,
        attachments: List<ComposerAttachment> = emptyList(),
        settings: ThreadSettings? = null,
        delivery: TurnDeliveryMetadata? = null,
    )

    suspend fun cancelTurn(environmentId: EnvironmentId, id: ThreadId)

    suspend fun respondToApproval(
        environmentId: EnvironmentId,
        id: ThreadId,
        approvalId: String,
        decision: ApprovalDecision,
    )

    suspend fun respondToInput(
        environmentId: EnvironmentId,
        id: ThreadId,
        inputId: String,
        answers: Map<String, UserInputAnswer>,
    )

    /**
     * Searches persisted user and assistant messages on every selected environment.
     * A failed/offline environment contributes no hits so local shell-field search
     * remains useful while another machine reconnects.
     */
    suspend fun searchThreads(
        environmentIds: Set<EnvironmentId>,
        query: String,
        limitPerEnvironment: Int = 50,
    ): List<ThreadSearchMatch>

    /**
     * Creates a thread and starts its first turn in one command. The server
     * bootstraps the thread (and a worktree, when asked) as part of the turn, so
     * a failure cannot leave an empty thread behind.
     */
    suspend fun createThread(
        environmentId: EnvironmentId,
        projectKey: String,
        prompt: String,
        settings: ThreadSettings,
        branch: String,
        newWorktree: Boolean,
        attachments: List<ComposerAttachment> = emptyList(),
        threadId: ThreadId? = null,
        delivery: TurnDeliveryMetadata? = null,
    ): ThreadId

    suspend fun updateThreadSettings(
        environmentId: EnvironmentId,
        id: ThreadId,
        settings: ThreadSettings,
    )

    suspend fun regenerateTitle(environmentId: EnvironmentId, id: ThreadId)

    suspend fun setPinned(environmentId: EnvironmentId, id: ThreadId, pinned: Boolean)

    suspend fun setArchived(environmentId: EnvironmentId, id: ThreadId, archived: Boolean)

    suspend fun setSettled(environmentId: EnvironmentId, id: ThreadId, settled: Boolean)

    suspend fun setSnoozed(
        environmentId: EnvironmentId,
        id: ThreadId,
        snoozed: Boolean,
        untilIso: String? = null,
    )

    suspend fun deleteThread(environmentId: EnvironmentId, id: ThreadId)

    /**
     * Reverts the worktree to a checkpoint. Destructive and forward-only: newer
     * turns are dropped, which is why the UI confirms first.
     */
    suspend fun revertToCheckpoint(environmentId: EnvironmentId, id: ThreadId, turnCount: Int)

    /* ── Tool reads ──────────────────────────────────────────────────── */

    suspend fun files(environmentId: EnvironmentId, id: ThreadId, path: String? = null): FileNode

    suspend fun sourceFile(environmentId: EnvironmentId, id: ThreadId, path: String): SourceFile

    /**
     * An absolute, signed URL for one workspace file, for content that cannot be
     * read as text. The server issues a short-lived token in the path, so the
     * returned URL is fetchable with a plain GET and expires on its own. The live
     * gateway caches it only until shortly before that declared expiry.
     */
    suspend fun asset(
        environmentId: EnvironmentId,
        id: ThreadId,
        path: String,
    ): WorkspaceAsset

    suspend fun assetUrl(environmentId: EnvironmentId, id: ThreadId, path: String): String =
        asset(environmentId, id, path).url

    /**
     * Starts the same read an imminent viewer will use. Failures are deliberately
     * quiet: selection remains the authoritative load and shows its own retry UI.
     */
    fun prewarmFile(environmentId: EnvironmentId, id: ThreadId, path: String)

    /** Signs an image already attached to a transcript message. */
    suspend fun attachmentUrl(environmentId: EnvironmentId, attachmentId: String): String

    /**
     * An absolute, signed URL for a project's icon, or null when the project has
     * none (or the server predates project favicons).
     *
     * Separate from [assetUrl] because it is keyed on the workspace root rather
     * than a thread: the home list renders project icons on rows whose threads it
     * has not subscribed to. The server buckets these tokens to a half-hour
     * boundary, so the URL is stable long enough to be worth caching by URL.
     */
    suspend fun projectIconUrl(project: Project): String?

    suspend fun review(environmentId: EnvironmentId, id: ThreadId, refresh: Boolean = false): List<ReviewFile>

    suspend fun gitStatus(environmentId: EnvironmentId, id: ThreadId): GitStatus

    suspend fun branches(environmentId: EnvironmentId, id: ThreadId): List<BranchRef>

    /** Branches for a project that has no thread yet, used by the new-task flow. */
    suspend fun projectBranches(environmentId: EnvironmentId, projectKey: String): List<BranchRef>

    /**
     * A live terminal session: opens or reattaches, then folds output into lines.
     *
     * A stream rather than a read, because a terminal has no useful snapshot — the
     * echo of what you typed arrives as a later frame. Cancelling the collector
     * detaches but leaves the shell running, which is what makes leaving the screen
     * and coming back land in the same session.
     *
     * [cols] and [rows] size the PTY on attach; a running session is resized to
     * match. Use [terminalResize] afterwards when the surface changes.
     */
    fun terminal(
        environmentId: EnvironmentId,
        id: ThreadId,
        terminalId: String = DEFAULT_TERMINAL_ID,
        cols: Int = DEFAULT_TERMINAL_COLS,
        rows: Int = DEFAULT_TERMINAL_ROWS,
    ): Flow<TerminalSession>

    /**
     * Workspace paths matching a partial `@` mention, from the server's file
     * index. An empty query is a bounded browse rather than everything.
     */
    suspend fun searchPaths(
        environmentId: EnvironmentId,
        id: ThreadId,
        query: String,
        limit: Int = 20,
    ): List<String>

    /**
     * Merged usage over one window. A request rather than a subscription: every
     * environment scans its own provider transcripts, which is slow enough that the
     * screen shows a loading state for it.
     */
    suspend fun usage(window: UsageWindow = UsageWindow.Month): Usage

    suspend fun repositories(environmentId: EnvironmentId, query: String): List<RepositoryRef>

    suspend fun remotePaths(environmentId: EnvironmentId, partialPath: String): List<String>

    /* ── Projects ────────────────────────────────────────────────────── */

    /**
     * Registers a project rooted at an existing directory on the machine.
     *
     * [createWorkspaceRootIfMissing] is what separates "point at a folder I have"
     * from "make this folder for me"; the caller decides, because creating a
     * directory on someone's machine by accident is not recoverable from here.
     */
    suspend fun createProject(
        environmentId: EnvironmentId,
        title: String,
        workspaceRoot: String,
        createWorkspaceRootIfMissing: Boolean = false,
    )

    /**
     * Clones a repository on the machine and registers the result as a project.
     * Returns the directory the clone landed in, which is not always the
     * destination that was asked for.
     */
    suspend fun cloneProject(
        environmentId: EnvironmentId,
        repository: String,
        destinationPath: String,
    ): String

    /* ── Git writes ──────────────────────────────────────────────────── */

    suspend fun commit(
        environmentId: EnvironmentId,
        id: ThreadId,
        message: String,
        onProgress: (GitActionProgress) -> Unit = {},
    ): GitActionResult

    suspend fun push(
        environmentId: EnvironmentId,
        id: ThreadId,
        onProgress: (GitActionProgress) -> Unit = {},
    ): GitActionResult

    suspend fun createPullRequest(
        environmentId: EnvironmentId,
        id: ThreadId,
        onProgress: (GitActionProgress) -> Unit = {},
    ): GitActionResult

    suspend fun pull(environmentId: EnvironmentId, id: ThreadId)

    suspend fun createBranch(environmentId: EnvironmentId, id: ThreadId, name: String)

    suspend fun switchBranch(environmentId: EnvironmentId, id: ThreadId, name: String)

    /* ── Terminal ────────────────────────────────────────────────────── */

    suspend fun terminalWrite(
        environmentId: EnvironmentId,
        id: ThreadId,
        terminalId: String,
        data: String,
    )

    /** Tells the PTY how big the surface is, so line wrapping matches what is drawn. */
    suspend fun terminalResize(
        environmentId: EnvironmentId,
        id: ThreadId,
        terminalId: String,
        cols: Int,
        rows: Int,
    )

    /** Clears server scrollback and resets the attached VT surface. */
    suspend fun terminalClear(
        environmentId: EnvironmentId,
        id: ThreadId,
        terminalId: String,
    )

    /** Starts a fresh shell in the same workspace and keeps the terminal id. */
    suspend fun terminalRestart(
        environmentId: EnvironmentId,
        id: ThreadId,
        terminalId: String,
        cols: Int,
        rows: Int,
    )

    /** Ends the session and its shell. The scrollback is kept unless asked otherwise. */
    suspend fun terminalClose(
        environmentId: EnvironmentId,
        id: ThreadId,
        terminalId: String,
        deleteHistory: Boolean = false,
    )
}

/**
 * Id of the first shell on a thread, from `DEFAULT_TERMINAL_ID` in
 * `packages/contracts/src/terminal.ts`. Ids are client-chosen and always sent
 * explicitly; the server allocates nothing.
 */
const val DEFAULT_TERMINAL_ID = "term-1"

enum class ApprovalDecision {
    AllowOnce,
    AllowAlways,
    Deny,
}

/** One actionable request shown on Home, keyed by environment and thread. */
data class HomePendingRequest(
    val environmentId: EnvironmentId,
    val threadId: ThreadId,
    val approval: PendingApproval? = null,
    val userInput: PendingUserInput? = null,
) {
    init {
        require((approval == null) xor (userInput == null)) {
            "A pending request must contain exactly one request payload."
        }
    }

    val key: String =
        approval?.let { "approval:${environmentId.value}:${threadId.value}:${it.id}" }
            ?: "input:${environmentId.value}:${threadId.value}:${requireNotNull(userInput).id}"
}
