package club.touchtech.s5code.kotlin.feature.pullrequests

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.CallMerge
import androidx.compose.material.icons.automirrored.rounded.Undo
import androidx.compose.material.icons.rounded.AutoAwesome
import androidx.compose.material.icons.rounded.Block
import androidx.compose.material.icons.rounded.Difference
import androidx.compose.material.icons.rounded.EditNote
import androidx.compose.material.icons.rounded.ExpandMore
import androidx.compose.material.icons.rounded.Lock
import androidx.compose.material.icons.rounded.LockOpen
import androidx.compose.material.icons.rounded.RateReview
import androidx.compose.material.icons.rounded.Schedule
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.PullRequestAgentTask
import club.touchtech.s5code.kotlin.data.PullRequestPermissions
import club.touchtech.s5code.kotlin.data.PullRequestReference
import club.touchtech.s5code.kotlin.data.allowedMergeMethods
import club.touchtech.s5code.kotlin.data.mergeMethodTitle
import club.touchtech.s5code.kotlin.data.pullRequestActionLabel
import club.touchtech.s5code.kotlin.data.pullRequestMergeConfirmation
import club.touchtech.s5code.kotlin.data.pullRequestMergeState
import club.touchtech.s5code.kotlin.data.pullRequestPrimaryActionLabel
import club.touchtech.s5code.kotlin.data.pullRequestStatus
import club.touchtech.s5code.kotlin.data.resolveMergeMethod
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogRequest
import club.touchtech.s5code.kotlin.design.component.S5FloatingToolbar
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.component.S5MenuOption
import club.touchtech.s5code.kotlin.design.component.S5OverflowMenu
import club.touchtech.s5code.kotlin.design.component.S5SplitButton
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDetailDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestStackDto
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

/** The status menu's writes, in RN's order. */
private val STATUS_ACTIONS = listOf("ready", "draft", "close", "reopen", "revert")

private fun statusActionIcon(action: String): ImageVector =
    when (action) {
        "ready" -> Icons.Rounded.RateReview
        "draft" -> Icons.Rounded.EditNote
        "close" -> Icons.Rounded.Block
        "reopen" -> Icons.Rounded.LockOpen
        "revert" -> Icons.AutoMirrored.Rounded.Undo
        else -> Icons.Rounded.Lock
    }

/** `PullRequestStatusMenu`: the state label, opening ready/draft/close/reopen/revert when allowed. */
@Composable
fun PullRequestStatusMenu(pr: PullRequestDetailDto, runner: PullRequestActionRunner) {
    val status = pullRequestStatus(pr.state, pr.isDraft)
    val actions = STATUS_ACTIONS.filter(runner::available)
    var expanded by remember { mutableStateOf(false) }
    val trigger: @Composable () -> Unit = {
        Row(
            Modifier.then(if (actions.isNotEmpty()) Modifier.clickable { expanded = true } else Modifier)
                .padding(vertical = S5Theme.spacing.tiny),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Icon(status.icon, contentDescription = null, tint = status.tone.color(), modifier = Modifier.size(18.dp))
            Text(status.label, style = MaterialTheme.typography.labelLarge, color = status.tone.color())
            if (runner.busy) {
                S5InlineLoading(Modifier.size(16.dp))
            } else if (actions.isNotEmpty()) {
                Icon(Icons.Rounded.ExpandMore, contentDescription = "Pull request status", modifier = Modifier.size(16.dp))
            }
        }
    }
    if (actions.isEmpty()) return trigger()
    S5OverflowMenu(
        icon = Icons.Rounded.ExpandMore,
        label = "Pull request status",
        options =
            actions.map {
                S5MenuOption(it, pullRequestActionLabel(it, pr.provider), statusActionIcon(it), destructive = it == "close")
            },
        onSelect = runner::request,
        expanded = expanded,
        onExpandedChange = { expanded = it },
        anchor = trigger,
    )
}

/** What merge controls need from the PR's stack read (`usePullRequestStack`). */
@Immutable
data class PullRequestStackState(
    val stack: PullRequestStackDto?,
    val fresh: Boolean,
    val failed: Boolean,
    val supportsStackActions: Boolean,
)

/**
 * `PullRequestMergeControls`: merge with the remembered method — or the
 * stack through this layer — plus auto-merge from the method menu. The
 * gateway re-checks a single merge against a fresh read before sending it.
 */
@Composable
fun PullRequestMergeControls(
    store: AppStore,
    environmentId: String,
    reference: PullRequestReference,
    pr: PullRequestDetailDto,
    permissions: PullRequestPermissions,
    stack: PullRequestStackState,
    confirmController: S5ConfirmDialogController,
    onRefreshStack: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    val preferences by store.preferences.collectAsStateWithLifecycle()
    var selected by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val methods = pr.allowedMergeMethods()
    val method = resolveMergeMethod(methods, selected, preferences.pullRequestMergeMethod)
    val state =
        pullRequestMergeState(
            pr = pr,
            permissions = permissions,
            method = method,
            stack = stack.stack,
            stackFresh = stack.fresh,
            stackFailed = stack.failed,
            supportsStackActions = stack.supportsStackActions,
            busy = busy,
        )

    fun perform(action: String) {
        if (busy || (action != "disable-auto-merge" && method == null)) return
        if (if (action == "merge") !state.canMerge else !state.canAutoMerge) return
        busy = true
        scope.launch {
            try {
                val stackMerge = action == "merge" && stack.stack != null && state.scope != null
                store.pullRequests.runAction(
                    environmentId,
                    reference,
                    action,
                    mergeMethod = if (action == "disable-auto-merge") null else method,
                    stackNumber = if (stackMerge) stack.stack?.number else null,
                    expectedStackHeads = if (stackMerge) state.scope?.heads else null,
                )
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (error: Exception) {
                confirmController.alert("Couldn't complete the merge action", error.alertMessage())
            } finally {
                busy = false
            }
        }
    }

    fun confirm(action: String) {
        val (title, message) = pullRequestMergeConfirmation(action, pr, method, state.scope, stack.stack?.base)
        confirmController.show(
            S5ConfirmDialogRequest(
                title = title,
                message = message,
                confirmText = if (action == "merge") state.label else title.removeSuffix("?"),
                onConfirm = { perform(action) },
            )
        )
    }

    var menuOpen by remember { mutableStateOf(false) }
    Column(
        Modifier.fillMaxWidth().padding(horizontal = S5Theme.spacing.large, vertical = S5Theme.spacing.medium),
        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
            val text =
                when {
                    busy -> "Working…"
                    !permissions.connected -> "Reconnect to merge"
                    else -> state.label
                }
            val options =
                methods.map {
                    S5MenuOption(it, mergeMethodTitle(it), selected = it == method, enabled = state.canMerge || state.canAutoMerge)
                } +
                    if (state.canAutoMerge) {
                        listOf(
                            S5MenuOption(
                                state.autoAction,
                                if (pr.autoMergeEnabled == true) "Disable auto-merge" else "Enable auto-merge",
                                Icons.Rounded.Schedule,
                            )
                        )
                    } else {
                        emptyList()
                    }
            if (options.isEmpty()) {
                S5Button(
                    text = text,
                    onClick = { confirm("merge") },
                    enabled = state.canMerge,
                    emphasis = S5ActionEmphasis.Secondary,
                    modifier = Modifier.weight(1f),
                )
            } else {
                S5OverflowMenu(
                    icon = Icons.Rounded.ExpandMore,
                    label = "Merge options",
                    options = options,
                    onSelect = { id ->
                        if (id in methods) {
                            selected = id
                            store.updatePreferences { it.copy(pullRequestMergeMethod = id) }
                        } else if (id == state.autoAction && state.canAutoMerge) {
                            confirm(state.autoAction)
                        }
                    },
                    expanded = menuOpen,
                    onExpandedChange = { menuOpen = it },
                    modifier = Modifier.weight(1f, fill = false),
                    anchor = {
                        S5SplitButton(
                            text = text,
                            onClick = { confirm("merge") },
                            icon = Icons.AutoMirrored.Rounded.CallMerge,
                            trailingIcon = Icons.Rounded.ExpandMore,
                            trailingLabel = "Merge options",
                            onTrailingClick = { menuOpen = !menuOpen },
                            enabled = state.canMerge,
                            trailingEnabled = options.any { it.enabled },
                        )
                    },
                )
            }
            if (busy || state.checkingStack) S5InlineLoading()
        }
        state.visibleReason?.let {
            Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        if (pr.autoMergeEnabled == true) {
            Text(
                "Auto-merge is enabled" + (pr.autoMergeMethod?.let { " · $it" } ?: ""),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (stack.failed && stack.supportsStackActions) {
            S5Button(text = "Retry stack lookup", onClick = onRefreshStack, style = S5ButtonStyle.Text)
        }
    }
}

/**
 * `PullRequestActionToolbar`: AI helpers, the primary action (submit the
 * pending review, or work on the PR in a thread), and Changed files.
 */
@Composable
fun PullRequestActionBar(
    agent: PullRequestAgent,
    permissions: PullRequestPermissions,
    pendingComments: Int,
    onOpenReview: () -> Unit,
    onOpenFiles: () -> Unit,
) {
    val pendingReview = permissions.verdicts.isNotEmpty() && pendingComments > 0
    val showAgentControls = agent.canAsk || !permissions.connected
    val showPrimary = pendingReview || agent.threads.isNotEmpty() || showAgentControls
    val chooseThread = !pendingReview && agent.threads.size > 1
    val label = pullRequestPrimaryActionLabel(pendingReview, pendingComments, agent.threads.size)
    var helpersOpen by remember { mutableStateOf(false) }
    var threadsOpen by remember { mutableStateOf(false) }
    Box(Modifier.fillMaxWidth().navigationBarsPadding().padding(bottom = S5Theme.spacing.medium), contentAlignment = Alignment.Center) {
        S5FloatingToolbar {
            if (showAgentControls) {
                S5OverflowMenu(
                    icon = Icons.Rounded.AutoAwesome,
                    label = "PR agent actions",
                    options =
                        agent.actions
                            .filter { it.task != PullRequestAgentTask.Ask }
                            .map { agent.menuOption(it.task) },
                    onSelect = agent::select,
                    expanded = helpersOpen,
                    onExpandedChange = { if (!agent.disabled) helpersOpen = it },
                )
            }
            if (showPrimary) {
                val primary: @Composable () -> Unit = {
                    S5Button(
                        text = label,
                        onClick = {
                            when {
                                pendingReview -> onOpenReview()
                                chooseThread -> threadsOpen = true
                                else -> agent.start(PullRequestAgentTask.Ask)
                            }
                        },
                        enabled = pendingReview || !agent.disabled,
                        emphasis = S5ActionEmphasis.Secondary,
                    )
                }
                if (chooseThread) {
                    S5OverflowMenu(
                        icon = Icons.Rounded.ExpandMore,
                        label = "Open thread",
                        options = agent.threadOptions(PullRequestAgentTask.Ask),
                        onSelect = agent::select,
                        expanded = threadsOpen,
                        onExpandedChange = { threadsOpen = it },
                        anchor = primary,
                    )
                } else {
                    primary()
                }
            }
            S5IconButton(icon = Icons.Rounded.Difference, label = "Changed files", onClick = onOpenFiles)
        }
    }
}
