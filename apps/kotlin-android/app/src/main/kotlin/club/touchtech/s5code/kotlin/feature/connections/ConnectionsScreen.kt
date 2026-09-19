package club.touchtech.s5code.kotlin.feature.connections

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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Cloud
import androidx.compose.material.icons.rounded.Computer
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.Hub
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogRequest
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5ErrorState
import club.touchtech.s5code.kotlin.design.component.S5HeroFab
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5RowGroup
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SectionHeader
import club.touchtech.s5code.kotlin.design.component.S5SettingsRow
import club.touchtech.s5code.kotlin.design.component.S5ShapeBadge
import club.touchtech.s5code.kotlin.design.component.S5StatusPill
import club.touchtech.s5code.kotlin.design.component.S5SwitchRow
import club.touchtech.s5code.kotlin.design.component.S5TextField
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.rowPosition
import club.touchtech.s5code.kotlin.design.theme.S5MaterialShapes
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.Environment
import club.touchtech.s5code.kotlin.model.EnvironmentKind
import kotlinx.coroutines.launch

/** Environment list: status, add, reconnect, open detail. */
@Composable
fun ConnectionsScreen(
    store: AppStore,
    onBack: () -> Unit,
    onAdd: () -> Unit,
    onOpen: (String) -> Unit,
) {
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val account by store.cloud.state.collectAsStateWithLifecycle()
    val cloudEnvironments = store.cloudEnvironments
    val cloudState = cloudEnvironments?.state?.collectAsStateWithLifecycle()?.value
    val linking = cloudEnvironments?.linking?.collectAsStateWithLifecycle()?.value
    val linkError = cloudEnvironments?.linkError?.collectAsStateWithLifecycle()?.value

    // Relay environments available to this account but not yet added, matching
    // RN's T3 Connect section on the same screen. A signed-out or unconfigured
    // account contributes only its connected rows, like RN's
    // ConnectedOnlyCloudEnvironmentRows.
    val signedIn = account is club.touchtech.s5code.kotlin.cloud.CloudAccountState.SignedIn
    val discoveryAvailable = signedIn && cloudEnvironments != null
    val availableCloudRows =
        remember(cloudState, environments, discoveryAvailable) {
            if (!discoveryAvailable) return@remember emptyList()
            val rows =
                (cloudState as? club.touchtech.s5code.kotlin.cloud.CloudEnvironmentsState.Loaded)
                    ?.rows
                    .orEmpty()
            rows.filter { row -> environments.none { it.id.value == row.environmentId } }
        }
    // One load per visit; a signed-out account leaves the state untouched.
    LaunchedEffect(account) {
        if (signedIn) cloudEnvironments?.refresh()
    }

    val cloudEnvs = remember(environments) { environments.filter { it.kind == EnvironmentKind.Cloud } }
    val localEnvs = remember(environments) { environments.filter { it.kind != EnvironmentKind.Cloud } }
    val cloudLoading = cloudState is club.touchtech.s5code.kotlin.cloud.CloudEnvironmentsState.Loading ||
        cloudState is club.touchtech.s5code.kotlin.cloud.CloudEnvironmentsState.Idle
    val cloudError =
        (cloudState as? club.touchtech.s5code.kotlin.cloud.CloudEnvironmentsState.Failed)?.message
    // RN renders the section whenever discovery could return rows — header,
    // refresh affordance, and a body that says what it found.
    val showConnectSection = discoveryAvailable || cloudEnvs.isNotEmpty()

    S5Screen(
        title = "Environments",
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
        floatingActionButton = {
            S5HeroFab(icon = Icons.Rounded.Add, label = "Add environment", onClick = onAdd)
        },
    ) { padding ->
        if (environments.isEmpty() && !showConnectSection) {
            S5EmptyState(
                icon = Icons.Rounded.Hub,
                title = "No environments connected yet",
                detail = "Tap + to add one.",
                modifier = Modifier.padding(padding),
            )
        } else {
            LazyColumn(
                Modifier.fillMaxSize().padding(padding),
                contentPadding =
                    PaddingValues(
                        start = S5Theme.spacing.gutter,
                        end = S5Theme.spacing.gutter,
                        bottom = 96.dp,
                    ),
                verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                items(localEnvs, key = { it.id.value }) { environment ->
                    EnvironmentCard(
                        environment,
                        onClick = { onOpen(environment.id.value) },
                        onToggle = { enabled ->
                            store.setEnvironmentEnabled(environment.id, enabled)
                        },
                    )
                }
                if (showConnectSection) {
                    item(key = "connect-header") {
                        S5SectionHeader(
                            label = "S5 Connect",
                            modifier = Modifier.padding(top = S5Theme.spacing.small),
                            trailing =
                                if (discoveryAvailable) {
                                    {
                                        S5IconButton(
                                            icon = Icons.Rounded.Refresh,
                                            label = "Refresh",
                                            onClick = { cloudEnvironments?.refresh() },
                                        )
                                    }
                                } else {
                                    null
                                },
                        )
                    }
                    items(cloudEnvs, key = { it.id.value }) { environment ->
                        EnvironmentCard(
                            environment,
                            onClick = { onOpen(environment.id.value) },
                            onToggle = { enabled ->
                                store.setEnvironmentEnabled(environment.id, enabled)
                            },
                        )
                    }
                    items(availableCloudRows, key = { "cloud-${it.environmentId}" }) { row ->
                        AvailableCloudRow(
                            row = row,
                            busy = linking == row.environmentId,
                            onAdd = {
                                cloudEnvironments?.link(row.environmentId) {}
                            },
                        )
                    }
                    // A failed link attempt reports next to the rows, the way
                    // RN surfaces connectRelayEnvironment errors.
                    if (linkError != null) {
                        item(key = "connect-link-error") {
                            S5Notice(
                                icon = Icons.Rounded.Cloud,
                                text = linkError,
                                containerColor = MaterialTheme.colorScheme.errorContainer,
                                contentColor = MaterialTheme.colorScheme.onErrorContainer,
                                onDismiss = { cloudEnvironments?.clearLinkError() },
                            )
                        }
                    }
                    if (cloudEnvs.isEmpty() && availableCloudRows.isEmpty()) {
                        when {
                            cloudLoading ->
                                item(key = "connect-loading") {
                                    S5Notice(
                                        icon = Icons.Rounded.Cloud,
                                        text = "Loading linked cloud environments.",
                                    )
                                }
                            discoveryAvailable && cloudError == null ->
                                item(key = "connect-empty") {
                                    S5Notice(
                                        icon = Icons.Rounded.Cloud,
                                        text = "No additional linked cloud environments.",
                                    )
                                }
                        }
                    }
                    // A failed discovery reports itself alongside any rows,
                    // as RN's error card does — it must not hide behind a
                    // healthy-looking list.
                    if (discoveryAvailable && cloudError != null) {
                        item(key = "connect-error") {
                            S5ErrorState(
                                title = "Could not load S5 Connect environments",
                                detail = cloudError,
                                onRetry = { cloudEnvironments?.refresh() },
                                retryLabel = "Try again",
                            )
                        }
                    }
                }
            }
        }
    }
}

/** A relay environment the account can reach but this device has not paired yet. */
@Composable
private fun AvailableCloudRow(
    row: club.touchtech.s5code.kotlin.cloud.CloudEnvironmentRow,
    busy: Boolean,
    onAdd: () -> Unit,
) {
    S5Card(
        tone = if (row.online == false) S5CardTone.Receded else S5CardTone.Standard,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(
            Modifier.padding(S5Theme.spacing.large),
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            S5ShapeBadge(
                icon = Icons.Rounded.Cloud,
                contentDescription = null,
                shape = S5MaterialShapes.avatar(),
                containerColor = MaterialTheme.colorScheme.secondaryContainer,
                contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                size = 44.dp,
                iconSize = 22.dp,
            )
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                Text(row.label, style = MaterialTheme.typography.titleMediumEmphasized)
                Text(
                    // availableCloudEnvironmentPresentation in the RN client:
                    // error text when the probe failed, the relay's own error
                    // when offline, "Available · …" otherwise.
                    when {
                        row.online == false -> row.statusError ?: "Relay is offline."
                        row.statusError != null -> row.statusError
                        row.online == true -> "Available · Relay online"
                        else -> "Available · Checking relay status…"
                    },
                    style = MaterialTheme.typography.labelSmall,
                    color =
                        if (row.online == false) MaterialTheme.colorScheme.error
                        else MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            // RN's available row toggles on to link; the row leaves the
            // section once linked, so the switch never reads "on" here.
            Switch(checked = false, onCheckedChange = { if (it) onAdd() }, enabled = !busy)
        }
    }
}

@Composable
private fun EnvironmentCard(
    environment: Environment,
    onClick: () -> Unit,
    onToggle: (Boolean) -> Unit,
) {
    val health = connectionPresentation(environment.state)
    S5Card(
        tone = if (health.offline) S5CardTone.Receded else S5CardTone.Standard,
        onClick = onClick,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(
            Modifier.padding(S5Theme.spacing.large),
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            S5ShapeBadge(
                icon = environmentIcon(environment),
                contentDescription = null,
                shape = S5MaterialShapes.avatar(),
                containerColor = health.container,
                contentColor = health.content,
                size = 44.dp,
                iconSize = 22.dp,
            )
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                Text(environment.label, style = MaterialTheme.typography.titleMediumEmphasized)
                Text(
                    environment.host,
                    style = S5Theme.code.inlineTechnical,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                    S5StatusPill(
                        label = health.label,
                        containerColor = health.container,
                        contentColor = health.content,
                    )
                    // The RN row shows the connection error under the status
                    // pill when there is one, in danger color; the detail
                    // screen keeps the same text selectable.
                    if (environment.isEnabled && environment.lastError.isNotBlank()) {
                        androidx.compose.foundation.text.selection.SelectionContainer {
                            Text(
                                environment.lastError,
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.error,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                }
            }
            // The switch, not the health icon, trails the row: it is the control
            // the RN client offers on the same row, and the pill already says the
            // state.
            Switch(
                checked = environment.isEnabled,
                onCheckedChange = onToggle,
                modifier = Modifier.padding(start = S5Theme.spacing.small),
            )
        }
    }
}

/** Per-environment detail: devices, server identity, reconnect, remove. */
@Composable
fun ConnectionDetailScreen(
    store: AppStore,
    environmentId: String,
    onBack: () -> Unit,
    onRemoved: () -> Unit,
    confirmController: S5ConfirmDialogController,
) {
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val environment = remember(environments, environmentId) {
        environments.firstOrNull { it.id.value == environmentId }
    }
    val scope = rememberCoroutineScope()

    if (environment == null) {
        S5Screen(title = "Environment", onBack = onBack) { padding ->
            S5EmptyState(
                icon = Icons.Rounded.Hub,
                title = "Environment removed",
                detail = "This environment is no longer paired with this device.",
                actionLabel = "Back to connections",
                onAction = onBack,
                modifier = Modifier.padding(padding),
            )
        }
        return
    }

    val health = connectionPresentation(environment.state)
    S5Screen(
        title = environment.label,
        subtitle = environment.host,
        prominence = S5TopBarProminence.Hero,
        onBack = onBack,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                S5Notice(
                    icon = health.icon,
                    text =
                        when {
                            !environment.isEnabled -> health.label
                            environment.lastError.isNotBlank() ->
                                "${health.label} · ${environment.lastError}"
                            else -> health.label
                        },
                )
            }

            // RN's expanded row edits label and URL for direct environments;
            // a relay-managed row is read-only because the tunnel owns the
            // endpoint.
            if (environment.kind == EnvironmentKind.Cloud) {
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    Text(
                        "Managed by S5 Connect. Tunnel details update automatically.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                var editLabel by
                    remember(environment.id) { mutableStateOf(environment.label) }
                var editUrl by
                    remember(environment.id) { mutableStateOf(environment.baseUrl) }
                S5RowGroup(title = "Edit environment") {
                    Column(
                        Modifier.padding(
                            start = S5Theme.spacing.gutter,
                            end = S5Theme.spacing.gutter,
                            bottom = S5Theme.spacing.small,
                        ),
                        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                    ) {
                        S5TextField(
                            value = editLabel,
                            onValueChange = { editLabel = it },
                            label = "Label",
                            placeholder = "My MacBook",
                            singleLine = true,
                        )
                        S5TextField(
                            value = editUrl,
                            onValueChange = { editUrl = it },
                            label = "URL",
                            placeholder = "192.168.1.100:8080",
                            singleLine = true,
                        )
                        Row(
                            Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.End,
                        ) {
                            S5Button(
                                text = "Save",
                                onClick = {
                                    store.updateEnvironment(
                                        environment.id,
                                        editLabel.trim(),
                                        editUrl.trim(),
                                    )
                                },
                                emphasis = S5ActionEmphasis.Primary,
                            )
                        }
                    }
                }
            }

            S5RowGroup(title = "This device") {
                S5SwitchRow(
                    icon = health.icon,
                    label = "Connected",
                    supporting =
                        if (environment.isEnabled) {
                            "This environment syncs with this device."
                        } else {
                            "Off keeps ${environment.label} saved but disconnects it."
                        },
                    checked = environment.isEnabled,
                    onCheckedChange = { enabled ->
                        store.setEnvironmentEnabled(environment.id, enabled)
                    },
                )
            }

            // No server publishes a device list yet, so this section stays
            // empty in practice; an empty header is not worth a row.
            if (environment.devices.isNotEmpty()) {
                S5RowGroup(title = "Devices") {
                    environment.devices.forEachIndexed { index, device ->
                        S5SettingsRow(
                            icon = Icons.Rounded.Computer,
                            label = device.name,
                            supporting = "${device.platform} · ${device.lastSeenLabel}",
                            value = if (device.reachable) "Reachable" else "Unreachable",
                            onClick = {},
                            position = rowPosition(index, environment.devices.size),
                        )
                    }
                }
            }

            S5RowGroup(title = "Server") {
                S5SettingsRow(
                    icon = Icons.Rounded.Hub,
                    label = "Version",
                    value = environment.serverVersion,
                    onClick = {},
                    position = rowPosition(0, 2),
                )
                S5SettingsRow(
                    icon = environmentIcon(environment),
                    label = "Connection",
                    value = if (environment.kind == EnvironmentKind.Cloud) "S5 Connect" else "Direct",
                    onClick = {},
                    position = rowPosition(1, 2),
                )
            }

            Row(
                Modifier.fillMaxWidth().padding(S5Theme.spacing.gutter),
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                if (environment.isEnabled) {
                    S5Button(
                        text = "Reconnect",
                        // Reconnect has no pending state of its own: the health dot and
                        // the notice above already report the phase, and a third label
                        // saying "Reconnecting…" would disagree with them within a
                        // frame.
                        onClick = { store.retryEnvironment(environment.id) },
                        icon = Icons.Rounded.Refresh,
                        emphasis = S5ActionEmphasis.Primary,
                    )
                }
                S5Button(
                    text = "Remove",
                    onClick = {
                        confirmController.show(
                            S5ConfirmDialogRequest(
                                title = "Remove from this device?",
                                message =
                                    "Forget ${environment.label} and its cached threads on this device. Switch it off instead to keep it saved.",
                                confirmText = "Remove",
                                destructive = true,
                                onConfirm = {
                                    scope.launch {
                                        try {
                                            store.unpair(environment.id)
                                            onRemoved()
                                        } catch (error: Exception) {
                                            store.showError(
                                                error.message ?: "The environment could not be removed."
                                            )
                                        }
                                    }
                                },
                            )
                        )
                    },
                    icon = Icons.Rounded.Delete,
                    emphasis = S5ActionEmphasis.Primary,
                    style = S5ButtonStyle.Outlined,
                )
            }
        }
    }
}
