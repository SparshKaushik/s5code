package club.touchtech.s5code.kotlin.feature.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ReplyAll
import androidx.compose.material.icons.rounded.AccountTree
import androidx.compose.material.icons.rounded.Analytics
import androidx.compose.material.icons.rounded.AddComment
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
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.app.Routes
import club.touchtech.s5code.kotlin.cloud.CloudAccountState
import club.touchtech.s5code.kotlin.design.component.S5RowGroup
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SearchField
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
fun SettingsScreen(
    store: AppStore,
    /** Null as a Home tab, where the screen is a root with the tab bar instead. */
    onBack: (() -> Unit)?,
    onOpen: (String) -> Unit,
    bottomBar: @Composable () -> Unit = {},
) {
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val account by store.cloud.state.collectAsStateWithLifecycle()
    val hasConnected = environments.any { it.state == ConnectionState.Connected }
    var query by rememberSaveable { mutableStateOf("") }

    S5Screen(
        title = "Settings",
        prominence = S5TopBarProminence.Hero,
        onBack = onBack,
        bottomBar = bottomBar,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            val accountLabel =
                when (val current = account) {
                    CloudAccountState.Unconfigured -> "Not available"
                    CloudAccountState.Loading -> "Checking"
                    is CloudAccountState.SignedIn -> current.label
                    is CloudAccountState.SignedOut -> "Sign in"
                }
            val sections =
                listOf(
                    SettingsSearchSection(
                        "Account",
                        listOf(
                            SettingsLink("account", "S5 account", Icons.Rounded.Person, Routes.SettingsAccount, accountLabel),
                            SettingsLink("provider-accounts", "Provider accounts", Icons.Rounded.Key, "settings/provider-accounts"),
                        ),
                    ),
                    SettingsSearchSection(
                        "App",
                        listOf(
                            SettingsLink("environments", "Environments", Icons.Rounded.Hub, Routes.SettingsEnvironments, "${environments.size}"),
                            SettingsLink("threads", "Thread behavior", Icons.Rounded.Forum, Routes.SettingsThreads),
                            SettingsLink("follow-ups", "Follow-ups", Icons.AutoMirrored.Rounded.ReplyAll, Routes.SettingsFollowUps),
                            SettingsLink("notifications", "Notifications", Icons.Rounded.Notifications, Routes.SettingsNotifications),
                            SettingsLink("scheduled-tasks", "Scheduled tasks", Icons.Rounded.Schedule, "settings/scheduled-tasks"),
                            SettingsLink("appearance", "Appearance", Icons.Rounded.Palette, Routes.SettingsAppearance),
                        ),
                    ),
                    SettingsSearchSection(
                        "App settings",
                        listOfNotNull(
                            SettingsLink("organization", "Organization", Icons.Rounded.Workspaces, Routes.SettingsProjectGrouping),
                            // RN shows Overview only once a project group exists to show it
                            // for; a device with no connected environment has no checkouts.
                            SettingsLink("overview", "Overview", Icons.Rounded.Workspaces, Routes.SettingsOverview)
                                .takeIf { hasConnected },
                            SettingsLink("usage", "Usage limits", Icons.Rounded.Analytics, Routes.Usage),
                            SettingsLink("archive", "Archived threads", Icons.Rounded.Archive, Routes.Archive),
                        ),
                    ),
                    // RN's Server settings section exists only once a server can
                    // answer `server.updateSettings`; the four pages share that gate.
                    SettingsSearchSection(
                        "Server settings",
                        if (!hasConnected) emptyList()
                        else listOf(
                            SettingsLink("new-threads", "New threads", Icons.Rounded.AddComment, Routes.SettingsNewThreads),
                            SettingsLink("source-control", "Source control", Icons.Rounded.AccountTree, Routes.SettingsSourceControl),
                            SettingsLink("agent-behavior", "Agent behavior", Icons.Rounded.Psychology, Routes.SettingsAgentBehavior),
                            SettingsLink("maintenance", "Maintenance", Icons.Rounded.Build, Routes.SettingsMaintenance),
                        ),
                    ),
                    SettingsSearchSection(
                        "About S5 Code",
                        listOf(
                            SettingsLink("client-storage", "Client storage", Icons.Rounded.Storage, Routes.SettingsClientStorage),
                            SettingsLink("diagnostics", "Diagnostics", Icons.Rounded.BugReport, Routes.SettingsDiagnostics),
                        ),
                    ),
                )
            val visible = filterSettingsSections(sections, query, SettingsLink::id, SettingsLink::label)

            S5SearchField(
                value = query,
                onValueChange = { query = it },
                placeholder = "Search settings",
                modifier = Modifier.padding(horizontal = S5Theme.spacing.gutter),
            )
            if (visible.isEmpty()) {
                Text(
                    "No settings match \u201c${query.trim()}\u201d.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.large),
                )
            }
            visible.forEach { section ->
                S5RowGroup(title = section.title) {
                    section.rows.forEachIndexed { index, row ->
                        S5SettingsRow(
                            icon = row.icon,
                            label = row.label,
                            value = row.value,
                            onClick = { onOpen(row.route) },
                            position = rowPosition(index, section.rows.size),
                        )
                    }
                }
                if (section.title == "Account" && query.isBlank()) {
                    Text(
                        "S5 Code works locally without signing in. Cloud features are optional.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(horizontal = S5Theme.spacing.gutter),
                    )
                }
            }
            Box(Modifier.padding(bottom = S5Theme.spacing.section))
        }
    }
}

private data class SettingsLink(
    val id: String,
    val label: String,
    val icon: ImageVector,
    val route: String,
    val value: String? = null,
)
