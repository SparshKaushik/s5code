package club.touchtech.s5code.kotlin.feature.newtask

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.CallSplit
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.rounded.Logout
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Computer
import androidx.compose.material.icons.rounded.CreateNewFolder
import androidx.compose.material.icons.rounded.Folder
import androidx.compose.material.icons.rounded.FolderOpen
import androidx.compose.material.icons.rounded.Link
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.Remote
import club.touchtech.s5code.kotlin.data.appendBrowsePathSegment
import club.touchtech.s5code.kotlin.data.defaultCloneUrl
import club.touchtech.s5code.kotlin.data.ensureBrowseDirectoryPath
import club.touchtech.s5code.kotlin.data.filterFilesystemBrowseEntries
import club.touchtech.s5code.kotlin.data.getCloneDestinationBrowsePath
import club.touchtech.s5code.kotlin.data.getCloneDestinationPath
import club.touchtech.s5code.kotlin.data.getCloneDirectoryName
import club.touchtech.s5code.kotlin.data.getFilesystemBrowsePath
import club.touchtech.s5code.kotlin.data.inferProjectTitleFromPath
import club.touchtech.s5code.kotlin.data.isWindowsPlatform
import club.touchtech.s5code.kotlin.data.normalizePastedCloneUrl
import club.touchtech.s5code.kotlin.data.normalizeProjectPathForComparison
import club.touchtech.s5code.kotlin.data.rememberRemote
import club.touchtech.s5code.kotlin.data.resolveAddProjectPath
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5RowGroup
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SectionHeader
import club.touchtech.s5code.kotlin.design.component.S5SelectableRow
import club.touchtech.s5code.kotlin.design.component.S5TextField
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.rowPosition
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.feature.connections.connectionPresentation
import club.touchtech.s5code.kotlin.feature.connections.environmentIcon
import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.model.Environment
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.transport.wire.SourceControlDiscoveryResultDto
import kotlinx.coroutines.launch

/* ── Shared flow pieces ──────────────────────────────────────────────── */

/** `AddProjectRemoteSource`: the clone sources the RN source screen lists. */
private val PROVIDER_SOURCES = listOf("github", "gitlab", "forgejo", "bitbucket", "azure-devops")

private fun sourceLabel(source: String): String =
    when (source) {
        "github" -> "GitHub"
        "gitlab" -> "GitLab"
        "forgejo" -> "Forgejo / Gitea"
        "bitbucket" -> "Bitbucket"
        "azure-devops" -> "Azure DevOps"
        else -> "Git URL"
    }

private fun sourcePathHint(source: String): String =
    when (source) {
        "github", "forgejo" -> "owner/repo"
        "gitlab" -> "group/project"
        "bitbucket" -> "workspace/repository"
        "azure-devops" -> "project/repository"
        else -> "URL"
    }

/** `buildAddProjectRemoteSourceReadiness`: provider availability from discovery. */
private fun providerReadiness(
    discovery: SourceControlDiscoveryResultDto?,
): Map<String, Pair<Boolean, String?>> {
    val unavailable =
        "Provider status unavailable. Open Source Control settings and rescan."
    val readiness = mutableMapOf<String, Pair<Boolean, String?>>()
    for (source in PROVIDER_SOURCES) {
        val provider = discovery?.sourceControlProviders?.firstOrNull { it.kind == source }
        readiness[source] =
            when {
                discovery == null || provider == null -> false to unavailable
                provider.status != "available" -> false to provider.installHint
                provider.auth.status == "unauthenticated" ->
                    false to
                        (provider.auth.detail
                            ?: "${provider.label} is not authenticated. Open Source Control settings for setup guidance.")
                else -> true to null
            }
    }
    return readiness
}

/** `sortAddProjectProviderSources`: ready providers first, then label order. */
private fun sortedProviderSources(readiness: Map<String, Pair<Boolean, String?>>): List<String> =
    PROVIDER_SOURCES.sortedWith(
        compareByDescending<String> { readiness[it]?.first == true }.thenBy { sourceLabel(it) }
    )

/** RN's `EnvironmentOption`: the environment plus what the flow needs from it. */
private data class EnvironmentOption(
    val environment: Environment,
    val creatable: Boolean,
    /** `connectionStatusText` for rows that cannot take a project. */
    val statusText: String,
)

private fun environmentOptions(environments: List<Environment>): List<EnvironmentOption> =
    environments
        .map { environment ->
            val creatable = environment.isEnabled &&
                environment.state == ConnectionState.Connected
            val statusText =
                when (environment.state) {
                    ConnectionState.Connecting -> "Connecting…"
                    ConnectionState.Recovering ->
                        if (environment.lastError.isNotBlank()) {
                            "Failed to connect. Reconnecting… Reason: ${environment.lastError}"
                        } else {
                            "Reconnecting…"
                        }
                    ConnectionState.Connected -> "Connected"
                    ConnectionState.Offline, ConnectionState.Disabled -> "Offline"
                    ConnectionState.AuthRequired ->
                        if (environment.lastError.isNotBlank()) {
                            "Connection failed. Reason: ${environment.lastError}"
                        } else {
                            "Connection failed"
                        }
                }
            EnvironmentOption(environment, creatable, statusText)
        }
        .sortedBy { it.environment.label }

/**
 * `resolveAddProjectEnvironment`: a requested environment only counts when it
 * can take a project; without a request the first creatable option wins.
 */
private fun resolveAddProjectEnvironment(
    options: List<EnvironmentOption>,
    requestedId: String?,
): EnvironmentOption? {
    if (requestedId != null) {
        return options.firstOrNull {
            it.environment.id.value == requestedId && it.creatable
        }
    }
    return options.firstOrNull { it.creatable }
}

@Composable
private fun environmentOption(option: EnvironmentOption, index: Int, count: Int,
    selected: Boolean, onSelect: () -> Unit) {
    S5SelectableRow(
        label = option.environment.label,
        supporting =
            if (option.creatable) option.environment.id.value else option.statusText,
        selected = selected,
        onClick = { if (option.creatable) onSelect() },
        leading = {
            Icon(environmentIcon(option.environment), contentDescription = null)
        },
        trailing =
            if (selected) {
                { Icon(Icons.Rounded.Check, contentDescription = null) }
            } else {
                null
            },
        position = rowPosition(index, count),
    )
}

/** `EmptyEnvironmentState`: no creatable environment, so point at pairing. */
@Composable
private fun EmptyEnvironmentState(onAddEnvironment: () -> Unit) {
    S5EmptyState(
        icon = Icons.Rounded.Computer,
        title = "Environment unavailable",
        detail = "Start or reconnect an environment before adding a project.",
        actionLabel = "Add environment",
        onAction = onAddEnvironment,
    )
}

/* ── Source chooser ──────────────────────────────────────────────────── */

/**
 * `AddProjectSourceScreen`: environment picker (once a second exists), then
 * "Local folder" plus the clone sources the machine's source-control discovery
 * reports as ready.
 */
@Composable
fun AddProjectSourceScreen(
    store: AppStore,
    onBack: () -> Unit,
    onRepository: (environmentId: String, source: String) -> Unit,
    onLocalPath: (environmentId: String) -> Unit,
    onAddEnvironment: () -> Unit,
) {
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val options = remember(environments) { environmentOptions(environments) }
    var requestedId by rememberSaveable { mutableStateOf<String?>(null) }
    val selected =
        options.firstOrNull { it.environment.id.value == requestedId && it.creatable }
            ?: options.firstOrNull { it.creatable }
    val discovery =
        rememberRemote(selected?.environment?.id?.value) {
            val id = selected?.environment?.id ?: return@rememberRemote null
            runCatching { store.workspace.discoverSourceControl(id) }.getOrNull()
        }
    val readiness = remember(discovery.value) { providerReadiness(discovery.value.valueOrNull) }

    S5Screen(
        title = "Add project",
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            if (selected == null) {
                EmptyEnvironmentState(onAddEnvironment)
            } else {
                if (options.size > 1) {
                    S5RowGroup(title = "Environments") {
                        options.forEachIndexed { index, option ->
                            environmentOption(
                                option = option,
                                index = index,
                                count = options.size,
                                selected =
                                    option.environment.id == selected.environment.id,
                                onSelect = { requestedId = option.environment.id.value },
                            )
                        }
                    }
                }
                S5RowGroup {
                    val sources = listOf("url") + sortedProviderSources(readiness)
                    // The local row leads the list, then the clone sources.
                    S5SelectableRow(
                        label = "Local folder",
                        supporting = "Browse a folder on disk",
                        selected = false,
                        onClick = { onLocalPath(selected.environment.id.value) },
                        leading = {
                            Icon(Icons.Rounded.CreateNewFolder, contentDescription = null)
                        },
                        trailing = {
                            Icon(
                                Icons.AutoMirrored.Rounded.KeyboardArrowRight,
                                contentDescription = null,
                            )
                        },
                        position = rowPosition(0, sources.size + 1),
                    )
                    sources.forEachIndexed { index, source ->
                        val ready = source == "url" || readiness[source]?.first == true
                        val title =
                            if (source == "url") "Git URL" else "${sourceLabel(source)} repository"
                        val subtitle =
                            when {
                                source == "url" -> "Clone from a remote URL"
                                ready -> "Clone ${sourceLabel(source)} ${sourcePathHint(source)}"
                                else -> readiness[source]?.second.orEmpty()
                            }
                        S5SelectableRow(
                            label = title,
                            supporting = subtitle,
                            selected = false,
                            onClick = {
                                if (ready) {
                                    onRepository(selected.environment.id.value, source)
                                }
                            },
                            leading = {
                                Icon(
                                    if (source == "url") Icons.Rounded.Link
                                    else Icons.AutoMirrored.Rounded.CallSplit,
                                    contentDescription = null,
                                )
                            },
                            trailing =
                                if (ready) {
                                    {
                                        Icon(
                                            Icons.AutoMirrored.Rounded.KeyboardArrowRight,
                                            contentDescription = null,
                                        )
                                    }
                                } else {
                                    null
                                },
                            position = rowPosition(index + 1, sources.size + 1),
                        )
                    }
                }
                if (discovery.value is Remote.Loading) {
                    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                        S5InlineLoading()
                    }
                }
            }
            Box(Modifier.padding(bottom = S5Theme.spacing.section))
        }
    }
}

/* ── Repository / URL input ──────────────────────────────────────────── */

/**
 * `AddProjectRepositoryScreen`: one validating field. A provider row looks the
 * reference up on the machine; a raw URL skips the lookup and goes straight to
 * the destination step.
 */
@Composable
fun AddProjectRepositoryScreen(
    store: AppStore,
    environmentId: String,
    source: String,
    onBack: () -> Unit,
    onDestination: (
        environmentId: String,
        source: String,
        remoteUrl: String,
        repositoryTitle: String,
        repositoryName: String,
    ) -> Unit,
    onAddEnvironment: () -> Unit,
) {
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val options = remember(environments) { environmentOptions(environments) }
    val environment = resolveAddProjectEnvironment(options, environmentId)
    var input by rememberSaveable { mutableStateOf("") }
    var submitting by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    fun submit() {
        val env = environment ?: return
        val reference = input.trim()
        if (reference.isEmpty() || submitting) return
        if (source == "url") {
            val remoteUrl = normalizePastedCloneUrl(reference)
            onDestination(
                env.environment.id.value,
                source,
                remoteUrl,
                remoteUrl,
                getCloneDirectoryName(remoteUrl),
            )
            return
        }
        error = null
        submitting = true
        scope.launch {
            val outcome =
                runCatching {
                    store.workspace.lookupRepository(env.environment.id, source, reference)
                }
            submitting = false
            outcome.fold(
                onSuccess = { repository ->
                    onDestination(
                        env.environment.id.value,
                        source,
                        defaultCloneUrl(repository.provider, repository.url, repository.sshUrl),
                        repository.nameWithOwner,
                        getCloneDirectoryName(repository.nameWithOwner),
                    )
                },
                onFailure = { cause ->
                    error = cause.message ?: "An error occurred."
                },
            )
        }
    }

    S5Screen(
        title = sourceLabel(source),
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
        loading = submitting,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            error?.let { message ->
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5Notice(
                        icon = Icons.Rounded.Link,
                        text = message,
                        containerColor = MaterialTheme.colorScheme.errorContainer,
                        contentColor = MaterialTheme.colorScheme.onErrorContainer,
                        onDismiss = { error = null },
                    )
                }
            }
            if (environment == null) {
                EmptyEnvironmentState(onAddEnvironment)
            } else {
                S5TextField(
                    value = input,
                    onValueChange = {
                        input = it
                        error = null
                    },
                    placeholder =
                        if (source == "url") "https://github.com/org/repo.git"
                        else sourcePathHint(source),
                    singleLine = true,
                    modifier =
                        Modifier.padding(
                            horizontal = S5Theme.spacing.gutter,
                            vertical = S5Theme.spacing.small,
                        ),
                )
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5Button(
                        text = if (source == "url") "Continue" else "Lookup repository",
                        onClick = ::submit,
                        emphasis = S5ActionEmphasis.Primary,
                        icon = Icons.AutoMirrored.Rounded.KeyboardArrowRight,
                        enabled = !submitting && input.isNotBlank(),
                    )
                }
            }
            Box(Modifier.padding(bottom = S5Theme.spacing.section))
        }
    }
}

/* ── Browse-path input ───────────────────────────────────────────────── */

/**
 * `useBrowsePathInput`: the path field and the folder list are one model — the
 * input seeds the browse directory, a committed row rewrites the input. The
 * commit only happens after the server lists the destination, so a dead folder
 * cannot swallow the path the user had.
 */
private class BrowsePathController(
    val isNavigating: Boolean,
    val pathInput: String,
    val setPathInput: (String) -> Unit,
    val navigateTo: (browseDirectoryPath: String, selectedDirectoryName: String?) -> Unit,
)

@Composable
private fun rememberBrowsePathController(
    environment: EnvironmentOption?,
    pinnedDirectoryName: String,
    browse: suspend (String) -> Unit,
): BrowsePathController {
    val baseDirectory = environment?.environment?.addProjectBaseDirectory.orEmpty()
    val caseSensitive = !isWindowsPlatform(environment?.environment?.platformOs.orEmpty())
    var pathInput by
        rememberSaveable(environment?.environment?.id?.value) {
            mutableStateOf(
                getCloneDestinationPath(
                    ensureBrowseDirectoryPath(baseDirectory.trim()).ifBlank { "~/" },
                    pinnedDirectoryName,
                )
            )
        }
    var navigating by remember { mutableStateOf(false) }
    var generation by remember { mutableStateOf(0) }
    val scope = rememberCoroutineScope()

    return BrowsePathController(
        isNavigating = navigating,
        pathInput = pathInput,
        setPathInput = { value ->
            generation += 1
            navigating = false
            pathInput = value
        },
        navigateTo = { browseDirectoryPath, selectedDirectoryName ->
            val selectedDirectoryPath =
                selectedDirectoryName?.let { appendBrowsePathSegment(browseDirectoryPath, it) }
                    ?: browseDirectoryPath
            val nextInput =
                if (pinnedDirectoryName.isNotEmpty() && selectedDirectoryName != null) {
                    getCloneDestinationBrowsePath(
                        browseDirectoryPath = browseDirectoryPath,
                        selectedDirectoryName = selectedDirectoryName,
                        cloneDirectoryName = pinnedDirectoryName,
                        caseSensitive = caseSensitive,
                    )
                } else {
                    getCloneDestinationPath(selectedDirectoryPath, pinnedDirectoryName)
                }
            val myGeneration = ++generation
            navigating = true
            scope.launch {
                runCatching { browse(selectedDirectoryPath) }
                if (generation == myGeneration) {
                    pathInput = nextInput
                    navigating = false
                }
            }
        },
    )
}

/** `FolderBrowser`: the typed path drives the listing; picking a row commits. */
@Composable
private fun FolderBrowser(
    store: AppStore,
    environment: EnvironmentOption,
    pathInput: String,
    controller: BrowsePathController,
    pinnedDirectoryName: String,
) {
    val platform = environment.environment.platformOs.orEmpty()
    val browsePath = remember(pathInput, platform) { getFilesystemBrowsePath(pathInput, platform) }
    val listing =
        rememberRemote(environment.environment.id.value, browsePath.directoryPath) {
            if (browsePath.directoryPath.isEmpty()) emptyList()
            else store.workspace.browseFilesystem(environment.environment.id, browsePath.directoryPath)
        }
    // A pinned repository folder does not exist yet, so filtering the listing by
    // it would empty the folder picker. Anything else the user typed filters.
    val pinnedMatches =
        if (isWindowsPlatform(platform)) {
            browsePath.filterQuery.equals(pinnedDirectoryName, ignoreCase = true)
        } else {
            browsePath.filterQuery == pinnedDirectoryName
        }
    val filterQuery = if (pinnedMatches) "" else browsePath.filterQuery
    val entries =
        listing.value.valueOrNull?.let { filterFilesystemBrowseEntries(it, filterQuery) }
            .orEmpty()

    S5SectionHeader("Browse folders")
    (listing.value as? Remote.Failed)?.let { failed ->
        Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
            S5Notice(
                icon = Icons.Rounded.FolderOpen,
                text = failed.message,
                containerColor = MaterialTheme.colorScheme.errorContainer,
                contentColor = MaterialTheme.colorScheme.onErrorContainer,
            )
        }
    }
    S5RowGroup {
        val rowCount = entries.size + if (browsePath.canBrowseUp) 1 else 0
        if (listing.value is Remote.Loading && listing.value.valueOrNull == null) {
            Box(
                Modifier.fillMaxWidth().padding(S5Theme.spacing.medium),
                contentAlignment = Alignment.Center,
            ) {
                S5InlineLoading()
            }
        }
        if (browsePath.canBrowseUp) {
            S5SelectableRow(
                label = "..",
                selected = false,
                onClick = {
                    browsePath.parentPath?.let { controller.navigateTo(it, null) }
                },
                leading = {
                    Icon(
                        Icons.AutoMirrored.Rounded.Logout,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                },
                position = rowPosition(0, maxOf(rowCount, 1)),
            )
        }
        entries.forEachIndexed { index, entry ->
            S5SelectableRow(
                label = entry.name,
                selected = false,
                onClick = {
                    controller.navigateTo(browsePath.directoryPath, entry.name)
                },
                leading = {
                    Icon(
                        Icons.Rounded.Folder,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                },
                position =
                    rowPosition(
                        index + if (browsePath.canBrowseUp) 1 else 0,
                        maxOf(rowCount, 1),
                    ),
            )
        }
    }
}

/* ── Local folder ────────────────────────────────────────────────────── */

/**
 * `AddProjectLocalFolderScreen`: a path field, an Add button, and a folder
 * browser for the same directory. Creating lands on the new-task draft bound
 * to the new project, as RN's reset to NewTaskDraft does.
 */
@Composable
fun AddProjectLocalFolderScreen(
    store: AppStore,
    environmentId: String,
    onBack: () -> Unit,
    onCreated: (environmentId: String, projectId: String) -> Unit,
    onAddEnvironment: () -> Unit,
) {
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val options = remember(environments) { environmentOptions(environments) }
    val environment = resolveAddProjectEnvironment(options, environmentId)
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    var submitting by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var existingProjectTitle by remember { mutableStateOf<String?>(null) }
    var existingProjectTarget by remember { mutableStateOf<Pair<String, String>?>(null) }
    val controller =
        rememberBrowsePathController(environment, pinnedDirectoryName = "") { path ->
            environment?.let { store.workspace.browseFilesystem(it.environment.id, path) } ?: Unit
        }
    val pathInput = controller.pathInput

    fun submit() {
        val env = environment ?: return
        if (controller.isNavigating || submitting) return
        error = null
        val (path, pathError) =
            resolveAddProjectPath(pathInput, env.environment.platformOs.orEmpty())
        if (path == null) {
            error = pathError
            return
        }
        // `findExistingAddProject`: a path this environment already tracks is a
        // jump to it, not a second project.
        val existing =
            env.environment.let { target ->
                projects.firstOrNull {
                    it.environmentId == target.id &&
                        normalizeProjectPathForComparison(it.workspaceRoot) ==
                            normalizeProjectPathForComparison(path)
                }
            }
        if (existing != null) {
            existingProjectTitle = existing.title
            existingProjectTarget = env.environment.id.value to existing.id.value
            return
        }
        submitting = true
        scope.launch {
            val outcome =
                runCatching {
                    store.workspace.createProject(env.environment.id, path)
                }
            submitting = false
            outcome.fold(
                onSuccess = { projectId ->
                    onCreated(env.environment.id.value, projectId.value)
                },
                onFailure = { cause -> error = cause.message ?: "An error occurred." },
            )
        }
    }

    S5Screen(
        title = "Add project",
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
        loading = submitting,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            error?.let { message ->
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5Notice(
                        icon = Icons.Rounded.FolderOpen,
                        text = message,
                        containerColor = MaterialTheme.colorScheme.errorContainer,
                        contentColor = MaterialTheme.colorScheme.onErrorContainer,
                        onDismiss = { error = null },
                    )
                }
            }
            if (environment == null) {
                EmptyEnvironmentState(onAddEnvironment)
            } else {
                S5TextField(
                    value = pathInput,
                    onValueChange = controller.setPathInput,
                    placeholder = "~/projects/my-app",
                    singleLine = true,
                    modifier =
                        Modifier.padding(
                            horizontal = S5Theme.spacing.gutter,
                            vertical = S5Theme.spacing.small,
                        ),
                )
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5Button(
                        text = "Add project",
                        onClick = ::submit,
                        emphasis = S5ActionEmphasis.Primary,
                        icon = Icons.Rounded.CreateNewFolder,
                        enabled = !controller.isNavigating && !submitting,
                    )
                }
                FolderBrowser(
                    store = store,
                    environment = environment,
                    pathInput = pathInput,
                    controller = controller,
                    pinnedDirectoryName = "",
                )
            }
            Box(Modifier.padding(bottom = S5Theme.spacing.section))
        }
    }

    existingProjectTitle?.let { title ->
        AlertDialog(
            onDismissRequest = {
                existingProjectTitle = null
                existingProjectTarget?.let { (envId, projectId) -> onCreated(envId, projectId) }
            },
            title = { Text("Project already exists") },
            text = { Text(title) },
            confirmButton = {
                TextButton(
                    onClick = {
                        existingProjectTitle = null
                        existingProjectTarget?.let { (envId, projectId) ->
                            onCreated(envId, projectId)
                        }
                    }
                ) {
                    Text("OK")
                }
            },
        )
    }
}

/* ── Clone destination ───────────────────────────────────────────────── */

/**
 * `AddProjectDestinationScreen`: the looked-up repository card, the clone
 * destination field with the folder browser underneath, and the clone button.
 */
@Composable
fun AddProjectDestinationScreen(
    store: AppStore,
    environmentId: String,
    remoteUrl: String?,
    repositoryTitle: String?,
    repositoryName: String,
    onBack: () -> Unit,
    onCreated: (environmentId: String, projectId: String) -> Unit,
    onAddEnvironment: () -> Unit,
) {
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val options = remember(environments) { environmentOptions(environments) }
    val environment = resolveAddProjectEnvironment(options, environmentId)
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    var submitting by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var existingProjectTitle by remember { mutableStateOf<String?>(null) }
    var existingProjectTarget by remember { mutableStateOf<Pair<String, String>?>(null) }
    val pinnedName = repositoryName.trim()
    val controller =
        rememberBrowsePathController(environment, pinnedDirectoryName = pinnedName) { path ->
            environment?.let { store.workspace.browseFilesystem(it.environment.id, path) } ?: Unit
        }
    val pathInput = controller.pathInput

    fun submit() {
        val env = environment ?: return
        val url = remoteUrl ?: return
        if (controller.isNavigating || submitting) return
        error = null
        val (path, pathError) =
            resolveAddProjectPath(pathInput, env.environment.platformOs.orEmpty())
        if (path == null) {
            error = pathError
            return
        }
        submitting = true
        scope.launch {
            val outcome =
                runCatching {
                    val (projectId, cwd) =
                        store.workspace.cloneProject(env.environment.id, url, path)
                    // RN's createProject checks for an existing project at the
                    // clone's landing directory, which is not always the path
                    // that was asked for.
                    val existing =
                        projects.firstOrNull {
                            it.environmentId == env.environment.id &&
                                normalizeProjectPathForComparison(it.workspaceRoot) ==
                                    normalizeProjectPathForComparison(cwd)
                        }
                    projectId to existing
                }
            submitting = false
            outcome.fold(
                onSuccess = { (projectId, existing) ->
                    if (existing != null) {
                        existingProjectTitle = existing.title
                        existingProjectTarget = env.environment.id.value to existing.id.value
                    } else {
                        onCreated(env.environment.id.value, projectId.value)
                    }
                },
                onFailure = { cause -> error = cause.message ?: "An error occurred." },
            )
        }
    }

    S5Screen(
        title = "Add project",
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
        loading = submitting,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            error?.let { message ->
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5Notice(
                        icon = Icons.Rounded.Link,
                        text = message,
                        containerColor = MaterialTheme.colorScheme.errorContainer,
                        contentColor = MaterialTheme.colorScheme.onErrorContainer,
                        onDismiss = { error = null },
                    )
                }
            }
            repositoryTitle?.let { title ->
                S5Card(tone = S5CardTone.Standard, modifier = Modifier.fillMaxWidth()
                    .padding(horizontal = S5Theme.spacing.gutter)) {
                    Column(Modifier.padding(S5Theme.spacing.large)) {
                        Text(title, style = MaterialTheme.typography.titleSmallEmphasized)
                        Text(
                            remoteUrl.orEmpty(),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
            if (environment == null) {
                EmptyEnvironmentState(onAddEnvironment)
            } else {
                S5TextField(
                    value = pathInput,
                    onValueChange = controller.setPathInput,
                    placeholder = "~/projects/my-app",
                    singleLine = true,
                    modifier =
                        Modifier.padding(
                            horizontal = S5Theme.spacing.gutter,
                            vertical = S5Theme.spacing.small,
                        ),
                )
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5Button(
                        text = "Clone project",
                        onClick = ::submit,
                        emphasis = S5ActionEmphasis.Primary,
                        icon = Icons.Rounded.CreateNewFolder,
                        enabled =
                            !controller.isNavigating && !submitting && remoteUrl != null,
                    )
                }
                FolderBrowser(
                    store = store,
                    environment = environment,
                    pathInput = pathInput,
                    controller = controller,
                    pinnedDirectoryName = pinnedName,
                )
            }
            Box(Modifier.padding(bottom = S5Theme.spacing.section))
        }
    }

    existingProjectTitle?.let { title ->
        AlertDialog(
            onDismissRequest = {
                existingProjectTitle = null
                existingProjectTarget?.let { (envId, projectId) -> onCreated(envId, projectId) }
            },
            title = { Text("Project already exists") },
            text = { Text(title) },
            confirmButton = {
                TextButton(
                    onClick = {
                        existingProjectTitle = null
                        existingProjectTarget?.let { (envId, projectId) ->
                            onCreated(envId, projectId)
                        }
                    }
                ) {
                    Text("OK")
                }
            },
        )
    }
}
