package club.touchtech.s5code.kotlin.model

/**
 * Presentation models the screens render.
 *
 * They mirror the shapes the React Native client renders
 * (`EnvironmentThreadShell`, `EnvironmentProject`, thread feed entries), which is
 * what lets `data/Projection.kt` map wire DTOs onto them without any screen
 * knowing the wire format.
 */

@JvmInline value class EnvironmentId(val value: String)

@JvmInline value class ProjectId(val value: String)

@JvmInline value class ThreadId(val value: String)

enum class ConnectionState {
    Connecting,
    Connected,
    Recovering,
    Offline,
    AuthRequired,
}

enum class EnvironmentKind {
    Direct,
    Cloud,
}

data class Environment(
    val id: EnvironmentId,
    val label: String,
    val host: String,
    val kind: EnvironmentKind,
    val state: ConnectionState,
    val lastSeenLabel: String,
    val devices: List<EnvironmentDevice> = emptyList(),
    val serverVersion: String = "0.5.2",
    /**
     * What this environment's server supports. Defaulted off, because a command
     * the server does not understand is a protocol defect that kills the socket
     * rather than a rejected request — so these gate write paths, not just which
     * buttons are drawn.
     */
    val capabilities: EnvironmentCapabilities = EnvironmentCapabilities(),
)

/**
 * Server capabilities as a screen needs them.
 *
 * Restated here rather than reused from `transport.ServerCapabilities` so the
 * model layer stays free of the transport: this is what a row reads to decide
 * whether it can offer settle or snooze at all.
 */
data class EnvironmentCapabilities(
    val threadSettlement: Boolean = false,
    val threadSnooze: Boolean = false,
    val threadPinning: Boolean = false,
    val threadTitleRegeneration: Boolean = false,
)

data class EnvironmentDevice(
    val name: String,
    val platform: String,
    val reachable: Boolean,
    val lastSeenLabel: String,
)

data class Project(
    val id: ProjectId,
    val environmentId: EnvironmentId,
    val title: String,
    val workspaceRoot: String,
    val repository: String?,
    val branch: String,
    /**
     * The project's icon path, relative to [workspaceRoot], when the server found
     * one. Null means "no icon", which rows render as a folder rather than as a
     * gap.
     */
    val faviconPath: String? = null,
)

enum class ThreadStatus {
    Working,
    AwaitingApproval,
    AwaitingInput,
    Failed,
    Idle,
    Settled,
    Snoozed,
    Queued,
}

data class PullRequestRef(val number: Int, val state: PullRequestState, val title: String)

enum class PullRequestState {
    Open,
    Merged,
    Closed,
    Draft,
}

data class ThreadSummary(
    val id: ThreadId,
    val environmentId: EnvironmentId,
    val projectId: ProjectId,
    val title: String,
    val status: ThreadStatus,
    val provider: ProviderInstance,
    val model: String,
    val branch: String?,
    val updatedLabel: String,
    /**
     * When the thread last changed, in epoch millis. Sorting uses this rather than
     * [updatedLabel]: the label is rounded for display, so ordering on it puts a
     * four-minute-old thread and a fifty-minute-old one in the same bucket.
     */
    val updatedAtMillis: Long = 0,
    val pinned: Boolean = false,
    val snoozedUntilLabel: String? = null,
    val lastError: String? = null,
    val pullRequest: PullRequestRef? = null,
    val changedFiles: Int = 0,
    val additions: Int = 0,
    val deletions: Int = 0,
    val archived: Boolean = false,
    val elapsedLabel: String? = null,
    /** Epoch millis for the active turn, used by visible Home rows to tick locally. */
    val activeTurnStartedAtMillis: Long? = null,
    val excerpt: String? = null,
    /** Non-null on the wire while the server is generating a replacement title. */
    val titleRegenerating: Boolean = false,
)

/** One entry in the thread transcript. */
sealed interface FeedEntry {
    val id: String

    /**
     * The turn this row belongs to, when the server attributed it to one.
     *
     * Carried on the row rather than derived, because folding a finished turn down
     * to its answer needs to know which rows belong to that turn, and only the
     * server knows: a tool call and the message that requested it can be minutes
     * apart.
     */
    val turnId: String?
        get() = null

    /** When the row happened, epoch millis. Zero when the server sent no timestamp. */
    val atMillis: Long
        get() = 0

    /** When the row last changed. Same as [atMillis] for anything that cannot grow. */
    val endedAtMillis: Long
        get() = atMillis

    data class UserMessage(
        override val id: String,
        val text: String,
        val timeLabel: String,
        val attachments: List<ComposerAttachment> = emptyList(),
        override val atMillis: Long = 0,
    ) : FeedEntry

    data class AgentMessage(
        override val id: String,
        val markdown: String,
        val timeLabel: String,
        val streaming: Boolean = false,
        val attachments: List<ComposerAttachment> = emptyList(),
        override val turnId: String? = null,
        override val atMillis: Long = 0,
        override val endedAtMillis: Long = atMillis,
    ) : FeedEntry

    data class ToolCall(
        override val id: String,
        val name: String,
        val summary: String,
        val detail: String,
        val state: ToolState,
        override val turnId: String? = null,
        override val atMillis: Long = 0,
    ) : FeedEntry

    data class Reasoning(
        override val id: String,
        val text: String,
        override val turnId: String? = null,
        override val atMillis: Long = 0,
    ) : FeedEntry

    data class PlanUpdate(
        override val id: String,
        val steps: List<PlanStep>,
        override val turnId: String? = null,
        override val atMillis: Long = 0,
    ) : FeedEntry

    data class Subagent(
        override val id: String,
        val name: String,
        val task: String,
        val active: Boolean,
        override val turnId: String? = null,
        override val atMillis: Long = 0,
    ) : FeedEntry

    data class TurnDivider(override val id: String, val label: String) : FeedEntry

    data class ErrorEntry(
        override val id: String,
        val message: String,
        override val turnId: String? = null,
        override val atMillis: Long = 0,
    ) : FeedEntry
}

enum class ToolState {
    Running,
    Succeeded,
    Failed,
}

data class PlanStep(val text: String, val state: PlanStepState)

enum class PlanStepState {
    Pending,
    Active,
    Done,
}

data class PendingApproval(
    val id: String,
    val title: String,
    val detail: String,
    val command: String?,
    val kind: ApprovalKind,
)

enum class ApprovalKind {
    Command,
    FileWrite,
    NetworkAccess,
}

data class PendingUserInput(
    /** The provider's request id, which one response command answers. */
    val id: String,
    /** Every question must be answered before the request-wide response is sent. */
    val questions: List<UserInputQuestion>,
)

data class UserInputQuestion(
    /** Answers are a record keyed by this provider-owned id. */
    val id: String,
    val header: String,
    val prompt: String,
    val kind: UserInputKind,
    val options: List<String>,
)

sealed interface UserInputAnswer {
    data class Text(val value: String) : UserInputAnswer

    data class Choices(val values: List<String>) : UserInputAnswer
}

enum class UserInputKind {
    Text,
    SingleSelect,
    MultiSelect,
}

enum class ThreadSyncPhase {
    /** A cached transcript is visible while the server catches up. */
    Syncing,
    /** No live snapshot has arrived yet. */
    Loading,
    /** The server marked the subscription synchronized. */
    Live,
}

data class ThreadDetail(
    val summary: ThreadSummary,
    val feed: List<FeedEntry>,
    val approval: PendingApproval?,
    val userInput: PendingUserInput?,
    /** Active worktree root when present; project root is supplied by the screen otherwise. */
    val workspaceRoot: String? = null,
    val queuedMessages: Int = 0,
    /** Whether the live detail stream has reached its completion marker. */
    val syncPhase: ThreadSyncPhase = ThreadSyncPhase.Live,
    val checkpoints: List<Checkpoint> = emptyList(),
    /**
     * The most recent turn, or null on a thread that has never run. Needed by the
     * transcript to know which turn is still open: the open one is the only one
     * that must not fold.
     */
    val latestTurn: TurnInfo? = null,
    /**
     * The provider session's own status (`idle`, `starting`, `running`, `ready`,
     * `interrupted`, `stopped`, `error`), or null on a thread with no session yet.
     *
     * Kept as the raw string for the same reason as [TurnInfo.state]: the contract's
     * set is open on the server side. The transcript needs it because the turn record
     * alone cannot tell a finished turn from one whose completion arrived before the
     * orchestrator was done with it.
     */
    val sessionStatus: String? = null,
    /** When the session last changed, in epoch millis. Stands in for a turn start during `starting`. */
    val sessionUpdatedAtMillis: Long? = null,
    /**
     * Full run settings for the thread. [ThreadSummary] carries only provider and
     * model because that is all the home list renders; the composer's settings
     * sheet needs mode, effort, and policy too, and those belong to the open
     * thread rather than every row in a list.
     */
    val settings: ThreadSettings = ThreadSettings(),
)

/**
 * A turn's lifecycle, from `LatestTurn` on the thread snapshot.
 *
 * [state] stays a string because the contract's set is open on the server side and
 * the client only ever asks two questions of it: is it running, and was it
 * interrupted.
 */
data class TurnInfo(
    val turnId: String,
    val state: String,
    val startedAtMillis: Long? = null,
    val completedAtMillis: Long? = null,
) {
    /** A turn is open until it has both completed and stopped running. */
    val settled get() = completedAtMillis != null && state != "running"

    val interrupted get() = state == "interrupted"
}

data class Checkpoint(
    val id: String,
    val label: String,
    val timeLabel: String,
    val filesChanged: Int,
    val current: Boolean = false,
)

/* ── Workspace tools ─────────────────────────────────────────────────── */

data class FileNode(
    val path: String,
    val name: String,
    val isDirectory: Boolean,
    val children: List<FileNode> = emptyList(),
    val sizeLabel: String? = null,
    val truncated: Boolean = false,
)

data class SourceFile(
    val path: String,
    val language: String,
    val lines: List<String>,
    val byteLength: Long = lines.sumOf { it.length.toLong() },
    val truncated: Boolean = false,
)

data class WorkspaceAsset(
    val url: String,
    val expiresAtMillis: Long,
    val sourcePath: String? = null,
)

data class ReviewFile(
    val path: String,
    val additions: Int,
    val deletions: Int,
    val status: ReviewFileStatus,
    val hunks: List<DiffHunk>,
)

enum class ReviewFileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Binary,
}

data class DiffHunk(val header: String, val lines: List<DiffLine>)

data class DiffLine(val kind: DiffLineKind, val text: String, val oldNo: Int?, val newNo: Int?)

enum class DiffLineKind {
    Context,
    Added,
    Removed,
}

data class GitStatus(
    val branch: String,
    val baseBranch: String,
    val ahead: Int,
    val behind: Int,
    val staged: List<String>,
    val unstaged: List<String>,
    val untracked: List<String>,
    val worktreePath: String?,
    val pullRequest: PullRequestRef?,
)

data class BranchRef(val name: String, val current: Boolean, val remote: Boolean, val ageLabel: String)

/** From `TerminalSessionStatus` in `packages/contracts/src/terminal.ts`, plus closed. */
enum class TerminalStatus {
    Starting,
    Running,
    Exited,
    Error,
    Closed;

    val live get() = this == Starting || this == Running
}

data class TerminalSession(
    val id: String,
    val title: String,
    val cwd: String,
    val status: TerminalStatus,
    /** Raw PTY history replayed into the native Ghostty terminal. */
    val buffer: String,
    /** Plain lines retained for non-native callers and diagnostics. */
    val lines: List<String>,
    /** The last error the session reported, cleared by the next output. */
    val error: String? = null,
)

data class UsageTotals(
    val costUsd: Double,
    val inputTokens: Long,
    val outputTokens: Long,
    val cacheReadTokens: Long,
    val requests: Long,
)

data class UsageDay(val label: String, val costUsd: Double, val tokens: Long)

/**
 * The span the usage screen asks for, matching the RN client's four options.
 *
 * `Day` is the odd one out and deliberately so: a 24-hour window is requested at
 * hourly resolution, so its chart has 24 rolling buckets rather than one or two
 * calendar-day bars. Everything longer is bucketed by day.
 */
enum class UsageWindow(val label: String, val days: Int) {
    Day("Past 24h", 1),
    Week("7 days", 7),
    Month("30 days", 30),
    Quarter("90 days", 90);

    /** Hourly buckets for the rolling day, daily for the rest. */
    val hourly: Boolean
        get() = this == Day
}

data class UsageProviderBreakdown(
    val provider: ProviderInstance,
    val costUsd: Double,
    val tokens: Long,
    val share: Float,
    val models: List<UsageModelBreakdown>,
)

data class UsageModelBreakdown(val model: String, val costUsd: Double, val tokens: Long)

data class Usage(
    val totals: UsageTotals,
    val days: List<UsageDay>,
    val providers: List<UsageProviderBreakdown>,
    val environments: List<String>,
    /** Which window produced these figures, so the chart can label its axis. */
    val window: UsageWindow = UsageWindow.Month,
)

/* ── New task / add project ──────────────────────────────────────────── */

enum class WorkspaceMode {
    CurrentCheckout,
    NewWorktree,
}

enum class RuntimeMode(val label: String) {
    Default("Default"),
    Plan("Plan"),
}

enum class ApprovalPolicy(val label: String) {
    Ask("Ask every time"),
    AutoEdit("Auto-approve edits"),
    Full("Full access"),
}

/**
 * One choice of a select-shaped provider option, from `ProviderOptionChoice` in
 * `packages/contracts/src/model.ts`.
 */
data class ProviderOptionChoice(
    val id: String,
    val label: String,
    val description: String? = null,
    val isDefault: Boolean = false,
)

/**
 * One knob a model advertises, resolved against the thread's stored selections.
 *
 * Provider options are entirely server-described: Codex offers reasoning effort
 * and a service tier, Claude offers reasoning plus fast mode and a context
 * window, and OpenCode offers nothing. A client with a hardcoded three-value
 * "Reasoning effort" row is wrong for every one of them — it offers efforts a
 * model does not have and hides the ones it does.
 *
 * Ported from `ProviderOptionDescriptor` in `packages/contracts/src/model.ts`.
 */
sealed interface ProviderOptionDescriptor {
    val id: String
    val label: String
    val description: String?

    data class Select(
        override val id: String,
        override val label: String,
        override val description: String? = null,
        val options: List<ProviderOptionChoice> = emptyList(),
        val currentValue: String? = null,
    ) : ProviderOptionDescriptor {
        /**
         * The value in effect: the stored one, else the advertised default. Falling
         * back to the default is what keeps a fresh thread's row from reading as
         * unset when the provider has already decided.
         */
        val effectiveValue: String?
            get() = currentValue ?: options.firstOrNull { it.isDefault }?.id

        val effectiveLabel: String?
            get() = options.firstOrNull { it.id == effectiveValue }?.label
    }

    data class Toggle(
        override val id: String,
        override val label: String,
        override val description: String? = null,
        val currentValue: Boolean = false,
    ) : ProviderOptionDescriptor
}

/** One stored option value, from `ProviderOptionSelection`. */
data class ProviderOptionSelection(val id: String, val value: ProviderOptionValue)

/** A selection's value, which the contract allows to be a string or a boolean. */
sealed interface ProviderOptionValue {
    data class Text(val value: String) : ProviderOptionValue

    data class Flag(val value: Boolean) : ProviderOptionValue
}

data class ThreadSettings(
    val provider: ProviderInstance = ProviderInstance.Default,
    val model: String = "gpt-5-codex",
    val runtimeMode: RuntimeMode = RuntimeMode.Default,
    val approvalPolicy: ApprovalPolicy = ApprovalPolicy.Ask,
    /**
     * Provider option values, in the contract's canonical array shape. Empty is
     * normal: it means the model advertises no knobs, or none has been changed
     * from its default yet.
     */
    val options: List<ProviderOptionSelection> = emptyList(),
)

/**
 * One selectable provider instance and the models it offers.
 *
 * Keyed by instance rather than by driver: a user with two Codex instances (one
 * per API key) has two entries, which is exactly what the server's routing
 * expects. Only instances the server reports as enabled, installed, and
 * authenticated make it into the catalog, so the picker cannot offer an agent a
 * turn start would be refused for.
 */
data class ProviderCatalogEntry(
    val instance: ProviderInstance,
    val models: List<String>,
    /**
     * Option descriptors per model slug, as the server advertises them. Kept
     * beside the slugs rather than replacing them because every caller but the
     * options rows only wants the names, and a map keeps the model list a plain
     * list of strings to search and render.
     */
    val optionDescriptors: Map<String, List<ProviderOptionDescriptor>> = emptyMap(),
)

/** One `/command` a provider advertises, as the composer's popover renders it. */
data class SlashCommand(val name: String, val description: String)

data class RepositoryRef(val fullName: String, val description: String, val private: Boolean)

/* ── Home list layout ────────────────────────────────────────────────── */

sealed interface HomeListItem {
    val key: String

    /**
     * The unsent new-task draft, offered at the top of the list.
     *
     * A draft is durable and survives process death, but before this it was only
     * reachable by walking the new-task flow again — so a prompt the user spent
     * five minutes writing looked lost. Same idea as the web sidebar's draft
     * block (`SidebarDraftBlock` in `apps/web/src/components/Sidebar.tsx`).
     */
    data class Draft(
        val preview: String,
        val projectTitle: String?,
        val environmentLabel: String?,
    ) : HomeListItem {
        override val key: String = "new-task-draft"
    }

    data class Thread(
        val thread: ThreadSummary,
        val project: Project?,
        val environmentLabel: String?,
        /**
         * The exact visible field that satisfied the search, so the row can show
         * and highlight the match instead of only proving that one exists.
         */
        val searchMatch: SearchMatch? = null,
        /**
         * First line of the thread's unsent draft, when it has one.
         *
         * On the row rather than looked up by the row, so the list stays a pure
         * function of its inputs and a keystroke in one thread cannot re-render the
         * rest of the list.
         */
        val draftPreview: String? = null,
    ) : HomeListItem {
        override val key: String = "thread:${thread.environmentId.value}:${thread.id.value}"
    }

    data class Queued(val thread: ThreadSummary, val project: Project?) : HomeListItem {
        override val key: String =
            "queued:${thread.environmentId.value}:${thread.id.value}"
    }

    data class PendingApprovalCard(
        val thread: ThreadSummary,
        val project: Project?,
        val environmentLabel: String?,
        val approval: PendingApproval,
    ) : HomeListItem {
        override val key: String =
            "approval:${thread.environmentId.value}:${thread.id.value}:${approval.id}"
    }

    data class PendingInputCard(
        val thread: ThreadSummary,
        val project: Project?,
        val environmentLabel: String?,
        val request: PendingUserInput,
    ) : HomeListItem {
        override val key: String =
            "input:${thread.environmentId.value}:${thread.id.value}:${request.id}"
    }

    data class ShelfHeader(
        val label: String,
        val count: Int,
        val expanded: Boolean,
        val kind: ShelfKind,
    ) : HomeListItem {
        override val key: String = "shelf:${kind.name}"
    }

    data class Section(val label: String) : HomeListItem {
        override val key: String = "section:$label"
    }
}

/** The row field selected to explain a Home search result. */
data class SearchMatch(val source: SearchMatchSource, val text: String)

/** One environment-scoped message-body hit returned by `orchestration.searchThreads`. */
data class ThreadSearchMatch(
    val environmentId: EnvironmentId,
    val threadId: ThreadId,
    val projectId: ProjectId,
    val source: SearchMatchSource,
    val snippet: String,
    val messageCreatedAt: String?,
)

enum class SearchMatchSource(val label: String) {
    Title("Title"),
    Excerpt("Activity"),
    UserMessage("You"),
    AssistantMessage("Agent"),
    Branch("Branch"),
    Project("Project"),
    Repository("Repository"),
    Environment("Environment"),
    Provider("Provider"),
    Model("Model"),
}

enum class ShelfKind {
    Snoozed,
    Settled,
}

enum class ThreadFilter(val label: String) {
    All("All"),
    Active("Active"),
    Pending("Pending"),
    Snoozed("Snoozed"),
    Settled("Settled"),
}

enum class ProjectGrouping(val label: String) {
    ByProject("By project"),
    ByRepository("By repository"),
    Flat("Flat list"),
}

enum class ThreadSort(val label: String) {
    Recent("Most recent"),
    Created("Newest first"),
    Alphabetical("A–Z"),
}
