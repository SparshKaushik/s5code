package club.touchtech.s5code.kotlin.feature.newtask

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Cloud
import androidx.compose.material.icons.rounded.Computer
import androidx.compose.material.icons.rounded.CreateNewFolder
import androidx.compose.material.icons.rounded.Folder
import androidx.compose.material.icons.rounded.FolderOpen
import androidx.compose.material.icons.rounded.Lock
import androidx.compose.material.icons.rounded.Public
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.Remote
import club.touchtech.s5code.kotlin.data.rememberRemote
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5ErrorState
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5RowGroup
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SelectableRow
import club.touchtech.s5code.kotlin.design.component.S5SettingsRow
import club.touchtech.s5code.kotlin.design.component.S5TextField
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.rowPosition
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.EnvironmentKind
import kotlinx.coroutines.launch

/** Choose how the project gets added: clone a repo, or point at a local path. */
@Composable
fun AddProjectSourceScreen(
    onBack: () -> Unit,
    onRepository: () -> Unit,
    onLocalPath: () -> Unit,
) {
    S5Screen(
        title = "Add project",
        subtitle = "Clone a repository or use a folder that's already there",
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            S5RowGroup {
                S5SettingsRow(
                    icon = Icons.Rounded.Download,
                    label = "Clone a repository",
                    supporting = "Browse repositories you have access to on this machine",
                    onClick = onRepository,
                    position = rowPosition(0, 2),
                )
                S5SettingsRow(
                    icon = Icons.Rounded.FolderOpen,
                    label = "Use an existing folder",
                    supporting = "Point at a path on the machine's filesystem",
                    onClick = onLocalPath,
                    position = rowPosition(1, 2),
                )
            }
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                S5Notice(
                    icon = Icons.Rounded.Computer,
                    text = "Projects live on the machine, not on this device. Nothing is downloaded here.",
                )
            }
        }
    }
}

/**
 * Repository selection.
 *
 * There is no repository *search* RPC, only a lookup that validates one
 * reference, so this is a validating field rather than a browsable list. Saying
 * that in the supporting text is better than an empty list that looks broken.
 */
@Composable
fun AddProjectRepositoryScreen(store: AppStore, onBack: () -> Unit, onSelected: () -> Unit) {
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val draft by store.projectDraft.collectAsStateWithLifecycle()
    val environmentId = draft.environmentId.takeIf { it.value.isNotEmpty() } ?: environments.firstOrNull()?.id
    var query by rememberSaveable { mutableStateOf("") }
    // Debounced by the field's own edits rather than a timer: the read only runs
    // once a reference looks like `owner/name`.
    val reference = query.trim()
    val lookup =
        rememberRemote(environmentId?.value, reference) {
            if (environmentId == null || !reference.contains('/')) emptyList()
            else store.workspace.repositories(environmentId, reference)
        }
    val matches = lookup.value.valueOrNull.orEmpty()

    S5Screen(
        title = "Repository",
        subtitle = "Clone on the machine",
        onBack = onBack,
        loading = lookup.value is Remote.Loading,
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            S5TextField(
                value = query,
                onValueChange = { query = it },
                label = "Repository",
                placeholder = "owner/name",
                leadingIcon = Icons.Rounded.Search,
                singleLine = true,
                supporting =
                    when {
                        reference.isEmpty() -> "Enter owner/name. The machine's git credentials do the clone."
                        !reference.contains('/') -> "Needs an owner and a name, like touchtech/s5code."
                        lookup.value is Remote.Loading -> "Checking that repository…"
                        matches.isEmpty() -> "That repository wasn't found on this machine's account."
                        else -> "Found it."
                    },
                modifier =
                    Modifier.padding(
                        horizontal = S5Theme.spacing.gutter,
                        vertical = S5Theme.spacing.small,
                    ),
            )
            if (environments.isEmpty()) {
                S5EmptyState(
                    icon = Icons.Rounded.Computer,
                    title = "No environments",
                    detail = "Pair with a machine before adding projects to it.",
                )
            } else {
                LazyColumn(
                    Modifier.fillMaxSize(),
                    contentPadding =
                        PaddingValues(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.small),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
                ) {
                    items(matches, key = { it.fullName }) { repository ->
                        S5SelectableRow(
                            label = repository.fullName,
                            supporting = repository.description,
                            selected = draft.repository == repository.fullName,
                            onClick = {
                                store.updateProjectDraft {
                                    it.copy(
                                        environmentId = environmentId ?: it.environmentId,
                                        repository = repository.fullName,
                                    )
                                }
                                onSelected()
                            },
                            leading = {
                                Icon(
                                    if (repository.private) Icons.Rounded.Lock else Icons.Rounded.Public,
                                    contentDescription = if (repository.private) "Private" else "Public",
                                )
                            },
                        )
                    }
                }
            }
        }
    }
}

/** Destination: environment, parent path, project name, conflict validation. */
@Composable
fun AddProjectDestinationScreen(store: AppStore, onBack: () -> Unit, onCreated: () -> Unit) {
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val draft by store.projectDraft.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val repository = draft.repository
    val environmentId = draft.environmentId.takeIf { it.value.isNotEmpty() } ?: environments.firstOrNull()?.id
    var name by rememberSaveable(repository) {
        mutableStateOf(repository?.substringAfterLast('/').orEmpty())
    }
    var parent by rememberSaveable { mutableStateOf("~/code") }
    var cloning by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }

    val destination = "$parent/$name"
    val conflict = remember(projects, destination) { projects.any { it.workspaceRoot == destination } }
    val canClone = repository != null && environmentId != null && name.isNotBlank() && !conflict && !cloning

    S5Screen(
        title = "Destination",
        subtitle = "Where the clone lands",
        onBack = onBack,
        loading = cloning,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            S5RowGroup(title = "Environment") {
                environments.forEachIndexed { index, environment ->
                    S5SelectableRow(
                        label = environment.label,
                        supporting = environment.host,
                        selected = environment.id == environmentId,
                        onClick = {
                            store.updateProjectDraft { it.copy(environmentId = environment.id) }
                        },
                        leading = {
                            Icon(
                                if (environment.kind == EnvironmentKind.Cloud) Icons.Rounded.Cloud
                                else Icons.Rounded.Computer,
                                contentDescription = null,
                            )
                        },
                        position = rowPosition(index, environments.size),
                    )
                }
            }

            Column(
                Modifier.padding(horizontal = S5Theme.spacing.gutter),
                verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                S5TextField(
                    value = parent,
                    onValueChange = { parent = it },
                    label = "Parent folder",
                    singleLine = true,
                )
                S5TextField(
                    value = name,
                    onValueChange = { name = it },
                    label = "Folder name",
                    singleLine = true,
                    isError = conflict,
                    supporting =
                        if (conflict) "A project already uses $destination."
                        else "Will clone ${repository ?: "the repository"} into $destination",
                )
                failure?.let { message ->
                    S5ErrorState(
                        title = "Clone failed",
                        detail = message,
                        onRetry = { failure = null },
                        retryLabel = "Dismiss",
                    )
                }
                S5Button(
                    text = if (cloning) "Cloning…" else "Clone and add",
                    onClick = {
                        val target = environmentId ?: return@S5Button
                        val reference = repository ?: return@S5Button
                        cloning = true
                        failure = null
                        scope.launch {
                            val outcome =
                                runCatching { store.workspace.cloneProject(target, reference, destination) }
                            cloning = false
                            outcome.fold(
                                onSuccess = { onCreated() },
                                onFailure = { cause ->
                                    failure = cause.message ?: "The machine refused that clone."
                                    store.showError(failure!!)
                                },
                            )
                        }
                    },
                    emphasis = S5ActionEmphasis.Primary,
                    icon = Icons.Rounded.CreateNewFolder,
                    enabled = canClone,
                )
            }
            Box(Modifier.padding(bottom = S5Theme.spacing.section))
        }
    }
}

/** Browse or type a path on the machine, then register it as a project. */
@Composable
fun AddProjectLocalPathScreen(store: AppStore, onBack: () -> Unit, onCreated: () -> Unit) {
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val draft by store.projectDraft.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val environmentId = draft.environmentId.takeIf { it.value.isNotEmpty() } ?: environments.firstOrNull()?.id
    var path by rememberSaveable { mutableStateOf("~/") }
    var creating by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }
    // Browse follows what is typed, so the list is completion for the field rather
    // than a separate navigation model.
    val browse =
        rememberRemote(environmentId?.value, path) {
            if (environmentId == null) emptyList() else store.workspace.remotePaths(environmentId, path)
        }
    val candidates = browse.value.valueOrNull.orEmpty()

    S5Screen(
        title = "Folder",
        subtitle = "On the machine's filesystem",
        onBack = onBack,
        loading = creating || browse.value is Remote.Loading,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            if (environments.size > 1) {
                S5RowGroup(title = "Environment") {
                    environments.forEachIndexed { index, environment ->
                        S5SelectableRow(
                            label = environment.label,
                            supporting = environment.host,
                            selected = environment.id == environmentId,
                            onClick = {
                                store.updateProjectDraft { it.copy(environmentId = environment.id) }
                            },
                            position = rowPosition(index, environments.size),
                        )
                    }
                }
            }
            S5TextField(
                value = path,
                onValueChange = { path = it },
                label = "Path",
                singleLine = true,
                leadingIcon = Icons.Rounded.FolderOpen,
                modifier =
                    Modifier.padding(
                        horizontal = S5Theme.spacing.gutter,
                        vertical = S5Theme.spacing.small,
                    ),
            )
            LazyColumn(
                Modifier.weight(1f),
                contentPadding = PaddingValues(horizontal = S5Theme.spacing.gutter),
                verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
            ) {
                items(candidates, key = { it }) { candidate ->
                    S5SelectableRow(
                        label = candidate,
                        selected = candidate == path,
                        onClick = { path = candidate },
                        leading = { Icon(Icons.Rounded.Folder, contentDescription = null) },
                    )
                }
            }
            Box(Modifier.padding(S5Theme.spacing.gutter)) {
                S5Card(tone = S5CardTone.Receded, modifier = Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(S5Theme.spacing.large)) {
                        Text("Creates a project rooted at $path", style = S5Theme.code.code)
                        failure?.let { message ->
                            Box(Modifier.padding(top = S5Theme.spacing.medium)) {
                                S5ErrorState(
                                    title = "Couldn't add that folder",
                                    detail = message,
                                    onRetry = { failure = null },
                                    retryLabel = "Dismiss",
                                )
                            }
                        }
                        Box(Modifier.padding(top = S5Theme.spacing.medium)) {
                            S5Button(
                                text = if (creating) "Adding…" else "Create project",
                                onClick = {
                                    val target = environmentId ?: return@S5Button
                                    creating = true
                                    failure = null
                                    scope.launch {
                                        val outcome =
                                            runCatching {
                                                store.workspace.createProject(
                                                    environmentId = target,
                                                    title = path.trimEnd('/').substringAfterLast('/'),
                                                    workspaceRoot = path,
                                                )
                                            }
                                        creating = false
                                        outcome.fold(
                                            onSuccess = { onCreated() },
                                            onFailure = { cause ->
                                                failure = cause.message ?: "That path was refused."
                                                store.showError(failure!!)
                                            },
                                        )
                                    }
                                },
                                emphasis = S5ActionEmphasis.Primary,
                                icon = Icons.Rounded.CreateNewFolder,
                                enabled = path.isNotBlank() && environmentId != null && !creating,
                            )
                        }
                    }
                }
            }
            Box(Modifier.padding(bottom = 8.dp))
        }
    }
}
