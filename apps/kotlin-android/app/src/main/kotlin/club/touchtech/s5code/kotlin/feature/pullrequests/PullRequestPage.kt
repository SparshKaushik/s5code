package club.touchtech.s5code.kotlin.feature.pullrequests

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Chat
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.AutoAwesome
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.app.PullRequestWorkDestination
import club.touchtech.s5code.kotlin.app.newTaskDraftWouldBeReplaced
import club.touchtech.s5code.kotlin.app.preparePullRequestWork
import club.touchtech.s5code.kotlin.data.MaintainerEditsChoice
import club.touchtech.s5code.kotlin.data.PullRequestAgentAction
import club.touchtech.s5code.kotlin.data.PullRequestAgentTask
import club.touchtech.s5code.kotlin.data.PullRequestPermissions
import club.touchtech.s5code.kotlin.data.PullRequestQuery
import club.touchtech.s5code.kotlin.data.PullRequestReference
import club.touchtech.s5code.kotlin.data.canPerformPullRequestAction
import club.touchtech.s5code.kotlin.data.maintainerEditsChoiceInForce
import club.touchtech.s5code.kotlin.data.pullRequestActionConfirmation
import club.touchtech.s5code.kotlin.data.pullRequestActionLabel
import club.touchtech.s5code.kotlin.data.pullRequestAgentActions
import club.touchtech.s5code.kotlin.data.pullRequestAgentThread
import club.touchtech.s5code.kotlin.data.pullRequestPermissions
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogRequest
import club.touchtech.s5code.kotlin.design.component.S5MenuOption
import club.touchtech.s5code.kotlin.model.EnvironmentCapabilities
import club.touchtech.s5code.kotlin.transport.parseChangeRequestUrl
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDetailDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestLinkedThreadDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestLinkedThreadsResultDto
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.launch

/* Shared state for the PR page and the screens it opens. */

/** An alert with only an OK button, as RN's `Dialog.alert(title, message)`. */
fun S5ConfirmDialogController.alert(title: String, message: String) =
    show(S5ConfirmDialogRequest(title = title, message = message, confirmText = "OK", cancelText = null, onConfirm = {}))

/** The error text an alert shows for a failed write. */
internal fun Throwable.alertMessage(): String = message?.takeIf(String::isNotBlank) ?: "Try again."

@Composable
fun rememberEnvironmentCapabilities(store: AppStore, environmentId: String): EnvironmentCapabilities {
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    return environments.firstOrNull { it.id.value == environmentId }?.capabilities ?: EnvironmentCapabilities()
}

@Composable
fun rememberPullRequestConnected(store: AppStore, environmentId: String): Boolean {
    val connected by remember(environmentId) { store.pullRequests.connected(environmentId) }.collectAsStateWithLifecycle(false)
    return connected
}

/** `usePullRequestPermissions`: what this session may write to [pr]. */
@Composable
fun rememberPullRequestPermissions(store: AppStore, environmentId: String, pr: PullRequestDetailDto?): PullRequestPermissions {
    val connected = rememberPullRequestConnected(store, environmentId)
    val capabilities = rememberEnvironmentCapabilities(store, environmentId)
    return remember(pr, connected, capabilities.sessionAllowsSourceControlWrite) {
        pullRequestPermissions(pr, connected, capabilities.sessionAllowsSourceControlWrite)
    }
}

@Composable
fun rememberPullRequestDetail(store: AppStore, environmentId: String, reference: PullRequestReference): PullRequestQuery<PullRequestDetailDto> {
    val query by remember(environmentId, reference) { store.pullRequests.detail(environmentId, reference) }
        .collectAsStateWithLifecycle(PullRequestQuery())
    return query
}

/** `usePullRequestMaintainerEdits`: the "Allow edits from maintainers" toggle, drawn optimistically. */
@Stable
class PullRequestMaintainerEdits internal constructor(
    val visible: Boolean,
    val value: Boolean,
    val saving: Boolean,
    val toggle: () -> Unit,
) {
    val subtitle: String
        get() =
            if (saving) {
                "Saving…"
            } else {
                "Maintainers can push to this branch. If your fork has workflows, this can also give access to its secrets."
            }
}

@Composable
fun rememberPullRequestMaintainerEdits(
    store: AppStore,
    environmentId: String,
    reference: PullRequestReference,
    pr: PullRequestDetailDto?,
    canEdit: Boolean,
    confirmController: S5ConfirmDialogController,
): PullRequestMaintainerEdits {
    val scope = rememberCoroutineScope()
    var saving by remember { mutableStateOf(false) }
    var choice by remember { mutableStateOf<MaintainerEditsChoice?>(null) }
    val kept = maintainerEditsChoiceInForce(choice, pr?.url.orEmpty(), pr?.observedAt, saving)
    SideEffect { if (kept != choice) choice = kept }
    val value = kept?.value ?: (pr?.maintainerCanModify == true)
    return PullRequestMaintainerEdits(
        visible = canEdit,
        value = value,
        saving = saving,
        toggle = toggle@{
            if (saving || pr == null || !canEdit) return@toggle
            val next = !value
            val previous = choice
            saving = true
            choice = MaintainerEditsChoice(pr.url, pr.observedAt, next)
            scope.launch {
                try {
                    store.pullRequests.update(environmentId, reference, maintainerCanModify = next)
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (error: Exception) {
                    choice = previous
                    confirmController.alert("Could not save maintainer access", error.alertMessage())
                } finally {
                    saving = false
                }
            }
        },
    )
}

/**
 * `usePullRequestAction`: runs one status write at a time, refreshing the
 * page after; closing and branch deletion ask first.
 */
@Stable
class PullRequestActionRunner internal constructor(
    private val launch: (suspend () -> Unit) -> Unit,
    private val run: suspend (action: String, updateMethod: String?) -> Unit,
    private val confirmController: S5ConfirmDialogController,
) {
    var busy by mutableStateOf(false)
        private set
    internal var pr: PullRequestDetailDto? = null
    internal var permissions: PullRequestPermissions = PullRequestPermissions()

    fun available(action: String): Boolean {
        val current = pr ?: return false
        return !busy && permissions.canWrite && canPerformPullRequestAction(current, action)
    }

    fun perform(action: String, updateMethod: String? = null) {
        if (!available(action)) return
        busy = true
        launch {
            try {
                run(action, updateMethod)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (error: Exception) {
                confirmController.alert("Couldn't update this pull request", error.alertMessage())
            } finally {
                busy = false
            }
        }
    }

    /** Asks first when the action has confirmation copy, else runs at once. */
    fun request(action: String) {
        val current = pr ?: return
        val (title, message) = pullRequestActionConfirmation(action, current) ?: return perform(action)
        confirmController.show(
            S5ConfirmDialogRequest(
                title = title,
                message = message,
                confirmText = title.ifEmpty { pullRequestActionLabel(action, current.provider) },
                destructive = action == "close" || action == "delete-branch",
                onConfirm = { perform(action) },
            )
        )
    }
}

@Composable
fun rememberPullRequestActionRunner(
    store: AppStore,
    environmentId: String,
    reference: PullRequestReference,
    pr: PullRequestDetailDto?,
    permissions: PullRequestPermissions,
    confirmController: S5ConfirmDialogController,
): PullRequestActionRunner {
    val scope = rememberCoroutineScope()
    val runner =
        remember(environmentId, reference) {
            PullRequestActionRunner(
                launch = { block -> scope.launch { block() } },
                run = { action, method ->
                    store.pullRequests.runAction(environmentId, reference, action, updateMethod = method)
                },
                confirmController = confirmController,
            )
        }
    runner.pr = pr
    runner.permissions = permissions
    return runner
}

/** The reference linked-thread reads use: the URL's host fills one the route left out. */
fun PullRequestReference.withHostOf(pr: PullRequestDetailDto?): PullRequestReference =
    if (host != null || pr == null) this else copy(host = parseChangeRequestUrl(pr.url)?.host)

/**
 * `usePullRequestAgentActions` + `usePullRequestThreads`: the threads linked
 * to a PR and starting agent work from it — in a linked thread's composer,
 * or a new-task draft checked out on the PR's head.
 */
@Stable
class PullRequestAgent internal constructor(
    val threads: List<PullRequestLinkedThreadDto>,
    val canAsk: Boolean,
    val canWatch: Boolean,
    val disabled: Boolean,
    val actions: List<PullRequestAgentAction>,
    private val onStart: (PullRequestAgentTask, String?) -> Unit,
) {
    fun action(task: PullRequestAgentTask): PullRequestAgentAction? = actions.firstOrNull { it.task == task }

    /** Whether [task]'s menu entry is unavailable. */
    fun isDisabled(task: PullRequestAgentTask): Boolean = disabled || !canAsk || action(task)?.disabled == true

    /** [threadId] is a linked thread's id, "new" for a fresh draft, or null for the only thread. */
    fun start(task: PullRequestAgentTask, threadId: String? = null) = onStart(task, threadId)

    /** `threadActions`: each linked thread, then a fresh draft. */
    fun threadOptions(task: PullRequestAgentTask): List<S5MenuOption> =
        threads.map { thread ->
            S5MenuOption(
                id = "${task.id}:${thread.id}",
                label = thread.title.ifBlank { "Untitled thread" },
                icon = Icons.AutoMirrored.Rounded.Chat,
                supporting = if (thread.archivedAt != null) "Archived thread" else "Continue thread",
                enabled = task == PullRequestAgentTask.Ask || thread.archivedAt == null,
            )
        } + if (canAsk) listOf(S5MenuOption("${task.id}:new", "Start a new thread", Icons.Rounded.Add)) else emptyList()

    /** `menuAction`: one task, opening a thread picker when several threads are linked. */
    fun menuOption(task: PullRequestAgentTask, title: String = action(task)?.task?.title ?: "Open thread"): S5MenuOption =
        S5MenuOption(
            id = task.id,
            label = title,
            icon = Icons.Rounded.AutoAwesome,
            enabled = !isDisabled(task),
            children = if (threads.size > 1) threadOptions(task) else emptyList(),
        )

    /** Runs a [menuOption] or [threadOptions] pick. */
    fun select(id: String) {
        val task = actions.firstOrNull { id == it.task.id || id.startsWith("${it.task.id}:") }?.task ?: return
        start(task, if (id == task.id) null else id.removePrefix("${task.id}:"))
    }
}

@Composable
fun rememberPullRequestAgent(
    store: AppStore,
    environmentId: String,
    reference: PullRequestReference,
    pr: PullRequestDetailDto?,
    confirmController: S5ConfirmDialogController,
    onOpenThread: (environmentId: String, threadId: String) -> Unit,
    onOpenNewTask: () -> Unit,
    filePath: String? = null,
): PullRequestAgent {
    val capabilities = rememberEnvironmentCapabilities(store, environmentId)
    val connected = rememberPullRequestConnected(store, environmentId)
    val supported = capabilities.threadPullRequests
    val linkedReference = reference.withHostOf(pr)
    val relations by remember(environmentId, linkedReference, supported, pr != null) {
            if (pr != null && supported) store.pullRequests.linkedThreads(environmentId, linkedReference)
            else flowOf(PullRequestQuery<PullRequestLinkedThreadsResultDto>())
        }
        .collectAsStateWithLifecycle(PullRequestQuery())
    val threads = relations.value?.threads.orEmpty()
    val canAsk = !connected || capabilities.sessionAllowsOperate
    val canCheckout = capabilities.sessionAllowsSourceControlWrite
    val canWatch = capabilities.threadPullRequestWatch
    var busy by remember { mutableStateOf(false) }
    val disabled = busy || pr == null || (connected && supported && !relations.hasValue && threads.isEmpty())
    val actions = remember(pr, canWatch) { pr?.let { pullRequestAgentActions(it, canWatch) }.orEmpty() }
    val latestOpenThread by rememberUpdatedState(onOpenThread)
    val latestOpenNewTask by rememberUpdatedState(onOpenNewTask)

    fun ask(current: PullRequestDetailDto, thread: PullRequestLinkedThreadDto?, task: PullRequestAgentTask) {
        if (busy) return
        fun proceed() {
            busy = true
            try {
                when (
                    val destination =
                        store.preparePullRequestWork(environmentId, current, thread, task, filePath, connected, canCheckout)
                ) {
                    is PullRequestWorkDestination.Thread -> latestOpenThread(destination.environmentId, destination.threadId)
                    PullRequestWorkDestination.NewTask -> latestOpenNewTask()
                }
            } catch (error: Exception) {
                confirmController.alert("Couldn't open a thread", error.alertMessage())
            } finally {
                busy = false
            }
        }
        if (thread == null && store.newTaskDraftWouldBeReplaced(environmentId, current)) {
            confirmController.show(
                S5ConfirmDialogRequest(
                    title = "Replace your new task draft?",
                    message = "Work on #${current.number} starts a new draft on ${current.headBranch}.",
                    confirmText = "Replace",
                    cancelText = "Keep mine",
                    onConfirm = ::proceed,
                )
            )
        } else {
            proceed()
        }
    }

    return PullRequestAgent(threads, canAsk, canWatch, disabled, actions) { task, threadId ->
        val current = pr ?: return@PullRequestAgent
        if (disabled) return@PullRequestAgent
        val thread = pullRequestAgentThread(threads, threadId)
        if (threadId != null && threadId != "new" && thread == null) return@PullRequestAgent
        if (task == PullRequestAgentTask.Ask && thread != null && (!canAsk || thread.archivedAt != null)) {
            latestOpenThread(environmentId, thread.id)
            return@PullRequestAgent
        }
        if (!canAsk || actions.firstOrNull { it.task == task }?.disabled == true) return@PullRequestAgent
        ask(current, thread?.takeIf { it.archivedAt == null }, task)
    }
}
