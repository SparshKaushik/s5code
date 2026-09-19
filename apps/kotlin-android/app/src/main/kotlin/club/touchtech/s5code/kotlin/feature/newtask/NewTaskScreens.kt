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
import androidx.compose.foundation.lazy.items
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
import androidx.compose.runtime.LaunchedEffect
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
import club.touchtech.s5code.kotlin.design.component.S5ErrorState
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.component.S5ProjectIcon
import club.touchtech.s5code.kotlin.design.component.S5LoadingState
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5ProviderAvatar
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SectionHeader
import club.touchtech.s5code.kotlin.design.component.S5SelectableRow
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.rememberDraftTextFieldState
import club.touchtech.s5code.kotlin.design.component.rowPosition
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.feature.connections.connectionPresentation
import club.touchtech.s5code.kotlin.feature.home.buildProjectScopes
import club.touchtech.s5code.kotlin.feature.home.projectScopeSelectionTarget
import club.touchtech.s5code.kotlin.feature.home.sortProjectScopes
import club.touchtech.s5code.kotlin.feature.connections.environmentIcon
import club.touchtech.s5code.kotlin.feature.connections.showRetry
import club.touchtech.s5code.kotlin.feature.thread.Dictation
import club.touchtech.s5code.kotlin.feature.thread.DictationMicControl
import club.touchtech.s5code.kotlin.feature.thread.DictationToolbar
import club.touchtech.s5code.kotlin.feature.thread.rememberDictation
import club.touchtech.s5code.kotlin.feature.settings.ModelSearchScope
import club.touchtech.s5code.kotlin.feature.settings.TaskSettingsSheet
import club.touchtech.s5code.kotlin.data.IncomingShareAttachmentType
import club.touchtech.s5code.kotlin.data.IncomingShareDestination
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import club.touchtech.s5code.kotlin.model.ComposerImageCandidate
import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.model.EnvironmentKind
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ThreadSort
import club.touchtech.s5code.kotlin.model.WorkspaceMode
import club.touchtech.s5code.kotlin.platform.active
import club.touchtech.s5code.kotlin.platform.composerImageReceiver
import club.touchtech.s5code.kotlin.platform.rememberComposerImageIntake
import club.touchtech.s5code.kotlin.platform.rememberComposerImagePicker
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

/**
 * `deriveProjectEmptyState` from RN's NewTaskRouteScreen: what the picker says
 * when there are no scopes to list, and whether a spinner belongs next to it.
 */
private data class ProjectEmptyState(val title: String, val detail: String, val loading: Boolean)

private fun projectEmptyState(
    enabled: List<club.touchtech.s5code.kotlin.model.Environment>,
    hasSnapshot: Boolean,
): ProjectEmptyState {
    if (enabled.isEmpty()) {
        return ProjectEmptyState(
            "No environments connected",
            "Add an environment before creating a task.",
            loading = false,
        )
    }
    val offline = enabled.all { it.state == ConnectionState.Offline }
    val error = enabled.firstOrNull { it.state == ConnectionState.AuthRequired }
    val connecting =
        enabled.any {
            it.state == ConnectionState.Connecting || it.state == ConnectionState.Recovering
        }
    if (!hasSnapshot) {
        if (offline) {
            return ProjectEmptyState(
                "Environment unavailable",
                enabled.firstNotNullOfOrNull { it.lastError.ifBlank { null } }
                    ?: "The saved environment is offline. Check the URL or start the environment, then retry.",
                loading = false,
            )
        }
        if (error != null) {
            return ProjectEmptyState(
                "Environment unavailable",
                error.lastError.ifBlank {
                    "The saved environment is offline. Check the URL or start the environment, then retry."
                },
                loading = false,
            )
        }
        if (connecting) {
            return ProjectEmptyState(
                "Connecting to environment",
                "Loading projects from the saved environment.",
                loading = true,
            )
        }
        return ProjectEmptyState(
            "Environment unavailable",
            enabled.firstNotNullOfOrNull { it.lastError.ifBlank { null } }
                ?: "The saved environment is offline. Check the URL or start the environment, then retry.",
            loading = false,
        )
    }
    return ProjectEmptyState(
        "No projects found",
        "The connected environment did not report any projects.",
        loading = false,
    )
}

/** Step 1: pick the project the task runs in, matching RN's NewTaskRouteScreen. */
@Composable
fun NewTaskProjectScreen(
    store: AppStore,
    onBack: () -> Unit,
    onProjectChosen: () -> Unit,
    onAddProject: () -> Unit,
    onAddEnvironment: () -> Unit,
) {
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val threads by store.workspace.threads.collectAsStateWithLifecycle()
    val preferences by store.preferences.collectAsStateWithLifecycle()
    val draft by store.draft.collectAsStateWithLifecycle()

    // A switched-off environment contributes no projects and no connection to
    // wait on, so only enabled rows drive the state.
    val enabled = remember(environments) { environments.filter { it.isEnabled } }
    val hasReadyEnvironment = enabled.any { it.state == ConnectionState.Connected }
    val hasSnapshot = enabled.any { it.snapshotLoaded }

    // Same scopes RN offers: repository-grouped projects sorted by recent
    // activity, each row selecting the member on the draft's environment.
    val scopes =
        remember(projects, preferences.projectGrouping, threads) {
            sortProjectScopes(
                buildProjectScopes(projects, preferences.projectGrouping),
                threads,
                ThreadSort.Recent,
            )
        }
    val empty = remember(enabled, hasSnapshot) { projectEmptyState(enabled, hasSnapshot) }

    val pendingShare by store.pendingShare.collectAsStateWithLifecycle()
    val shareSubtitle =
        pendingShare?.let { share ->
            when {
                share.attachments.isEmpty() -> "Choose a project for what you shared"
                share.attachments.size == 1 ->
                    "Choose a project for the " +
                        "${if (share.attachments.first().type == IncomingShareAttachmentType.Image) "image" else "file"} you shared"
                else ->
                    "Choose a project for the ${share.attachments.size} " +
                        "${if (share.attachments.all { it.type == IncomingShareAttachmentType.Image }) "images" else "files"} you shared"
            }
        }

    fun choose(project: club.touchtech.s5code.kotlin.model.Project) {
        store.updateDraft {
            it.copy(
                environmentId = project.environmentId,
                projectKey = project.id.value,
                branch = project.branch,
            )
        }
        onProjectChosen()
    }

    S5Screen(
        title = if (pendingShare != null) "Start a task" else "Choose project",
        subtitle = shareSubtitle,
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
        actions = {
            if (hasReadyEnvironment) {
                S5IconButton(
                    icon = Icons.Rounded.Add,
                    label = "Add project",
                    onClick = onAddProject,
                )
            }
        },
    ) { padding ->
        if (scopes.isEmpty()) {
            Column(
                Modifier.fillMaxSize().padding(padding).padding(S5Theme.spacing.gutter),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                if (empty.loading) {
                    S5InlineLoading(modifier = Modifier.padding(bottom = S5Theme.spacing.small))
                }
                Text(
                    empty.title,
                    style = MaterialTheme.typography.titleMediumEmphasized,
                    textAlign = TextAlign.Center,
                )
                Text(
                    empty.detail,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(top = S5Theme.spacing.tiny),
                )
                Box(Modifier.padding(top = S5Theme.spacing.medium)) {
                    if (!hasReadyEnvironment) {
                        S5Button(
                            text = "Add environment",
                            onClick = onAddEnvironment,
                            emphasis = S5ActionEmphasis.Primary,
                        )
                    } else {
                        S5Button(
                            text = "Add new project",
                            onClick = onAddProject,
                            emphasis = S5ActionEmphasis.Primary,
                        )
                    }
                }
            }
        } else {
            LazyColumn(
                Modifier.fillMaxSize().padding(padding),
                contentPadding =
                    PaddingValues(
                        start = S5Theme.spacing.gutter,
                        end = S5Theme.spacing.gutter,
                        bottom = 32.dp,
                    ),
                verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
            ) {
                items(scopes.size) { index ->
                    val scope = scopes[index]
                    val target =
                        projectScopeSelectionTarget(
                            scope,
                            draft.environmentId.value.takeIf { it.isNotEmpty() },
                        )
                    S5SelectableRow(
                        label = scope.title,
                        supporting =
                            if (scope.projects.size > 1) "${scope.projects.size} workspaces"
                            else target.workspaceRoot,
                        selected = false,
                        onClick = { choose(target) },
                        leading = {
                            S5ProjectIcon(
                                project = scope.representative,
                                resolveUrl = store.workspace::projectIconUrl,
                                size = 28.dp,
                            )
                        },
                        trailing = {
                            Icon(
                                Icons.AutoMirrored.Rounded.KeyboardArrowRight,
                                contentDescription = null,
                                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        },
                        position = rowPosition(index, scopes.size),
                    )
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
    val preferences by store.preferences.collectAsStateWithLifecycle()
    val catalogRefreshing by store.catalogRefreshing.collectAsStateWithLifecycle()
    // Model and settings open over the draft rather than pushing a page, so the
    // prompt you were writing stays on screen behind the sheet.
    var settingsOpen by remember { mutableStateOf(false) }
    val attachmentError by store.attachmentError.collectAsStateWithLifecycle()
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val providerCatalog by store.workspace.providerCatalog.collectAsStateWithLifecycle()
    val providerCatalogs by store.workspace.providerCatalogs.collectAsStateWithLifecycle()
    val machineCatalog =
        remember(draft.environmentId, providerCatalogs, providerCatalog, environments) {
            val scoped = providerCatalogs[draft.environmentId]
            if (scoped != null && scoped.isNotEmpty()) {
                scoped
            } else if (environments.none { it.id == draft.environmentId }) {
                providerCatalog
            } else {
                scoped ?: emptyList()
            }
        }
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

    // A share waiting in the inbox merges into the draft once a project exists.
    // Import consumes the inbox record, so this effect runs at most once per
    // share; the draft's `importedShareIds` is the receipt for process death.
    val pendingShare by store.pendingShare.collectAsStateWithLifecycle()
    var shareImportAttempted by remember(draft.projectKey) { mutableStateOf(setOf<String>()) }
    LaunchedEffect(pendingShare?.id, draft.environmentId, draft.projectKey) {
        val share = pendingShare ?: return@LaunchedEffect
        if (share.id in draft.importedShareIds || share.id in shareImportAttempted) {
            return@LaunchedEffect
        }
        if (draft.environmentId.value.isEmpty() || draft.projectKey.isEmpty()) {
            return@LaunchedEffect
        }
        shareImportAttempted = shareImportAttempted + share.id
        store.importIncomingShare(
            share.id,
            IncomingShareDestination(
                environmentId = draft.environmentId.value,
                projectId = draft.projectKey,
            ),
        )
    }

    val project = remember(projects, draft) { projects.firstOrNull { it.id.value == draft.projectKey } }
    val environment = remember(environments, draft) { environments.firstOrNull { it.id == draft.environmentId } }
    // Switching away is only a choice while a second enabled environment exists.
    val enabledEnvironmentCount = remember(environments) { environments.count { it.isEnabled } }
    val canStart = draft.prompt.isNotBlank() && !creating && environment?.isEnabled != false

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
                dictation = rememberDictation("new-task", promptState),
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
                modelLabel =
                    machineCatalog
                        .firstOrNull {
                            it.instance.instanceId == draft.settings.provider.instanceId
                        }
                        ?.modelLabel(draft.settings.model)
                        ?: draft.settings.model,
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
                    environment?.let { environmentIcon(it) } ?: Icons.Rounded.Computer,
                onEnvironment = onEnvironment,
                canChangeEnvironment = enabledEnvironmentCount > 1,
                modifier = Modifier.padding(top = S5Theme.spacing.section),
            )
        }
    }

    S5AttachmentPreviewDialog(
        attachment = previewAttachment,
        onDismiss = { previewAttachment = null },
    )

    LaunchedEffect(draft.environmentId, machineCatalog) {
        if (machineCatalog.isNotEmpty()) {
            val hasMatchingProvider =
                machineCatalog.any { it.instance.instanceId == draft.settings.provider.instanceId }
            if (!hasMatchingProvider) {
                val first = machineCatalog.first()
                store.updateDraft {
                    it.copy(
                        settings =
                            it.settings.copy(
                                provider = first.instance,
                                model = first.models.firstOrNull() ?: it.settings.model,
                                options = emptyList(),
                            )
                    )
                }
            } else {
                val currentEntry =
                    machineCatalog.firstOrNull { it.instance.instanceId == draft.settings.provider.instanceId }
                if (currentEntry != null &&
                    currentEntry.models.isNotEmpty() &&
                    draft.settings.model !in currentEntry.models
                ) {
                    store.updateDraft {
                        it.copy(
                            settings =
                                it.settings.copy(
                                    model = currentEntry.models.first(),
                                    options = emptyList(),
                                )
                        )
                    }
                }
            }
        }
    }

    if (settingsOpen) {
        TaskSettingsSheet(
            settings = draft.settings,
            catalog = machineCatalog,
            modelsFor = { provider -> store.modelsFor(provider, draft.environmentId) },
            onSettingsChange = { settings -> store.updateDraft { it.copy(settings = settings) } },
            onDismiss = { settingsOpen = false },
            // A draft has no context to hand over, so searching every agent is free.
            searchScope = ModelSearchScope.AllProviders,
            favorites = preferences.modelFavorites,
            onToggleFavorite = { store.toggleModelFavorite(it.instanceId, it.model) },
            catalogRefreshing = draft.environmentId.value in catalogRefreshing,
            onRefreshCatalog = { store.refreshProviderCatalog(draft.environmentId) },
            planModeEnabled = preferences.planModeEnabled,
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
    dictation: Dictation?,
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
    /** Display name for the chip, resolved from the catalog — never the slug. */
    modelLabel: String,
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
                    // Same freeze as the thread composer: a keystroke cannot
                    // invalidate a transcript mid-flight.
                    enabled = dictation?.state?.active != true,
                    onSubmitShortcut = {
                        if (canStart && dictation?.state?.active != true) onStart()
                    },
                    modifier =
                        Modifier.fillMaxWidth()
                            .heightIn(min = 72.dp)
                            .padding(horizontal = S5Theme.spacing.tiny)
                            .composerImageReceiver(onAddImages),
                )

                if (dictation?.presentation?.statusLabel != null) {
                    DictationToolbar(
                        dictation = dictation,
                        modifier = Modifier.padding(top = S5Theme.spacing.small),
                    )
                } else {
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
                        label = modelLabel,
                        leading = { S5ProviderAvatar(provider, size = 20.dp) },
                        trailingIcon = Icons.Rounded.ExpandMore,
                        onClick = onOpenSettings,
                        contentDescription = "Model and settings",
                        modifier = Modifier.widthIn(max = 180.dp),
                    )
                    Box(Modifier.weight(1f))
                    DictationMicControl(dictation)
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
}

/**
 * `resolveEnvironmentProjectMatch` in the RN client: switching machines follows
 * the same repo. Repository identity wins, then workspace basename, then title;
 * a known *different* repository never matches on the weaker signals.
 */
private fun environmentProjectMatch(
    projectsOnTarget: List<club.touchtech.s5code.kotlin.model.Project>,
    selected: club.touchtech.s5code.kotlin.model.Project?,
): club.touchtech.s5code.kotlin.model.Project? {
    val repositoryKey = selected?.repositoryIdentity?.canonicalKey
    val workspaceBasename = selected?.workspaceRoot?.split('/')?.lastOrNull()?.takeIf { it.isNotEmpty() }
    fun knownMismatch(project: club.touchtech.s5code.kotlin.model.Project): Boolean {
        val key = project.repositoryIdentity?.canonicalKey ?: return false
        return repositoryKey != null && key != repositoryKey
    }
    return (repositoryKey?.let { key ->
            projectsOnTarget.firstOrNull { it.repositoryIdentity?.canonicalKey == key }
        })
        ?: (workspaceBasename?.let { base ->
            projectsOnTarget.firstOrNull {
                !knownMismatch(it) && it.workspaceRoot.split('/').lastOrNull() == base
            }
        })
        ?: (selected?.let { wanted ->
            projectsOnTarget.firstOrNull { !knownMismatch(it) && it.title == wanted.title }
        })
        ?: projectsOnTarget.firstOrNull()
}

/** Environment picker with reachability. Switched-off environments are hidden. */
@Composable
fun NewTaskEnvironmentScreen(store: AppStore, onBack: () -> Unit) {
    val allEnvironments by store.workspace.environments.collectAsStateWithLifecycle()
    val environments = remember(allEnvironments) { allEnvironments.filter { it.isEnabled } }
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val draft by store.draft.collectAsStateWithLifecycle()
    S5Screen(title = "Environment", subtitle = "Where should this run?", onBack = onBack) { padding ->
        LazyColumn(
            Modifier.fillMaxSize().padding(padding),
            contentPadding = PaddingValues(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.small),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
        ) {
            items(environments, key = { it.id.value }) { environment ->
                val health = connectionPresentation(environment.state)
                // A task can only be drafted against a live connection — RN's
                // canCreateProjectInEnvironment.
                val creatable =
                    environment.state ==
                        club.touchtech.s5code.kotlin.model.ConnectionState.Connected
                S5SelectableRow(
                    label = environment.label,
                    supporting = "${environment.host} · ${health.label}",
                    selected = environment.id == draft.environmentId,
                    onClick = {
                        if (creatable) {
                            // RN's selectEnvironment follows the repo to the
                            // target machine rather than leaving the draft
                            // pointing at a project the environment lacks.
                            val current =
                                projects.firstOrNull { it.id.value == draft.projectKey }
                            val match =
                                environmentProjectMatch(
                                    projects.filter { it.environmentId == environment.id },
                                    current,
                                )
                            store.updateDraft {
                                it.copy(
                                    environmentId = environment.id,
                                    projectKey = match?.id?.value ?: it.projectKey,
                                    branch = match?.branch ?: it.branch,
                                )
                            }
                            onBack()
                        }
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
