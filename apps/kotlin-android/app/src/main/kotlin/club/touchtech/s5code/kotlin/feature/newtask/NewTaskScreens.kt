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
import androidx.compose.material.icons.rounded.AutoAwesome
import androidx.compose.material.icons.rounded.Terminal
import androidx.compose.material.icons.rounded.BrokenImage
import androidx.compose.material.icons.rounded.Cloud
import androidx.compose.material.icons.rounded.Computer
import androidx.compose.material.icons.rounded.CreateNewFolder
import androidx.compose.material.icons.rounded.ExpandMore
import androidx.compose.material.icons.rounded.Folder
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import kotlinx.coroutines.delay
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
import club.touchtech.s5code.kotlin.data.clampFileAttachmentUploadBytes
import club.touchtech.s5code.kotlin.data.fileAttachmentTooLargeMessage
import club.touchtech.s5code.kotlin.data.isUsageLimitsCommand
import club.touchtech.s5code.kotlin.data.pastedTextShouldFold
import club.touchtech.s5code.kotlin.data.persistPastedTextAttachment
import club.touchtech.s5code.kotlin.model.projectCloneDisplayName
import club.touchtech.s5code.kotlin.model.projectCloneProgressSummary
import club.touchtech.s5code.kotlin.data.formatComposerContextReference
import club.touchtech.s5code.kotlin.data.pullRequestComposerContext
import club.touchtech.s5code.kotlin.feature.thread.ComposerSuggestion
import club.touchtech.s5code.kotlin.feature.thread.ComposerTriggerKind
import club.touchtech.s5code.kotlin.feature.thread.SuggestionPopover
import club.touchtech.s5code.kotlin.feature.thread.detectComposerTrigger
import club.touchtech.s5code.kotlin.feature.thread.rankComposerPaths
import club.touchtech.s5code.kotlin.feature.thread.rankProviderSkill
import club.touchtech.s5code.kotlin.feature.thread.rankSlashCommands
import club.touchtech.s5code.kotlin.feature.thread.replaceComposerTextRange
import club.touchtech.s5code.kotlin.feature.thread.suggestion
import club.touchtech.s5code.kotlin.data.rememberRetryableRemote
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5AttachmentPreviewDialog
import club.touchtech.s5code.kotlin.design.component.S5SearchField
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
import club.touchtech.s5code.kotlin.model.BranchRef
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import club.touchtech.s5code.kotlin.model.ComposerImageCandidate
import club.touchtech.s5code.kotlin.model.ComposerPullRequestCandidate
import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.model.EnvironmentKind
import club.touchtech.s5code.kotlin.model.ProjectId
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ProviderSkill
import club.touchtech.s5code.kotlin.model.RuntimeMode
import club.touchtech.s5code.kotlin.model.SlashCommand
import club.touchtech.s5code.kotlin.model.ThreadSort
import club.touchtech.s5code.kotlin.model.WorkspaceMode
import club.touchtech.s5code.kotlin.platform.active
import club.touchtech.s5code.kotlin.platform.composerImageReceiver
import club.touchtech.s5code.kotlin.platform.composerPastedTextReceiver
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

    val coroutineScope = rememberCoroutineScope()
    var startingScratch by remember { mutableStateOf(false) }
    val scratchTargets = environments.filter { it.state == ConnectionState.Connected && it.scratchWorkspaceRoot != null }
    val scratchEnvironment = scratchTargets.firstOrNull { it.id == draft.environmentId } ?: scratchTargets.firstOrNull()

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
                buildProjectScopes(projects.filterNot { it.isScratch }, preferences.projectGrouping),
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
                workspaceMode = WorkspaceMode.CurrentCheckout,
                worktreePath = null,
            )
        }
        onProjectChosen()
    }

    fun startScratch() {
        val target = scratchEnvironment ?: return
        if (startingScratch) return
        coroutineScope.launch {
            startingScratch = true
            try { choose(store.workspace.ensureScratchProject(target.id)) }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) { store.showError(error.message ?: "Could not start without a project.") }
            finally { startingScratch = false }
        }
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
        if (scopes.isEmpty() && scratchEnvironment == null) {
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
                if (scratchEnvironment != null) item {
                    S5SelectableRow(label = "No project", supporting = scratchEnvironment.label,
                        selected = false, onClick = { startScratch() },
                        trailing = if (startingScratch) ({ S5InlineLoading() }) else null,
                        position = rowPosition(0, 1))
                }
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
    val context = androidx.compose.ui.platform.LocalContext.current
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

    val project = remember(projects, draft) { projects.firstOrNull { it.environmentId == draft.environmentId && it.id.value == draft.projectKey } }
    LaunchedEffect(project?.isScratch) {
        if (project?.isScratch == true) store.updateDraft { it.copy(workspaceMode = WorkspaceMode.CurrentCheckout, branch = "", worktreePath = null) }
    }
    val environment = remember(environments, draft) { environments.firstOrNull { it.id == draft.environmentId } }
    // RN's composerWorkspaceCwd: the worktree when one is chosen, else the
    // project's root — `@`, `$`, `/`, and `#` all resolve against it.
    val composerWorkspaceCwd =
        remember(project, draft) {
            // In worktree mode the worktree does not exist yet, so the project
            // root stands in — same as RN's composerWorkspaceCwd.
            when {
                draft.workspaceMode == WorkspaceMode.NewWorktree -> project?.workspaceRoot
                else -> draft.worktreePath ?: project?.workspaceRoot
            }?.takeIf { it.isNotEmpty() }
        }
    val composerSkills by
        store.workspace
            .providerSkills(draft.environmentId, draft.settings.provider, composerWorkspaceCwd)
            .collectAsStateWithLifecycle()
    val pullRequestRepository =
        project?.repositoryIdentity?.displayName
            ?.takeIf { environment?.capabilities?.pullRequests == true }
    // Same fold as the thread composer: a large paste becomes a file upload
    // where the environment takes them at all.
    val pastedTextMaxBytes =
        environment?.capabilities
            ?.takeIf { it.attachmentUploads }
            ?.fileAttachments?.maxUploadBytes
            ?.let { clampFileAttachmentUploadBytes(it) }
    // Switching away is only a choice while a second enabled environment exists.
    val enabledEnvironmentCount = remember(environments) { environments.count { it.isEnabled } }
    // T3 owns /usage-limits only where Limits has data; a new task would send
    // it to the agent, so the prompt is refused instead — as RN does.
    val offersUsageLimits by
        store.workspace
            .usageLimitsOffered(draft.environmentId, draft.settings.provider.driver)
            .collectAsStateWithLifecycle()

    // A project added by cloning exists before its files do: the prompt can be
    // written meanwhile, but Start waits for the clone. Null means the stream
    // has not delivered — "pending" in RN's useProjectClone — which only gates
    // when this draft was opened by the clone flow itself.
    val projectClones by
        remember(draft.environmentId) { store.workspace.projectClones(draft.environmentId) }
            .collectAsStateWithLifecycle()
    val projectClone =
        remember(projectClones, draft.projectKey) {
            projectClones?.firstOrNull { it.projectId == draft.projectKey }
        }
    val awaitingKnownClone =
        projectClones == null && draft.cloning &&
            project != null && project.id.value == draft.projectKey
    val cloneBlocksStart =
        awaitingKnownClone || (projectClone != null && projectClone.phase != "done")
    val canStart =
        draft.prompt.isNotBlank() && !creating &&
            environment?.isEnabled != false && !cloneBlocksStart

    val start: () -> Unit = start@{
        if (
            canStart &&
            offersUsageLimits &&
            isUsageLimitsCommand(draft.prompt) &&
            draft.attachments.isEmpty()
        ) {
            store.showError(
                "Send /usage-limits inside a thread, or open Settings → Usage → Limits.",
            )
            return@start
        }
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
            val visibleClone = projectClone?.takeIf { it.phase != "done" }
            val cloneProject = project
            NewTaskComposerDock(
                promptState = promptState,
                dictation = rememberDictation("new-task", promptState),
                attachments = draft.attachments,
                onRemoveAttachment = { attachment -> store.removeNewTaskDraftImage(attachment.id) },
                onPreviewAttachment = { previewAttachment = it },
                onAddImages = addImages,
                onPickImages = pickImages,
                canAttachPastedText = {
                    pastedTextMaxBytes != null &&
                        draft.attachments.size < ComposerAttachmentLimits.MAX_ATTACHMENTS
                },
                onPastedText = onPastedText@{ text ->
                    val maxBytes = pastedTextMaxBytes
                    if (maxBytes == null) {
                        store.showError("This server does not support file attachments.")
                        return@onPastedText
                    }
                    val bytes = text.toByteArray(Charsets.UTF_8).size.toLong()
                    if (bytes > maxBytes) {
                        store.showError(fileAttachmentTooLargeMessage("pasted-text.txt", maxBytes))
                        return@onPastedText
                    }
                    scope.launch {
                        val attachment =
                            persistPastedTextAttachment(
                                context.cacheDir,
                                text,
                                draft.attachments.map { it.name },
                            )
                        if (attachment == null) {
                            store.showError("Could not attach pasted text.")
                        } else {
                            store.addNewTaskDraftAttachment(attachment)
                        }
                    }
                },
                workspaceMode = draft.workspaceMode,
                allowWorkspaceSelection = project?.isScratch != true,
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
                commands =
                    remember(draft.settings.provider, composerWorkspaceCwd) {
                        store.workspace.slashCommands(
                            draft.settings.provider, composerWorkspaceCwd,
                        )
                    },
                skills = composerSkills,
                interactionModeAllowed =
                    preferences.planModeEnabled &&
                        machineCatalog
                            .firstOrNull {
                                it.instance.instanceId == draft.settings.provider.instanceId
                            }
                            ?.interactionModeToggle != false,
                onInteractionMode = { mode ->
                    store.updateDraft {
                        it.copy(settings = it.settings.copy(runtimeMode = mode))
                    }
                },
                onSearchPaths = { query ->
                    val cwd = composerWorkspaceCwd ?: return@NewTaskComposerDock emptyList()
                    store.workspace.searchPathsIn(draft.environmentId, cwd, query)
                },
                onSearchPullRequests =
                    pullRequestRepository?.let { repository ->
                        { query: String ->
                            store.workspace.searchComposerPullRequests(
                                draft.environmentId,
                                ProjectId(draft.projectKey),
                                repository,
                                query,
                            )
                        }
                    },
                onPickPullRequest = onPickPr@{ pullRequest, rangeStart, rangeEnd ->
                    // COMPOSER_CONTEXT_MAX_RECORDS — a draft carries at most
                    // this many context payloads.
                    if (draft.contextRecords.size >= 200) {
                        store.showError(
                            "Too many context items. Remove some context from the draft and try again.",
                        )
                        return@onPickPr
                    }
                    val record = pullRequestComposerContext(pullRequest)
                    val nextText =
                        replaceComposerTextRange(
                            draft.prompt,
                            rangeStart,
                            rangeEnd,
                            formatComposerContextReference(record) + " ",
                        )
                    store.updateDraft {
                        it.copy(
                            prompt = nextText,
                            contextRecords = it.contextRecords + record,
                        )
                    }
                },
                onRefreshWorkspaceSnapshot = {
                    store.workspace.refreshProviderWorkspaceSnapshot(
                        draft.environmentId, draft.settings.provider, composerWorkspaceCwd,
                    )
                },
                belowSuggestions = {
                    // Above the workspace controls so they keep their place
                    // relative to the composer when the banner goes away once
                    // the clone lands.
                    if (visibleClone != null && cloneProject != null) {
                        ProjectCloneBanner(
                            clone = visibleClone,
                            onCancel = {
                                scope.launch {
                                    runCatching {
                                            store.workspace.projectCloneAction(
                                                draft.environmentId,
                                                cloneProject.id,
                                                retry = false,
                                            )
                                        }
                                        .onFailure {
                                            store.showError(
                                                it.message ?: "Failed to cancel clone"
                                            )
                                        }
                                }
                            },
                            onRetry = {
                                scope.launch {
                                    runCatching {
                                            store.workspace.projectCloneAction(
                                                draft.environmentId,
                                                cloneProject.id,
                                                retry = true,
                                            )
                                        }
                                        .onFailure {
                                            store.showError(
                                                it.message ?: "Failed to retry clone"
                                            )
                                        }
                                }
                            },
                            onRemove = {
                                scope.launch {
                                    runCatching {
                                            store.workspace.removeProject(
                                                draft.environmentId, cloneProject.id,
                                            )
                                        }
                                        .onSuccess {
                                            // The draft's project is gone; leave
                                            // like RN's replace("Home").
                                            onBack()
                                        }
                                        .onFailure {
                                            store.showError(
                                                it.message ?: "Failed to remove project"
                                            )
                                        }
                                }
                            },
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                },
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
    /** `pastedTextDisposition` gate: may a large paste fold into a file chip. */
    canAttachPastedText: () -> Boolean,
    /** Receives clipboard text that was folded out of the field. */
    onPastedText: (String) -> Unit,
    workspaceMode: WorkspaceMode,
    allowWorkspaceSelection: Boolean,
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
    /** `/` rows for the draft's cwd — the provider's list plus T3's own. */
    commands: List<SlashCommand> = emptyList(),
    /** `providerSkills` for the draft's cwd — `$` trigger plus `skill:` rows. */
    skills: List<ProviderSkill> = emptyList(),
    interactionModeAllowed: Boolean = false,
    onInteractionMode: (RuntimeMode) -> Unit = {},
    /** `@` path search against the project's workspace. */
    onSearchPaths: suspend (String) -> List<String> = { emptyList() },
    /** `#` mention search; null leaves the trigger as plain text. */
    onSearchPullRequests:
        (suspend (String) -> List<ComposerPullRequestCandidate>)? = null,
    /**
     * The pick writes the mention link into the draft and appends its context
     * record; the ranges index the current prompt text.
     */
    onPickPullRequest: (ComposerPullRequestCandidate, Int, Int) -> Unit = { _, _, _ -> },
    /** Backfills the per-cwd provider snapshot when the picker opens empty. */
    onRefreshWorkspaceSnapshot: suspend () -> Unit = {},
    /** Renders between the suggestion list and the workspace controls — the clone banner's slot on RN. */
    belowSuggestions: @Composable () -> Unit = {},
) {
    // `detectComposerTrigger` on the live field text, same as the thread
    // composer: the run of non-whitespace at the caret classifies the token.
    val promptText = promptState.text.toString()
    val trigger = remember(promptText) { detectComposerTrigger(promptText) }
    val atMessageStart = trigger?.rangeStart == 0

    val commandSuggestions =
        remember(trigger, commands, skills, interactionModeAllowed) {
            if (trigger?.kind != ComposerTriggerKind.SlashCommand) {
                return@remember emptyList<ComposerSuggestion>()
            }
            val query = trigger.query.lowercase()
            buildList<ComposerSuggestion> {
                if ("model".contains(query)) {
                    add(ComposerSuggestion("/model", "Switch model", null, "/model "))
                }
                if (interactionModeAllowed) {
                    if ("plan".contains(query)) {
                        add(
                            ComposerSuggestion(
                                "/plan", "Switch to plan mode", null, "",
                                interactionMode = RuntimeMode.Plan,
                            )
                        )
                    }
                    if ("default".contains(query)) {
                        add(
                            ComposerSuggestion(
                                "/default", "Switch to default mode", null, "",
                                interactionMode = RuntimeMode.Default,
                            )
                        )
                    }
                }
                if (atMessageStart) {
                    val skillNames = skills.mapTo(HashSet()) { it.name.trim().lowercase() }
                    val visible =
                        commands.filter {
                            it.name.removePrefix("/").lowercase() !in skillNames
                        }
                    rankSlashCommands(visible, trigger.query).forEach { command ->
                        add(
                            ComposerSuggestion(
                                command.name, command.description,
                                Icons.Rounded.Terminal, "${command.name} ",
                            )
                        )
                    }
                }
                val skillQuery =
                    when {
                        query == "skill" -> ""
                        query.startsWith("skill:") -> query.removePrefix("skill:")
                        else -> query
                    }
                skills
                    .filter { skill ->
                        skill.enabled &&
                            (
                                skillQuery.isEmpty() ||
                                    listOfNotNull(
                                        skill.name, skill.displayName,
                                        skill.shortDescription, skill.description,
                                    ).any { it.lowercase().contains(skillQuery) }
                            )
                    }
                    .take(20)
                    .forEach { skill ->
                        add(
                            ComposerSuggestion(
                                "skill:${skill.name}",
                                skill.shortDescription ?: skill.description.orEmpty(),
                                Icons.Rounded.AutoAwesome,
                                "\$${skill.name} ",
                            )
                        )
                    }
            }
        }

    val skillSuggestions =
        remember(trigger, skills) {
            if (trigger?.kind != ComposerTriggerKind.Skill) {
                return@remember emptyList<ComposerSuggestion>()
            }
            val query =
                trigger.query.trim()
                    .replace(Regex("""^[\p{Sc}]+"""), "")
                    .lowercase()
            if (query.isEmpty()) {
                return@remember skills.take(20).map { it.suggestion() }
            }
            skills
                .mapNotNull { skill -> rankProviderSkill(skill, query) }
                .sortedWith(compareBy({ it.score }, { it.tieBreaker }))
                .take(20)
                .map { it.skill.suggestion() }
        }

    var pathSuggestions by remember { mutableStateOf(emptyList<String>()) }
    var pullRequestSuggestions by
        remember { mutableStateOf(emptyList<ComposerPullRequestCandidate>()) }
    LaunchedEffect(trigger) {
        if (trigger?.kind != ComposerTriggerKind.Path) {
            pathSuggestions = emptyList()
            return@LaunchedEffect
        }
        delay(160)
        pathSuggestions =
            runCatching { onSearchPaths(trigger.query) }
                .map { rankComposerPaths(it, trigger.query) }
                .getOrDefault(emptyList())
    }
    LaunchedEffect(trigger) {
        val search = onSearchPullRequests
        if (trigger?.kind != ComposerTriggerKind.PullRequest || search == null) {
            pullRequestSuggestions = emptyList()
            return@LaunchedEffect
        }
        delay(180)
        pullRequestSuggestions =
            runCatching { search(trigger.query) }.getOrDefault(emptyList())
    }
    // First visit usually precedes the per-cwd snapshot; the gateway's own
    // cooldown covers a failure.
    LaunchedEffect(Unit) { onRefreshWorkspaceSnapshot() }

    Column(
        Modifier.fillMaxWidth()
            .imePadding()
            .navigationBarsPadding()
            .padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.small),
        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
    ) {
        // Above the workspace controls so a pick does not move the row the
        // user is aiming at — same popover the thread composer renders.
        SuggestionPopover(
            commands = commandSuggestions + skillSuggestions,
            paths = pathSuggestions,
            pullRequests = pullRequestSuggestions,
            groupLabel =
                when (trigger?.kind) {
                    ComposerTriggerKind.PullRequest -> "Pull requests"
                    ComposerTriggerKind.Skill -> "Skills"
                    ComposerTriggerKind.Path -> "Files"
                    else -> "Commands"
                },
            onPick = onPickRow@{ suggestion ->
                val range = trigger ?: return@onPickRow
                if (suggestion.interactionMode != null) {
                    promptState.edit { replace(range.rangeStart, range.rangeEnd, "") }
                    onInteractionMode(suggestion.interactionMode)
                } else {
                    promptState.edit {
                        replace(range.rangeStart, range.rangeEnd, suggestion.replacement)
                    }
                }
            },
            onPickPullRequest = onPick@{ pullRequest ->
                val range = trigger ?: return@onPick
                if (range.kind != ComposerTriggerKind.PullRequest) return@onPick
                onPickPullRequest(pullRequest, range.rangeStart, range.rangeEnd)
            },
        )

        belowSuggestions()

        if (allowWorkspaceSelection) S5ComposerToolbarRow {
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
                            .composerPastedTextReceiver(
                                shouldFold = { text ->
                                    pastedTextShouldFold(text, canAttachPastedText())
                                },
                                onFolded = onPastedText,
                            )
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
    val repositoryKey = selected?.repositoryIdentity?.groupingCanonicalKey
    val workspaceBasename = selected?.workspaceRoot?.split('/')?.lastOrNull()?.takeIf { it.isNotEmpty() }
    fun knownMismatch(project: club.touchtech.s5code.kotlin.model.Project): Boolean {
        val key = project.repositoryIdentity?.groupingCanonicalKey ?: return false
        return repositoryKey != null && key != repositoryKey
    }
    return (repositoryKey?.let { key ->
            projectsOnTarget.firstOrNull { it.repositoryIdentity?.groupingCanonicalKey == key }
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
    val coroutineScope = rememberCoroutineScope()
    var switching by remember { mutableStateOf(false) }
    val selectedProject = projects.firstOrNull { it.environmentId == draft.environmentId && it.id.value == draft.projectKey }
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
                val creatable = !switching && (selectedProject?.isScratch != true || environment.scratchWorkspaceRoot != null) &&
                    environment.state ==
                        club.touchtech.s5code.kotlin.model.ConnectionState.Connected
                S5SelectableRow(
                    label = environment.label,
                    supporting = "${environment.host} · ${health.label}",
                    selected = environment.id == draft.environmentId,
                    onClick = {
                        if (creatable) {
                            if (selectedProject?.isScratch == true) {
                                coroutineScope.launch {
                                    switching = true
                                    try {
                                        val match = store.workspace.ensureScratchProject(environment.id)
                                        store.updateDraft { it.copy(environmentId = environment.id, projectKey = match.id.value,
                                            branch = "", workspaceMode = WorkspaceMode.CurrentCheckout, worktreePath = null) }
                                        onBack()
                                    } catch (cancelled: CancellationException) { throw cancelled }
                                    catch (error: Exception) { store.showError(error.message ?: "Could not change environment.") }
                                    finally { switching = false }
                                }
                            } else {
                            // RN's selectEnvironment follows the repo to the
                            // target machine rather than leaving the draft
                            // pointing at a project the environment lacks.
                            val current =
                                selectedProject
                            val match =
                                environmentProjectMatch(
                                    projects.filter { it.environmentId == environment.id && !it.isScratch },
                                    current,
                                )
                            store.updateDraft {
                                it.copy(
                                    environmentId = environment.id,
                                    projectKey = match?.id?.value.orEmpty(),
                                    branch = match?.branch.orEmpty(),
                                    worktreePath = null,
                                )
                            }
                            onBack()
                            }
                        }
                    },
                    leading = { Icon(health.icon, contentDescription = null) },
                    position = rowPosition(environments.indexOf(environment), environments.size),
                )
            }
        }
    }
}

/**
 * `filterNewTaskBranches`: a typed ref name searches every ref the server
 * listed — remote refs included — and the query itself is sanitized like
 * a branch name, so "my feature" finds `my-feature`.
 */
fun filterNewTaskBranches(branches: List<BranchRef>, rawQuery: String): List<BranchRef> {
    val query = rawQuery.trim().replace(Regex("[\\s]+"), "-").lowercase()
    return if (query.isEmpty()) branches else branches.filter { it.name.lowercase().contains(query) }
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
    var query by remember { mutableStateOf("") }
    val branches = remote.valueOrNull.orEmpty()
    val filtered = remember(branches, query) { filterNewTaskBranches(branches, query) }

    S5Screen(
        title = "Base branch",
        subtitle = if (remote is Remote.Loaded) "${branches.size} branches" else "",
        onBack = onBack,
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            S5SearchField(
                value = query,
                onValueChange = { query = it },
                placeholder = "Search branches",
                modifier = Modifier.padding(horizontal = S5Theme.spacing.gutter),
            )
            when (remote) {
                is Remote.Loading -> S5LoadingState("Listing branches…")
                is Remote.Failed ->
                    Box(Modifier.padding(S5Theme.spacing.gutter)) {
                        S5ErrorState(title = "Couldn't list branches", detail = remote.message, onRetry = retry)
                    }
                is Remote.Loaded ->
                    LazyColumn(
                        Modifier.fillMaxSize(),
                        contentPadding =
                            PaddingValues(
                                horizontal = S5Theme.spacing.gutter,
                                vertical = S5Theme.spacing.small,
                            ),
                        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
                    ) {
                        itemsIndexed(filtered, key = { _, branch -> branch.name }) { index, branch ->
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
                                position = rowPosition(index, filtered.size),
                            )
                        }
                    }
            }
        }
    }
}

/**
 * `ProjectCloneBanner`: live state of the clone backing a freshly added
 * project, above the composer while the draft waits for its files. Running
 * clones offer Cancel; failed or cancelled ones offer Retry and Remove.
 */
@Composable
private fun ProjectCloneBanner(
    clone: club.touchtech.s5code.kotlin.model.ProjectClone,
    onCancel: () -> Unit,
    onRetry: () -> Unit,
    onRemove: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val name = projectCloneDisplayName(clone)
    if (clone.phase == "running") {
        Surface(
            modifier,
            shape = MaterialTheme.shapes.large,
            color = MaterialTheme.colorScheme.surfaceContainerLow,
            border =
                androidx.compose.foundation.BorderStroke(
                    1.dp,
                    MaterialTheme.colorScheme.outlineVariant,
                ),
        ) {
            Row(
                Modifier.padding(S5Theme.spacing.small),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                Column(Modifier.weight(1f)) {
                    Text(
                        "Cloning $name",
                        style = MaterialTheme.typography.bodyMedium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        projectCloneProgressSummary(clone),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                TextButton(onClick = onCancel) { Text("Cancel") }
            }
        }
        return
    }
    val cancelled = clone.phase == "cancelled"
    Surface(
        modifier,
        shape = MaterialTheme.shapes.large,
        color =
            if (cancelled) MaterialTheme.colorScheme.tertiaryContainer
            else MaterialTheme.colorScheme.errorContainer,
        border =
            androidx.compose.foundation.BorderStroke(
                1.dp,
                if (cancelled) MaterialTheme.colorScheme.outlineVariant
                else MaterialTheme.colorScheme.error,
            ),
    ) {
        Column(Modifier.padding(S5Theme.spacing.small)) {
            Text(
                if (cancelled) "Cancelled cloning $name" else "Failed to clone $name",
                style = MaterialTheme.typography.bodyMedium,
                color =
                    if (cancelled) MaterialTheme.colorScheme.onTertiaryContainer
                    else MaterialTheme.colorScheme.onErrorContainer,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            clone.error?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.labelSmall,
                    color =
                        if (cancelled) MaterialTheme.colorScheme.onTertiaryContainer
                        else MaterialTheme.colorScheme.onErrorContainer,
                    maxLines = 3,
                )
            }
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.End,
            ) {
                TextButton(onClick = onRemove) { Text("Remove project") }
                TextButton(onClick = onRetry) { Text("Retry") }
            }
        }
    }
}
