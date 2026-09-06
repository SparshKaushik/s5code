package club.touchtech.s5code.kotlin.feature.newtask

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.CallSplit
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.ArrowUpward
import androidx.compose.material.icons.rounded.BrokenImage
import androidx.compose.material.icons.rounded.Cloud
import androidx.compose.material.icons.rounded.Computer
import androidx.compose.material.icons.rounded.CreateNewFolder
import androidx.compose.material.icons.rounded.ExpandMore
import androidx.compose.material.icons.rounded.Folder
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.Remote
import club.touchtech.s5code.kotlin.data.rememberRetryableRemote
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5AttachmentPreviewDialog
import club.touchtech.s5code.kotlin.design.component.S5AttachmentStrip
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5ComposerAction
import club.touchtech.s5code.kotlin.design.component.S5ComposerControl
import club.touchtech.s5code.kotlin.design.component.S5ComposerField
import club.touchtech.s5code.kotlin.design.component.S5ComposerSurface
import club.touchtech.s5code.kotlin.design.component.S5ComposerToolbarRow
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5ErrorState
import club.touchtech.s5code.kotlin.design.component.S5LoadingState
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5ProviderAvatar
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SearchField
import club.touchtech.s5code.kotlin.design.component.S5SectionHeader
import club.touchtech.s5code.kotlin.design.component.S5SelectableRow
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.S5WaitState
import club.touchtech.s5code.kotlin.design.component.rememberDraftTextFieldState
import club.touchtech.s5code.kotlin.design.component.rowPosition
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.feature.connections.connectionPresentation
import club.touchtech.s5code.kotlin.feature.connections.waitNotice
import club.touchtech.s5code.kotlin.feature.settings.ModelSearchScope
import club.touchtech.s5code.kotlin.feature.settings.TaskSettingsSheet
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import club.touchtech.s5code.kotlin.model.ComposerImageCandidate
import club.touchtech.s5code.kotlin.model.EnvironmentKind
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.WorkspaceMode
import club.touchtech.s5code.kotlin.platform.composerImageReceiver
import club.touchtech.s5code.kotlin.platform.rememberComposerImageIntake
import club.touchtech.s5code.kotlin.platform.rememberComposerImagePicker
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

/** Step 1: pick the project the task runs in. */
@Composable
fun NewTaskProjectScreen(
    store: AppStore,
    onBack: () -> Unit,
    onProjectChosen: () -> Unit,
    onAddProject: () -> Unit,
) {
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    var query by rememberSaveable { mutableStateOf("") }

    val filtered =
        remember(projects, query) {
            val needle = query.trim().lowercase()
            if (needle.isEmpty()) projects
            else
                projects.filter { project ->
                    listOfNotNull(project.title, project.repository, project.workspaceRoot).any {
                        it.lowercase().contains(needle)
                    }
                }
        }
    val grouped = remember(filtered, environments) { filtered.groupBy { it.environmentId } }
    // Same split as the RN route screen: "Connecting to environment" while the
    // project catalog has not arrived, and only then "No projects yet".
    val wait =
        remember(environments, projects.isEmpty()) {
            waitNotice(
                states = environments.map { it.state },
                environmentLabel = environments.singleOrNull()?.label,
                resourceName = "projects",
                hasContent = projects.isNotEmpty(),
            )
        }

    S5Screen(
        title = "New task",
        subtitle = "Choose a project",
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            S5SearchField(
                value = query,
                onValueChange = { query = it },
                placeholder = "Search projects",
                modifier =
                    Modifier.padding(
                        horizontal = S5Theme.spacing.gutter,
                        vertical = S5Theme.spacing.small,
                    ),
            )
            if (filtered.isEmpty() && wait != null && query.isBlank()) {
                S5WaitState(
                    title = wait.title,
                    detail = wait.detail,
                    icon = Icons.Rounded.Folder,
                    spinning = wait.spinning,
                )
            } else if (filtered.isEmpty()) {
                S5EmptyState(
                    icon = Icons.Rounded.Folder,
                    title = if (query.isBlank()) "No projects yet" else "No matches",
                    detail =
                        if (query.isBlank()) {
                            "Add a project by cloning a repository or pointing at a folder on the machine."
                        } else {
                            "Nothing matches \"$query\"."
                        },
                    actionLabel = "Add project",
                    onAction = onAddProject,
                )
            } else {
                LazyColumn(
                    Modifier.fillMaxSize(),
                    contentPadding = PaddingValues(bottom = 32.dp),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
                ) {
                    grouped.forEach { (environmentId, environmentProjects) ->
                        val environment = environments.firstOrNull { it.id == environmentId }
                        item(key = "env-${environmentId.value}") {
                            S5SectionHeader(environment?.label ?: environmentId.value)
                        }
                        items(environmentProjects, key = { it.id.value }) { project ->
                            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                                S5SelectableRow(
                                    label = project.title,
                                    supporting = project.repository ?: project.workspaceRoot,
                                    selected = false,
                                    onClick = {
                                        store.updateDraft {
                                            it.copy(
                                                environmentId = environmentId,
                                                projectKey = project.id.value,
                                                branch = project.branch,
                                            )
                                        }
                                        onProjectChosen()
                                    },
                                    leading = { Icon(Icons.Rounded.Folder, contentDescription = null) },
                                )
                            }
                        }
                    }
                    item(key = "add-project") {
                        Box(Modifier.padding(S5Theme.spacing.gutter)) {
                            S5Button(
                                text = "Add project",
                                onClick = onAddProject,
                                icon = Icons.Rounded.CreateNewFolder,
                                emphasis = S5ActionEmphasis.Primary,
                                style = S5ButtonStyle.Tonal,
                            )
                        }
                    }
                }
            }
        }
    }
}

/**
 * Step 2: the draft, shaped like the RN client rather than a settings form.
 *
 * The screen is a question and a composer, not a stack of labelled fields. A
 * centered "What should we build in <project>?" fills the empty space above,
 * with the project and environment as inline controls inside the sentence, and
 * everything you actually operate lives in a docked composer at the bottom: the
 * workspace and branch controls in a row above it, then the prompt, then the
 * attach/model/send toolbar along its bottom edge.
 *
 * That layout is not decoration. The prompt is the only thing on this screen
 * that takes real typing, so it sits under the thumb with the keyboard, and the
 * context choices sit next to it instead of scrolling away above it.
 */
@Composable
fun NewTaskDraftScreen(
    store: AppStore,
    onBack: () -> Unit,
    onProject: () -> Unit,
    onEnvironment: () -> Unit,
    onBranch: () -> Unit,
    onCreated: (String, String) -> Unit,
) {
    val draft by store.draft.collectAsStateWithLifecycle()
    // Model and settings open over the draft rather than pushing a page, so the
    // prompt you were writing stays on screen behind the sheet.
    var settingsOpen by remember { mutableStateOf(false) }
    val attachmentError by store.attachmentError.collectAsStateWithLifecycle()
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val providerCatalog by store.workspace.providerCatalog.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    var creating by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }
    var previewAttachment by remember { mutableStateOf<ComposerAttachment?>(null) }
    // Intake copies incoming images into cache, so a draft that outlives the
    // clipboard grant still resolves its attachments.
    val addImages = rememberComposerImageIntake(store::addNewTaskDraftImages)
    val promptState =
        rememberDraftTextFieldState(
            key = Unit,
            value = draft.prompt,
            onValueChange = { text -> store.updateDraft { it.copy(prompt = text) } },
        )
    val pickImages = rememberComposerImagePicker(
        remaining = ComposerAttachmentLimits.MAX_ATTACHMENTS - draft.attachments.size,
        onImages = addImages,
    )

    val project = remember(projects, draft) { projects.firstOrNull { it.id.value == draft.projectKey } }
    val environment = remember(environments, draft) { environments.firstOrNull { it.id == draft.environmentId } }
    val canStart = draft.prompt.isNotBlank() && !creating

    val start: () -> Unit = {
        if (canStart) {
            creating = true
            scope.launch {
                try {
                    val id = store.enqueueNewTask(draft)
                    onCreated(draft.environmentId.value, id.value)
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (error: Exception) {
                    val message = error.message ?: "The task could not be saved to the outbox."
                    failure = message
                    store.showError(message)
                } finally {
                    creating = false
                }
            }
        }
    }

    S5Screen(
        title = "New task",
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
        bottomBar = {
            NewTaskComposerDock(
                promptState = promptState,
                attachments = draft.attachments,
                onRemoveAttachment = { attachment -> store.removeNewTaskDraftImage(attachment.id) },
                onPreviewAttachment = { previewAttachment = it },
                onAddImages = addImages,
                onPickImages = pickImages,
                workspaceMode = draft.workspaceMode,
                onToggleWorkspaceMode = {
                    store.updateDraft {
                        it.copy(
                            workspaceMode =
                                if (it.workspaceMode == WorkspaceMode.CurrentCheckout) {
                                    WorkspaceMode.NewWorktree
                                } else {
                                    WorkspaceMode.CurrentCheckout
                                }
                        )
                    }
                },
                branch = draft.branch,
                onBranch = onBranch,
                provider = draft.settings.provider,
                model = draft.settings.model,
                onOpenSettings = { settingsOpen = true },
                creating = creating,
                canStart = canStart,
                onStart = start,
            )
        },
        loading = creating,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            attachmentError?.let { error ->
                Box(
                    Modifier.padding(
                        horizontal = S5Theme.spacing.gutter,
                        vertical = S5Theme.spacing.small,
                    )
                ) {
                    S5Notice(
                        icon = Icons.Rounded.BrokenImage,
                        text = error,
                        containerColor = MaterialTheme.colorScheme.errorContainer,
                        contentColor = MaterialTheme.colorScheme.onErrorContainer,
                        onDismiss = store::clearAttachmentError,
                    )
                }
            }
            failure?.let { message ->
                Box(
                    Modifier.padding(
                        horizontal = S5Theme.spacing.gutter,
                        vertical = S5Theme.spacing.small,
                    )
                ) {
                    S5Notice(
                        icon = Icons.Rounded.BrokenImage,
                        text = message,
                        containerColor = MaterialTheme.colorScheme.errorContainer,
                        contentColor = MaterialTheme.colorScheme.onErrorContainer,
                        onDismiss = { failure = null },
                    )
                }
            }
            NewTaskHero(
                projectTitle = project?.title ?: "a project",
                onProject = onProject,
                environmentLabel = environment?.label ?: draft.environmentId.value,
                environmentIcon =
                    if (environment?.kind == EnvironmentKind.Cloud) Icons.Rounded.Cloud
                    else Icons.Rounded.Computer,
                onEnvironment = onEnvironment,
                canChangeEnvironment = environments.size > 1,
                modifier = Modifier.padding(top = S5Theme.spacing.section),
            )
        }
    }

    S5AttachmentPreviewDialog(
        attachment = previewAttachment,
        onDismiss = { previewAttachment = null },
    )

    if (settingsOpen) {
        TaskSettingsSheet(
            settings = draft.settings,
            catalog = providerCatalog,
            modelsFor = store::modelsFor,
            onSettingsChange = { settings -> store.updateDraft { it.copy(settings = settings) } },
            onDismiss = { settingsOpen = false },
            // A draft has no context to hand over, so searching every agent is free.
            searchScope = ModelSearchScope.AllProviders,
        )
    }
}

/**
 * The question. Project and environment are part of the sentence, because that is
 * where you look for them when deciding whether to start: reading a form row
 * labelled "Environment" is a different, slower act than reading "on MacBook".
 */
@Composable
private fun NewTaskHero(
    projectTitle: String,
    onProject: () -> Unit,
    environmentLabel: String,
    environmentIcon: ImageVector,
    onEnvironment: () -> Unit,
    canChangeEnvironment: Boolean,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier.fillMaxWidth().padding(horizontal = S5Theme.spacing.xxLarge),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.xLarge),
    ) {
        val headline = MaterialTheme.typography.headlineSmall
        Text("What should we build", style = headline, textAlign = TextAlign.Center)
        // FlowRow so a long project name wraps under "in" instead of ellipsizing
        // the one word on this screen the user most needs to read.
        FlowRow(
            horizontalArrangement = Arrangement.Center,
            verticalArrangement = Arrangement.Center,
        ) {
            Text("in ", style = headline)
            Text(
                projectTitle,
                style = headline,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                textDecoration = TextDecoration.Underline,
                modifier =
                    Modifier.widthIn(max = 250.dp)
                        .clip(MaterialTheme.shapes.small)
                        .clickable(onClick = onProject)
                        .semantics { contentDescription = "Project: $projectTitle. Change project" },
            )
            Text("?", style = headline)
        }
        S5ComposerControl(
            label = "on $environmentLabel",
            icon = environmentIcon,
            // With one environment this is a label, not a control: no chevron, no
            // click. It stays full-strength rather than disabled, because a
            // greyed-out row reads as "broken" when it is really "only one".
            trailingIcon =
                if (canChangeEnvironment) Icons.AutoMirrored.Rounded.KeyboardArrowRight else null,
            onClick = if (canChangeEnvironment) onEnvironment else null,
            contentDescription = "Environment: $environmentLabel",
            modifier = Modifier.widthIn(max = 260.dp),
        )
    }
}

/**
 * Docked composer: workspace and branch above, prompt in the card, attach/model
 * and start along its bottom edge. Same surface and controls as the thread
 * composer's expanded state, since it is the same job.
 */
@Composable
private fun NewTaskComposerDock(
    promptState: TextFieldState,
    attachments: List<ComposerAttachment>,
    onRemoveAttachment: (ComposerAttachment) -> Unit,
    onPreviewAttachment: (ComposerAttachment) -> Unit,
    onAddImages: (List<ComposerImageCandidate>) -> Unit,
    onPickImages: () -> Unit,
    workspaceMode: WorkspaceMode,
    onToggleWorkspaceMode: () -> Unit,
    branch: String,
    onBranch: () -> Unit,
    provider: ProviderInstance,
    model: String,
    onOpenSettings: () -> Unit,
    creating: Boolean,
    canStart: Boolean,
    onStart: () -> Unit,
) {
    Column(
        Modifier.fillMaxWidth()
            .imePadding()
            .navigationBarsPadding()
            .padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.small),
        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
    ) {
        S5ComposerToolbarRow {
            S5ComposerControl(
                label =
                    when (workspaceMode) {
                        WorkspaceMode.CurrentCheckout -> "Current checkout"
                        WorkspaceMode.NewWorktree -> "New worktree"
                    },
                icon = Icons.Rounded.Folder,
                onClick = onToggleWorkspaceMode,
                contentDescription =
                    "Workspace: " +
                        when (workspaceMode) {
                            WorkspaceMode.CurrentCheckout -> "current checkout. Switch to a new worktree"
                            WorkspaceMode.NewWorktree -> "new worktree. Switch to the current checkout"
                        },
            )
            S5ComposerControl(
                label = branch,
                icon = Icons.AutoMirrored.Rounded.CallSplit,
                trailingIcon = Icons.AutoMirrored.Rounded.KeyboardArrowRight,
                onClick = onBranch,
                contentDescription = "Branch: $branch",
                modifier = Modifier.widthIn(max = 190.dp),
            )
        }

        S5ComposerSurface(cornerRadius = 26.dp) {
            Column(Modifier.padding(S5Theme.spacing.medium)) {
                if (attachments.isNotEmpty()) {
                    S5AttachmentStrip(
                        attachments = attachments,
                        onRemove = onRemoveAttachment,
                        onPreview = onPreviewAttachment,
                        thumbnailSize = 64.dp,
                        modifier = Modifier.fillMaxWidth().padding(bottom = S5Theme.spacing.small),
                    )
                }

                S5ComposerField(
                    state = promptState,
                    placeholder = "Ask anything…",
                    maxLines = 6,
                    onSubmitShortcut = { if (canStart) onStart() },
                    modifier =
                        Modifier.fillMaxWidth()
                            .heightIn(min = 72.dp)
                            .padding(horizontal = S5Theme.spacing.tiny)
                            .composerImageReceiver(onAddImages),
                )

                S5ComposerToolbarRow(Modifier.padding(top = S5Theme.spacing.small)) {
                    S5ComposerControl(
                        label = null,
                        icon = Icons.Rounded.Add,
                        contentDescription = "Attach image",
                        onClick = onPickImages,
                    )
                    // No explicit paste control. The field accepts image commits
                    // directly (`composerImageReceiver`), so Gboard's own paste key
                    // already works, and the thread composer never had one — two
                    // composers with different toolbars for the same job.
                    S5ComposerControl(
                        label = model,
                        leading = { S5ProviderAvatar(provider, size = 20.dp) },
                        trailingIcon = Icons.Rounded.ExpandMore,
                        onClick = onOpenSettings,
                        contentDescription = "Model and settings",
                        modifier = Modifier.widthIn(max = 180.dp),
                    )
                    Box(Modifier.weight(1f))
                    S5ComposerAction(
                        icon = Icons.Rounded.ArrowUpward,
                        label = if (creating) "Starting task" else "Start task",
                        onClick = onStart,
                        enabled = canStart,
                    )
                }
            }
        }
    }
}

/** Environment picker with reachability. */
@Composable
fun NewTaskEnvironmentScreen(store: AppStore, onBack: () -> Unit) {
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val draft by store.draft.collectAsStateWithLifecycle()
    S5Screen(title = "Environment", subtitle = "Where should this run?", onBack = onBack) { padding ->
        LazyColumn(
            Modifier.fillMaxSize().padding(padding),
            contentPadding = PaddingValues(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.small),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
        ) {
            items(environments, key = { it.id.value }) { environment ->
                val health = connectionPresentation(environment.state)
                S5SelectableRow(
                    label = environment.label,
                    supporting = "${environment.host} · ${health.label}",
                    selected = environment.id == draft.environmentId,
                    onClick = {
                        store.updateDraft { it.copy(environmentId = environment.id) }
                        onBack()
                    },
                    leading = { Icon(health.icon, contentDescription = null) },
                    position = rowPosition(environments.indexOf(environment), environments.size),
                )
            }
        }
    }
}

/** Branch picker with a retryable fetch state. */
@Composable
fun NewTaskBranchScreen(store: AppStore, onBack: () -> Unit) {
    val draft by store.draft.collectAsStateWithLifecycle()
    // The project has no thread yet, so this is the project-scoped listing rather
    // than the thread's worktree.
    val (state, retry) =
        rememberRetryableRemote(draft.environmentId.value, draft.projectKey) {
            store.workspace.projectBranches(draft.environmentId, draft.projectKey)
        }
    val remote = state.value
    val branches = remote.valueOrNull.orEmpty()

    S5Screen(
        title = "Base branch",
        subtitle = if (remote is Remote.Loaded) "${branches.size} branches" else "",
        onBack = onBack,
    ) { padding ->
        when (remote) {
            is Remote.Loading -> S5LoadingState("Listing branches…", Modifier.padding(padding))
            is Remote.Failed ->
                Box(Modifier.padding(padding).padding(S5Theme.spacing.gutter)) {
                    S5ErrorState(title = "Couldn't list branches", detail = remote.message, onRetry = retry)
                }
            is Remote.Loaded ->
                LazyColumn(
                    Modifier.fillMaxSize().padding(padding),
                    contentPadding =
                        PaddingValues(
                            horizontal = S5Theme.spacing.gutter,
                            vertical = S5Theme.spacing.small,
                        ),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
                ) {
                    itemsIndexed(branches, key = { _, branch -> branch.name }) { index, branch ->
                        S5SelectableRow(
                            label = branch.name,
                            supporting =
                                listOfNotNull(
                                        if (branch.current) "current" else null,
                                        if (branch.remote) "remote" else "local",
                                        branch.ageLabel.takeIf { it.isNotBlank() },
                                    )
                                    .joinToString(" · "),
                            selected = branch.name == draft.branch,
                            onClick = {
                                store.updateDraft { it.copy(branch = branch.name) }
                                onBack()
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
