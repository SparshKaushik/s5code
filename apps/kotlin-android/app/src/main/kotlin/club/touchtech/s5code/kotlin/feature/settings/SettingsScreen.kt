package club.touchtech.s5code.kotlin.feature.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.AddComment
import androidx.compose.material.icons.automirrored.rounded.ReplyAll
import androidx.compose.material.icons.rounded.AccountTree
import androidx.compose.material.icons.rounded.Analytics
import androidx.compose.material.icons.rounded.Archive
import androidx.compose.material.icons.rounded.BugReport
import androidx.compose.material.icons.rounded.Build
import androidx.compose.material.icons.rounded.Forum
import androidx.compose.material.icons.rounded.Hub
import androidx.compose.material.icons.rounded.Key
import androidx.compose.material.icons.rounded.Notifications
import androidx.compose.material.icons.rounded.Palette
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material.icons.rounded.Psychology
import androidx.compose.material.icons.rounded.Schedule
import androidx.compose.material.icons.rounded.Storage
import androidx.compose.material.icons.rounded.Workspaces
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.app.Routes
import club.touchtech.s5code.kotlin.cloud.CloudAccountState
import club.touchtech.s5code.kotlin.design.component.S5RowGroup
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SettingsRow
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.rowPosition
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.ConnectionState

/**
 * Settings root, matching RN's `SettingsRouteScreen`: a navigation hub of
 * grouped rows. The actual controls live on the sub-screens — thread behavior,
 * notifications, and the server-settings pages — reached from here.
 */
@Composable
fun SettingsScreen(store: AppStore, onBack: () -> Unit, onOpen: (String) -> Unit) {
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val account by store.cloud.state.collectAsStateWithLifecycle()
    val hasConnected = environments.any { it.state == ConnectionState.Connected }

    S5Screen(
        title = "Settings",
        prominence = S5TopBarProminence.Hero,
        onBack = onBack,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            S5RowGroup(title = "Account") {
                val accountLabel =
                    when (val current = account) {
                        CloudAccountState.Unconfigured -> "Not available"
                        CloudAccountState.Loading -> "Checking"
                        is CloudAccountState.SignedIn -> current.label
                        is CloudAccountState.SignedOut -> "Sign in"
                    }
                S5SettingsRow(
                    icon = Icons.Rounded.Person,
                    label = "S5 account",
                    value = accountLabel,
                    onClick = { onOpen(Routes.SettingsAccount) },
                    position = rowPosition(0, 2),
                )
                S5SettingsRow(
                    icon = Icons.Rounded.Key,
                    label = "Provider accounts",
                    onClick = { onOpen("settings/provider-accounts") },
                    position = rowPosition(1, 2),
                )
            }
            Text(
                "S5 Code works locally without signing in. Cloud features are optional.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = S5Theme.spacing.gutter),
            )

            S5RowGroup(title = "App") {
                val rows = 6
                var position = 0
                S5SettingsRow(
                    icon = Icons.Rounded.Hub,
                    label = "Environments",
                    value = "${environments.size}",
                    onClick = { onOpen(Routes.SettingsEnvironments) },
                    position = rowPosition(position++, rows),
                )
                S5SettingsRow(
                    icon = Icons.Rounded.Forum,
                    label = "Thread behavior",
                    onClick = { onOpen(Routes.SettingsThreads) },
                    position = rowPosition(position++, rows),
                )
                S5SettingsRow(
                    icon = Icons.AutoMirrored.Rounded.ReplyAll,
                    label = "Follow-ups",
                    onClick = { onOpen(Routes.SettingsFollowUps) },
                    position = rowPosition(position++, rows),
                )
                S5SettingsRow(
                    icon = Icons.Rounded.Notifications,
                    label = "Notifications",
                    onClick = { onOpen(Routes.SettingsNotifications) },
                    position = rowPosition(position++, rows),
                )
                S5SettingsRow(
                    icon = Icons.Rounded.Schedule,
                    label = "Scheduled tasks",
                    onClick = { onOpen("settings/scheduled-tasks") },
                    position = rowPosition(position++, rows),
                )
                S5SettingsRow(
                    icon = Icons.Rounded.Palette,
                    label = "Appearance",
                    onClick = { onOpen(Routes.SettingsAppearance) },
                    position = rowPosition(position++, rows),
                )
            }

            S5RowGroup(title = "App settings") {
                val rows = if (hasConnected) 4 else 3
                var position = 0
                S5SettingsRow(
                    icon = Icons.Rounded.Workspaces,
                    label = "Organization",
                    onClick = { onOpen(Routes.SettingsProjectGrouping) },
                    position = rowPosition(position++, rows),
                )
                // RN shows Overview only once a project group exists to show it
                // for; a device with no connected environment has no checkouts.
                if (hasConnected) {
                    S5SettingsRow(
                        icon = Icons.Rounded.Workspaces,
                        label = "Overview",
                        onClick = { onOpen(Routes.SettingsOverview) },
                        position = rowPosition(position++, rows),
                    )
                }
                S5SettingsRow(
                    icon = Icons.Rounded.Analytics,
                    label = "Usage limits",
                    onClick = { onOpen(Routes.Usage) },
                    position = rowPosition(position++, rows),
                )
                S5SettingsRow(
                    icon = Icons.Rounded.Archive,
                    label = "Archived threads",
                    onClick = { onOpen(Routes.Archive) },
                    position = rowPosition(position++, rows),
                )
            }

            // RN's Server settings section exists only once a server can
            // answer `server.updateSettings`; the four pages share that gate.
            if (hasConnected) {
                S5RowGroup(title = "Server settings") {
                    val rows = 4
                    var position = 0
                    S5SettingsRow(
                        icon = Icons.AutoMirrored.Rounded.AddComment,
                        label = "New threads",
                        onClick = { onOpen(Routes.SettingsNewThreads) },
                        position = rowPosition(position++, rows),
                    )
                    S5SettingsRow(
                        icon = Icons.Rounded.AccountTree,
                        label = "Source control",
                        onClick = { onOpen(Routes.SettingsSourceControl) },
                        position = rowPosition(position++, rows),
                    )
                    S5SettingsRow(
                        icon = Icons.Rounded.Psychology,
                        label = "Agent behavior",
                        onClick = { onOpen(Routes.SettingsAgentBehavior) },
                        position = rowPosition(position++, rows),
                    )
                    S5SettingsRow(
                        icon = Icons.Rounded.Build,
                        label = "Maintenance",
                        onClick = { onOpen(Routes.SettingsMaintenance) },
                        position = rowPosition(position++, rows),
                    )
                }
            }

            S5RowGroup(title = "About T3 Code") {
                S5SettingsRow(
                    icon = Icons.Rounded.Storage,
                    label = "Client storage",
                    onClick = { onOpen(Routes.SettingsClientStorage) },
                    position = rowPosition(0, 2),
                )
                S5SettingsRow(
                    icon = Icons.Rounded.BugReport,
                    label = "Diagnostics",
                    onClick = { onOpen(Routes.SettingsDiagnostics) },
                    position = rowPosition(1, 2),
                )
            }
            Box(Modifier.padding(bottom = S5Theme.spacing.section))
        }
    }
}
