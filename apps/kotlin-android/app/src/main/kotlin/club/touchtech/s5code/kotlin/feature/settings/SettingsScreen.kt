package club.touchtech.s5code.kotlin.feature.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Analytics
import androidx.compose.material.icons.rounded.Archive
import androidx.compose.material.icons.rounded.Bolt
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.Hub
import androidx.compose.material.icons.rounded.Notifications
import androidx.compose.material.icons.rounded.Palette
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Storage
import androidx.compose.material.icons.rounded.SystemUpdate
import androidx.compose.material.icons.rounded.Workspaces
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.BuildConfig
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.app.Routes
import club.touchtech.s5code.kotlin.cloud.CloudAccountState
import club.touchtech.s5code.kotlin.data.cacheSizeLabel
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.component.S5RowGroup
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SettingsRow
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.rowPosition
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.platform.updates.AppUpdateStatus

/** Settings root. Every configuration destination plus build identity. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun SettingsScreen(store: AppStore, onBack: () -> Unit, onOpen: (String) -> Unit) {
    val preferences by store.preferences.collectAsStateWithLifecycle()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val account by store.cloud.state.collectAsStateWithLifecycle()
    val updateStatus by store.updates.status.collectAsStateWithLifecycle()
    val context = LocalContext.current

    S5Screen(
        title = "Settings",
        prominence = S5TopBarProminence.Hero,
        onBack = onBack,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            S5RowGroup(title = "Account and connections") {
                val accountLabel =
                    when (val current = account) {
                        CloudAccountState.Unconfigured -> "Not available"
                        CloudAccountState.Loading -> ""
                        is CloudAccountState.SignedIn -> current.label
                        is CloudAccountState.SignedOut -> "Not signed in"
                    }
                S5SettingsRow(
                    icon = Icons.Rounded.Person,
                    label = "S5 account",
                    value = accountLabel,
                    onClick = { onOpen(Routes.SettingsAccount) },
                    position = rowPosition(0, 2),
                )
                S5SettingsRow(
                    icon = Icons.Rounded.Hub,
                    label = "Environments",
                    value = "${environments.size} paired",
                    onClick = { onOpen(Routes.SettingsEnvironments) },
                    position = rowPosition(1, 2),
                )
            }

            S5RowGroup(title = "Appearance and behavior") {
                S5SettingsRow(
                    icon = Icons.Rounded.Palette,
                    label = "Appearance",
                    value = preferences.themeMode.name,
                    onClick = { onOpen(Routes.SettingsAppearance) },
                    position = rowPosition(0, 2),
                )
                S5SettingsRow(
                    icon = Icons.Rounded.Workspaces,
                    label = "Project grouping",
                    value = preferences.projectGrouping.label,
                    onClick = { onOpen(Routes.SettingsProjectGrouping) },
                    position = rowPosition(1, 2),
                )
            }

            S5RowGroup(title = "Notifications") {
                S5SettingsRow(
                    icon = Icons.Rounded.Notifications,
                    label = "Notifications",
                    value = if (preferences.notifyApprovals) "On" else "Off",
                    onClick = { onOpen(Routes.SettingsNotifications) },
                    position = rowPosition(0, 2),
                )
                S5SettingsRow(
                    icon = Icons.Rounded.Bolt,
                    label = "Live Updates",
                    value = if (preferences.liveUpdatesEnabled) "On" else "Off",
                    onClick = { onOpen(Routes.SettingsLiveUpdates) },
                    position = rowPosition(1, 2),
                )
            }

            S5RowGroup(title = "Data") {
                S5SettingsRow(
                    icon = Icons.Rounded.Analytics,
                    label = "Usage",
                    value = null,
                    onClick = { onOpen(Routes.Usage) },
                    position = rowPosition(0, 3),
                )
                S5SettingsRow(
                    icon = Icons.Rounded.Archive,
                    label = "Archived threads",
                    value = null,
                    onClick = { onOpen(Routes.Archive) },
                    position = rowPosition(1, 3),
                )
                S5SettingsRow(
                    icon = Icons.Rounded.Storage,
                    label = "Client storage",
                    value = null,
                    onClick = { onOpen(Routes.SettingsClientStorage) },
                    position = rowPosition(2, 3),
                )
            }

            Box(Modifier.padding(S5Theme.spacing.gutter)) {
                S5Card(tone = S5CardTone.Receded, modifier = Modifier.fillMaxWidth()) {
                    Column(
                        Modifier.padding(S5Theme.spacing.large),
                        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                    ) {
                        Column {
                            Text("S5 Code for Android", style = MaterialTheme.typography.titleSmallEmphasized)
                            Text(
                                "${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE}) · club.touchtech.s5code.kotlin",
                                style = S5Theme.code.inlineTechnical,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }

                        when (val status = updateStatus) {
                            AppUpdateStatus.Idle -> {
                                S5Button(
                                    text = "Check for updates",
                                    icon = Icons.Rounded.Refresh,
                                    onClick = { store.updates.checkForUpdates(manual = true) },
                                    style = S5ButtonStyle.Text,
                                )
                            }

                            AppUpdateStatus.Checking -> {
                                Row(
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                                ) {
                                    S5InlineLoading(modifier = Modifier.size(16.dp))
                                    Text(
                                        "Checking GitHub releases…",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            }

                            is AppUpdateStatus.UpToDate -> {
                                Row(
                                    Modifier.fillMaxWidth(),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                ) {
                                    Text(
                                        "App is up to date",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = S5Theme.status.added,
                                    )
                                    S5Button(
                                        text = "Check again",
                                        icon = Icons.Rounded.Refresh,
                                        onClick = { store.updates.checkForUpdates(manual = true) },
                                        style = S5ButtonStyle.Text,
                                    )
                                }
                            }

                            is AppUpdateStatus.Available -> {
                                Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                                    Text(
                                        "Update available: ${status.release.title}",
                                        style = MaterialTheme.typography.labelLargeEmphasized,
                                        color = MaterialTheme.colorScheme.primary,
                                    )
                                    Text(
                                        "Size: ${cacheSizeLabel(status.release.apkSizeBytes)}",
                                        style = S5Theme.code.inlineTechnical,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                    if (!status.release.notes.isNullOrBlank()) {
                                        Text(
                                            status.release.notes.take(200).trimEnd() + if (status.release.notes.length > 200) "…" else "",
                                            style = MaterialTheme.typography.bodySmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                            maxLines = 3,
                                        )
                                    }
                                    S5Button(
                                        text = "Download update",
                                        icon = Icons.Rounded.Download,
                                        onClick = { store.updates.downloadUpdate(status.release) },
                                        style = S5ButtonStyle.Filled,
                                    )
                                }
                            }

                            is AppUpdateStatus.Downloading -> {
                                Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                                    Text(
                                        "Downloading ${status.release.title}… ${(status.progress * 100).toInt()}%",
                                        style = MaterialTheme.typography.labelMediumEmphasized,
                                    )
                                    LinearProgressIndicator(
                                        progress = { status.progress },
                                        modifier = Modifier.fillMaxWidth().clip(MaterialTheme.shapes.extraSmall),
                                    )
                                    Text(
                                        "${cacheSizeLabel(status.bytesDownloaded)} of ${cacheSizeLabel(status.totalBytes)}",
                                        style = S5Theme.code.inlineTechnical,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            }

                            is AppUpdateStatus.ReadyToInstall -> {
                                Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                                    Text(
                                        "Update downloaded and verified!",
                                        style = MaterialTheme.typography.labelLargeEmphasized,
                                        color = S5Theme.status.added,
                                    )
                                    Text(
                                        "Ready to install ${status.release.apkName}",
                                        style = S5Theme.code.inlineTechnical,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                    S5Button(
                                        text = "Install update",
                                        icon = Icons.Rounded.SystemUpdate,
                                        onClick = { store.updates.installUpdate(context, status.apkFile) },
                                        style = S5ButtonStyle.Filled,
                                    )
                                }
                            }

                            is AppUpdateStatus.Failed -> {
                                Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                                    Text(
                                        status.message,
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.error,
                                    )
                                    if (status.canRetry) {
                                        S5Button(
                                            text = "Retry check",
                                            icon = Icons.Rounded.Refresh,
                                            onClick = { store.updates.checkForUpdates(manual = true) },
                                            style = S5ButtonStyle.Text,
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Box(Modifier.padding(bottom = S5Theme.spacing.section))
        }
    }
}
