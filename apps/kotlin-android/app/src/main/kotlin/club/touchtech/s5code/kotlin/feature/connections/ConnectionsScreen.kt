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
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
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
import club.touchtech.s5code.kotlin.design.component.S5HeroFab
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5RowGroup
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SettingsRow
import club.touchtech.s5code.kotlin.design.component.S5ShapeBadge
import club.touchtech.s5code.kotlin.design.component.S5StatusPill
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
    S5Screen(
        title = "Connections",
        subtitle = "${environments.size} environments",
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
        floatingActionButton = {
            S5HeroFab(icon = Icons.Rounded.Add, label = "Add environment", onClick = onAdd)
        },
    ) { padding ->
        if (environments.isEmpty()) {
            S5EmptyState(
                icon = Icons.Rounded.Hub,
                title = "No environments",
                detail = "Pair with a machine running S5 Code to control agents from this device.",
                actionLabel = "Add environment",
                onAction = onAdd,
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
                items(environments, key = { it.id.value }) { environment ->
                    EnvironmentCard(environment, onClick = { onOpen(environment.id.value) })
                }
            }
        }
    }
}

@Composable
private fun EnvironmentCard(environment: Environment, onClick: () -> Unit) {
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
                icon =
                    if (environment.kind == EnvironmentKind.Cloud) Icons.Rounded.Cloud
                    else Icons.Rounded.Computer,
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
                    Text(
                        "seen ${environment.lastSeenLabel}",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            Icon(health.icon, contentDescription = null, modifier = Modifier.size(20.dp))
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
                S5Notice(icon = health.icon, text = "${health.label} · seen ${environment.lastSeenLabel}")
            }

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

            S5RowGroup(title = "Server") {
                S5SettingsRow(
                    icon = Icons.Rounded.Hub,
                    label = "Version",
                    value = environment.serverVersion,
                    onClick = {},
                    position = rowPosition(0, 2),
                )
                S5SettingsRow(
                    icon =
                        if (environment.kind == EnvironmentKind.Cloud) Icons.Rounded.Cloud
                        else Icons.Rounded.Computer,
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
                S5Button(
                    text = "Remove",
                    onClick = {
                        confirmController.show(
                            S5ConfirmDialogRequest(
                                title = "Remove environment?",
                                message =
                                    "This removes ${environment.label} and its credential from this device. The server is not deleted.",
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
