package club.touchtech.s5code.kotlin.feature.git

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.AddCircleOutline
import androidx.compose.material.icons.automirrored.rounded.CallMerge
import androidx.compose.material.icons.automirrored.rounded.CallSplit
import androidx.compose.material.icons.rounded.CloudDownload
import androidx.compose.material.icons.rounded.CloudUpload
import androidx.compose.material.icons.rounded.Commit
import androidx.compose.material.icons.rounded.Difference
import androidx.compose.material.icons.automirrored.rounded.MergeType
import androidx.compose.material.icons.automirrored.rounded.OpenInNew
import androidx.compose.material.icons.rounded.Source
import androidx.compose.material.icons.rounded.Sync
import androidx.compose.material.icons.rounded.Warning
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.Remote
import club.touchtech.s5code.kotlin.data.rememberRetryableRemote
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5ErrorState
import club.touchtech.s5code.kotlin.design.component.S5LoadingState
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5RowGroup
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SectionHeader
import club.touchtech.s5code.kotlin.design.component.S5SelectableRow
import club.touchtech.s5code.kotlin.design.component.S5SettingsRow
import club.touchtech.s5code.kotlin.design.component.S5SplitButton
import club.touchtech.s5code.kotlin.design.component.S5StatusPill
import club.touchtech.s5code.kotlin.design.component.S5TextField
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.rowPosition
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.GitStatus
import club.touchtech.s5code.kotlin.model.PullRequestState
import club.touchtech.s5code.kotlin.model.ThreadId
import kotlinx.coroutines.launch

/** Git overview: branch, ahead/behind, changed files, PR summary, actions. */
@Composable
fun GitOverviewScreen(
    store: AppStore,
    environmentId: String,
    threadId: String,
    onBack: () -> Unit,
    onOpen: (String) -> Unit,
) {
    val (state, retry) = rememberGitStatus(store, environmentId, threadId)
    val remote = state.value
    val status = remote.valueOrNull
    LaunchedEffect(environmentId, threadId, status?.unstaged, status?.staged, status?.untracked) {
        if (status != null && (status.unstaged.isNotEmpty() || status.staged.isNotEmpty() || status.untracked.isNotEmpty())) {
            runCatching {
                store.workspace.review(EnvironmentId(environmentId), ThreadId(threadId))
            }
        }
    }

    S5Screen(
        title = "Git",
        subtitle =
            status?.let { "${it.branch} · ${it.ahead} ahead, ${it.behind} behind" }.orEmpty(),
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
        floatingActionButton = {
            S5SplitButton(
                text = "Commit",
                onClick = { onOpen("git/commit") },
                icon = Icons.Rounded.Commit,
                trailingIcon = Icons.Rounded.Sync,
                trailingLabel = "Sync options",
                onTrailingClick = { onOpen("git-confirm") },
            )
        },
    ) { padding ->
        when (remote) {
            is Remote.Loading -> S5LoadingState("Reading git status…", Modifier.padding(padding))
            is Remote.Failed ->
                Box(Modifier.padding(padding).padding(S5Theme.spacing.gutter)) {
                    S5ErrorState(title = "Couldn't read git status", detail = remote.message, onRetry = retry)
                }
            is Remote.Loaded -> {
                val loaded = remote.value
                val changed = loaded.staged.size + loaded.unstaged.size + loaded.untracked.size
                Column(
                    Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                        S5Card(tone = S5CardTone.Hero, modifier = Modifier.fillMaxWidth()) {
                            Column(
                                Modifier.padding(S5Theme.spacing.large),
                                verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                            ) {
                                Row(
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                                ) {
                                    Icon(
                                        Icons.AutoMirrored.Rounded.CallSplit,
                                        contentDescription = null,
                                        modifier = Modifier.size(18.dp),
                                    )
                                    Text(loaded.branch, style = MaterialTheme.typography.titleMediumEmphasized)
                                }
                                Text(
                                    "based on ${loaded.baseBranch}",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                                Row(horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                                    S5StatusPill(
                                        label = "${loaded.ahead} ahead",
                                        containerColor = S5Theme.status.settledContainer,
                                        contentColor = S5Theme.status.onSettledContainer,
                                    )
                                    S5StatusPill(
                                        label = "${loaded.behind} behind",
                                        containerColor = S5Theme.status.approvalContainer,
                                        contentColor = S5Theme.status.onApprovalContainer,
                                    )
                                    S5StatusPill(
                                        label = "$changed changed",
                                        containerColor = MaterialTheme.colorScheme.secondaryContainer,
                                        contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                                    )
                                }
                                if (loaded.worktreePath != null) {
                                    Text(
                                        loaded.worktreePath,
                                        style = S5Theme.code.inlineTechnical,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                }
                            }
                        }
                    }

                    if (loaded.behind > 0) {
                        Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                            S5Notice(
                                icon = Icons.Rounded.Warning,
                                text = "${loaded.baseBranch} moved ahead. Pull before pushing to avoid a conflict.",
                            )
                        }
                    }

                    FileSection("Staged", loaded.staged, Icons.Rounded.AddCircleOutline)
                    FileSection("Changed", loaded.unstaged, Icons.Rounded.Difference)
                    FileSection("Untracked", loaded.untracked, Icons.Rounded.Source)

                    S5RowGroup(title = "Actions") {
                        val actions =
                            listOf(
                                Triple("Review changes", Icons.Rounded.Difference, "review"),
                                Triple("Branches", Icons.AutoMirrored.Rounded.CallSplit, "git/branches"),
                                Triple("Source control", Icons.Rounded.Source, "source-control"),
                                Triple("Pull requests", Icons.AutoMirrored.Rounded.CallMerge, "pull-requests"),
                            )
                        actions.forEachIndexed { index, (label, icon, route) ->
                            S5SettingsRow(
                                icon = icon,
                                label = label,
                                onClick = { onOpen(route) },
                                position = rowPosition(index, actions.size),
                            )
                        }
                    }
                    Box(Modifier.padding(bottom = 96.dp))
                }
            }
        }
    }
}

/**
 * One git status read per (environment, thread), retryable.
 *
 * Every git screen needs the same status and each is reachable directly by deep
 * link, so the read lives here rather than being hoisted into a shared parent
 * that would only exist to hold it.
 */
@Composable
private fun rememberGitStatus(
    store: AppStore,
    environmentId: String,
    threadId: String,
): Pair<androidx.compose.runtime.State<Remote<GitStatus>>, () -> Unit> {
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val id = remember(threadId) { ThreadId(threadId) }
    return rememberRetryableRemote(environmentId, threadId) { store.workspace.gitStatus(env, id) }
}

@Composable
private fun FileSection(label: String, files: List<String>, icon: ImageVector) {
    if (files.isEmpty()) return
    S5SectionHeader("$label (${files.size})")
    Column(
        Modifier.padding(horizontal = S5Theme.spacing.gutter),
        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
    ) {
        files.forEachIndexed { index, path ->
            S5SelectableRow(
                label = path.substringAfterLast('/'),
                supporting = path,
                selected = false,
                onClick = {},
                leading = { Icon(icon, contentDescription = null) },
                position = rowPosition(index, files.size),
            )
        }
    }
}

/** Commit message, validation, and progress. */
@Composable
fun GitCommitScreen(store: AppStore, environmentId: String, threadId: String, onBack: () -> Unit) {
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val id = remember(threadId) { ThreadId(threadId) }
    val (state, _) = rememberGitStatus(store, environmentId, threadId)
    val status = state.value.valueOrNull
    val scope = rememberCoroutineScope()
    var subject by remember { mutableStateOf("") }
    var body by remember { mutableStateOf("") }
    var committing by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }
    val tooLong = subject.length > 72
    val canCommit = subject.isNotBlank() && !tooLong && !committing

    /**
     * Commits, optionally pushing after. Both run on the machine, so a failure
     * has to be shown here: the screen cannot close on an operation that did not
     * happen.
     */
    fun commit(push: Boolean) {
        if (!canCommit) return
        committing = true
        failure = null
        scope.launch {
            val message = if (body.isBlank()) subject else "$subject\n\n$body"
            val outcome =
                runCatching {
                    val actionId =
                        store.beginAction(
                            if (push) "Committing and pushing" else "Creating commit",
                            subject,
                        )
                    try {
                        val commitResult =
                            store.workspace.commit(env, id, message) { progress ->
                                store.updateAction(actionId, progress.label, progress.description)
                            }
                        val result =
                            if (push) {
                                store.workspace.push(env, id) { progress ->
                                    store.updateAction(actionId, progress.label, progress.description)
                                }
                            } else {
                                commitResult
                            }
                        store.finishAction(
                            actionId,
                            result.title,
                            result.description,
                            result.pullRequestUrl,
                        )
                    } catch (cause: Throwable) {
                        store.failAction(
                            actionId,
                            if (push) "Commit and push failed" else "Commit failed",
                            cause.message ?: "The source control action was refused.",
                        )
                        throw cause
                    }
                }
            committing = false
            outcome.fold(
                onSuccess = { onBack() },
                onFailure = { cause -> failure = cause.message ?: "The commit was refused." },
            )
        }
    }

    S5Screen(
        title = "Commit",
        subtitle = status?.let { "${it.staged.size + it.unstaged.size} files" }.orEmpty(),
        onBack = onBack,
        loading = committing,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).padding(horizontal = S5Theme.spacing.gutter),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            S5TextField(
                value = subject,
                onValueChange = { subject = it },
                label = "Summary",
                placeholder = "fix(android): keep the composer above the IME",
                isError = tooLong,
                supporting =
                    if (tooLong) "Keep the summary under 72 characters (${subject.length})."
                    else "Conventional commit style, plain language.",
                singleLine = true,
            )
            S5TextField(
                value = body,
                onValueChange = { body = it },
                label = "Description",
                placeholder = "What changed, and why.",
                minHeight = 96.dp,
            )
            failure?.let { message ->
                S5ErrorState(
                    title = "Commit failed",
                    detail = message,
                    onRetry = { failure = null },
                    retryLabel = "Dismiss",
                )
            }
            Row(horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                S5Button(
                    text = if (committing) "Committing…" else "Commit",
                    onClick = { commit(push = false) },
                    emphasis = S5ActionEmphasis.Primary,
                    icon = Icons.Rounded.Commit,
                    enabled = canCommit,
                )
                S5Button(
                    text = "Commit and push",
                    onClick = { commit(push = true) },
                    emphasis = S5ActionEmphasis.Primary,
                    style = S5ButtonStyle.Outlined,
                    icon = Icons.Rounded.CloudUpload,
                    enabled = canCommit,
                )
            }
        }
    }
}

/** Branch list with create/switch. */
@Composable
fun GitBranchesScreen(store: AppStore, environmentId: String, threadId: String, onBack: () -> Unit) {
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val id = remember(threadId) { ThreadId(threadId) }
    val (statusState, _) = rememberGitStatus(store, environmentId, threadId)
    // Refreshed after a create or a switch, because both change what this list
    // says is current.
    val (branchState, reload) =
        rememberRetryableRemote(environmentId, threadId) { store.workspace.branches(env, id) }
    val scope = rememberCoroutineScope()
    var newBranch by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }
    val remote = branchState.value
    val branches = remote.valueOrNull.orEmpty()

    fun run(action: suspend () -> Unit) {
        if (busy) return
        busy = true
        failure = null
        scope.launch {
            val outcome = runCatching { action() }
            busy = false
            outcome.fold(
                onSuccess = { reload() },
                onFailure = { cause ->
                    failure = cause.message ?: "Git refused that."
                    store.showError(failure!!)
                },
            )
        }
    }

    S5Screen(
        title = "Branches",
        subtitle = statusState.value.valueOrNull?.let { "on ${it.branch}" }.orEmpty(),
        onBack = onBack,
        loading = busy,
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            Row(
                Modifier.fillMaxWidth().padding(S5Theme.spacing.gutter),
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                S5TextField(
                    value = newBranch,
                    onValueChange = { newBranch = it },
                    placeholder = "New branch",
                    singleLine = true,
                    leadingIcon = Icons.AutoMirrored.Rounded.CallSplit,
                    modifier = Modifier.weight(1f),
                )
                S5Button(
                    text = "Create",
                    onClick = {
                        val name = newBranch.trim()
                        newBranch = ""
                        run { store.workspace.createBranch(env, id, name) }
                    },
                    emphasis = S5ActionEmphasis.Prominent,
                    enabled = newBranch.isNotBlank() && !busy,
                )
            }
            failure?.let { message ->
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5ErrorState(
                        title = "Branch operation failed",
                        detail = message,
                        onRetry = { failure = null },
                        retryLabel = "Dismiss",
                    )
                }
            }
            when (remote) {
                is Remote.Loading -> S5LoadingState("Listing branches…")
                is Remote.Failed ->
                    Box(Modifier.padding(S5Theme.spacing.gutter)) {
                        S5ErrorState(
                            title = "Couldn't list branches",
                            detail = remote.message,
                            onRetry = reload,
                        )
                    }
                is Remote.Loaded ->
                    LazyColumn(
                        Modifier.fillMaxSize(),
                        contentPadding = PaddingValues(horizontal = S5Theme.spacing.gutter),
                        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
                    ) {
                        itemsIndexed(branches, key = { _, branch -> branch.name }) { index, branch ->
                            Box(Modifier.animateItem()) {
                            S5SelectableRow(
                                label = branch.name,
                                supporting =
                                    listOfNotNull(
                                            if (branch.remote) "remote" else "local",
                                            branch.ageLabel.takeIf { it.isNotBlank() },
                                        )
                                        .joinToString(" · "),
                                selected = branch.current,
                                onClick = {
                                    if (!branch.current) {
                                        run { store.workspace.switchBranch(env, id, branch.name) }
                                    }
                                },
                                leading = {
                                    Icon(Icons.AutoMirrored.Rounded.CallSplit, contentDescription = null)
                                },
                                position = rowPosition(index, branches.size),
                            )
                            }
                        }
                    }
            }
        }
    }
}

/**
 * Confirmation for push/pull/sync.
 *
 * "Publish branch" is absent: the gateway has no publish command, and offering a
 * button that cannot run is worse than not offering it.
 */
@Composable
fun GitConfirmScreen(store: AppStore, environmentId: String, threadId: String, onBack: () -> Unit) {
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val id = remember(threadId) { ThreadId(threadId) }
    val (state, _) = rememberGitStatus(store, environmentId, threadId)
    val status = state.value.valueOrNull
    val scope = rememberCoroutineScope()
    var selected by remember { mutableStateOf("sync") }
    var running by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }

    val operations =
        listOf(
            Triple(
                "pull",
                "Pull" to
                    (status?.let { "Fast-forward ${it.behind} commits" } ?: "Fetch from the remote"),
                Icons.Rounded.CloudDownload,
            ),
            Triple(
                "push",
                "Push" to (status?.let { "Send ${it.ahead} local commits" } ?: "Send local commits"),
                Icons.Rounded.CloudUpload,
            ),
            Triple("sync", "Sync" to "Pull then push", Icons.Rounded.Sync),
            Triple(
                "create_pr",
                "Create pull request" to "Push this branch and open or reuse its pull request",
                Icons.AutoMirrored.Rounded.CallMerge,
            ),
        )

    S5Screen(
        title = "Confirm",
        subtitle = status?.branch.orEmpty(),
        prominence = S5TopBarProminence.Centered,
        onBack = onBack,
        loading = running,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            Column(
                Modifier.padding(horizontal = S5Theme.spacing.gutter),
                verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
            ) {
                operations.forEachIndexed { index, (operationId, labels, icon) ->
                    S5SelectableRow(
                        label = labels.first,
                        supporting = labels.second,
                        selected = selected == operationId,
                        onClick = { selected = operationId },
                        leading = { Icon(icon, contentDescription = null) },
                        position = rowPosition(index, operations.size),
                    )
                }
            }
            failure?.let { message ->
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5ErrorState(
                        title = "That didn't run",
                        detail = message,
                        onRetry = { failure = null },
                        retryLabel = "Dismiss",
                    )
                }
            }
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                S5Notice(
                    icon = Icons.Rounded.Warning,
                    text = "Operations run on the machine's checkout, not on this device.",
                )
            }
            Row(
                Modifier.fillMaxWidth().padding(horizontal = S5Theme.spacing.gutter),
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                S5Button(
                    text = if (running) "Running…" else "Run",
                    onClick = {
                        if (running) return@S5Button
                        running = true
                        failure = null
                        scope.launch {
                            val outcome =
                                runCatching {
                                    val label =
                                        when (selected) {
                                            "pull" -> "Pulling changes"
                                            "push" -> "Pushing changes"
                                            "create_pr" -> "Creating pull request"
                                            else -> "Syncing changes"
                                        }
                                    val actionId = store.beginAction(label, status?.branch)
                                    try {
                                        val result =
                                            when (selected) {
                                                "pull" -> {
                                                    store.workspace.pull(env, id)
                                                    null
                                                }
                                                "push" ->
                                                    store.workspace.push(env, id) { progress ->
                                                        store.updateAction(
                                                            actionId,
                                                            progress.label,
                                                            progress.description,
                                                        )
                                                    }
                                                "create_pr" ->
                                                    store.workspace.createPullRequest(env, id) { progress ->
                                                        store.updateAction(
                                                            actionId,
                                                            progress.label,
                                                            progress.description,
                                                        )
                                                    }
                                                else -> {
                                                    store.updateAction(actionId, "Pulling changes")
                                                    store.workspace.pull(env, id)
                                                    store.workspace.push(env, id) { progress ->
                                                        store.updateAction(
                                                            actionId,
                                                            progress.label,
                                                            progress.description,
                                                        )
                                                    }
                                                }
                                            }
                                        store.finishAction(
                                            actionId,
                                            result?.title
                                                ?: if (selected == "pull") "Pull complete" else "Sync complete",
                                            result?.description,
                                            result?.pullRequestUrl,
                                        )
                                    } catch (cause: Throwable) {
                                        store.failAction(
                                            actionId,
                                            "Source control action failed",
                                            cause.message ?: "Git refused that operation.",
                                        )
                                        throw cause
                                    }
                                }
                            running = false
                            outcome.fold(
                                onSuccess = { onBack() },
                                onFailure = { cause ->
                                    failure = cause.message ?: "Git refused that operation."
                                },
                            )
                        }
                    },
                    emphasis = S5ActionEmphasis.Primary,
                    icon = Icons.Rounded.Sync,
                    enabled = !running,
                )
                S5Button(
                    text = "Cancel",
                    onClick = onBack,
                    emphasis = S5ActionEmphasis.Primary,
                    style = S5ButtonStyle.Text,
                )
            }
        }
    }
}

/** Source control home: common actions plus conflict state. */
@Composable
fun SourceControlScreen(
    store: AppStore,
    environmentId: String,
    threadId: String,
    onBack: () -> Unit,
    onOpen: (String) -> Unit,
) {
    val (state, _) = rememberGitStatus(store, environmentId, threadId)
    S5Screen(
        title = "Source control",
        subtitle = state.value.valueOrNull?.branch.orEmpty(),
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            S5RowGroup(title = "Common actions") {
                val rows =
                    listOf(
                        Triple("Commit", Icons.Rounded.Commit, "git/commit"),
                        Triple("Sync", Icons.Rounded.Sync, "git-confirm"),
                        Triple("Branches", Icons.AutoMirrored.Rounded.CallSplit, "git/branches"),
                        Triple("Review", Icons.Rounded.Difference, "review"),
                        Triple("Pull requests", Icons.AutoMirrored.Rounded.CallMerge, "pull-requests"),
                    )
                rows.forEachIndexed { index, (label, icon, route) ->
                    S5SettingsRow(
                        icon = icon,
                        label = label,
                        onClick = { onOpen(route) },
                        position = rowPosition(index, rows.size),
                    )
                }
            }
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                S5Notice(
                    icon = Icons.Rounded.Warning,
                    text = "No conflicts. If a merge stops, the conflicting files appear here.",
                )
            }
        }
    }
}

/** PR list and detail metadata. */
@Composable
fun PullRequestsScreen(store: AppStore, environmentId: String, onBack: () -> Unit) {
    val threads by store.workspace.threads.collectAsStateWithLifecycle()
    // Scoped to one environment: a PR list that merged two machines would show
    // the same repository twice with different local branches.
    val pullRequests =
        remember(threads, environmentId) {
            threads
                .filter { it.environmentId.value == environmentId }
                .mapNotNull { thread -> thread.pullRequest?.let { thread to it } }
        }

    S5Screen(
        title = "Pull requests",
        subtitle = "${pullRequests.size} open or merged",
        onBack = onBack,
    ) { padding ->
        if (pullRequests.isEmpty()) {
            S5EmptyState(
                icon = Icons.AutoMirrored.Rounded.CallMerge,
                title = "No pull requests",
                detail = "Threads that opened a PR show it here.",
                modifier = Modifier.padding(padding),
            )
        } else {
            LazyColumn(
                Modifier.fillMaxSize().padding(padding),
                contentPadding =
                    PaddingValues(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.small),
                verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                items(pullRequests.size, key = { pullRequests[it].second.number }) { index ->
                    Box(Modifier.animateItem()) {
                    val (thread, pr) = pullRequests[index]
                    S5Card(tone = S5CardTone.Standard, modifier = Modifier.fillMaxWidth()) {
                        Column(
                            Modifier.padding(S5Theme.spacing.large),
                            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                        ) {
                            Row(
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                            ) {
                                Icon(
                                    if (pr.state == PullRequestState.Merged) Icons.AutoMirrored.Rounded.MergeType
                                    else Icons.AutoMirrored.Rounded.CallMerge,
                                    contentDescription = null,
                                    modifier = Modifier.size(18.dp),
                                    tint =
                                        if (pr.state == PullRequestState.Merged) S5Theme.status.settled
                                        else MaterialTheme.colorScheme.primary,
                                )
                                Text("#${pr.number}", style = S5Theme.code.codeEmphasized)
                                S5StatusPill(
                                    label = pr.state.name,
                                    containerColor =
                                        if (pr.state == PullRequestState.Merged)
                                            S5Theme.status.settledContainer
                                        else MaterialTheme.colorScheme.secondaryContainer,
                                    contentColor =
                                        if (pr.state == PullRequestState.Merged)
                                            S5Theme.status.onSettledContainer
                                        else MaterialTheme.colorScheme.onSecondaryContainer,
                                )
                            }
                            Text(pr.title, style = MaterialTheme.typography.titleSmallEmphasized)
                            Text(
                                "${thread.branch} → main · ${thread.changedFiles} files",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            S5Button(
                                text = "Open on GitHub",
                                onClick = {},
                                emphasis = S5ActionEmphasis.Prominent,
                                style = S5ButtonStyle.Tonal,
                                icon = Icons.AutoMirrored.Rounded.OpenInNew,
                            )
                        }
                    }
                    }
                }
            }
        }
    }
}
