package club.touchtech.s5code.kotlin.feature.settings

import android.app.Application
import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
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
import androidx.compose.material.icons.rounded.Bolt
import androidx.compose.material.icons.rounded.Cloud
import androidx.compose.material.icons.rounded.Code
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.Difference
import androidx.compose.material.icons.rounded.DoneAll
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.automirrored.rounded.HelpOutline
import androidx.compose.material.icons.rounded.Image
import androidx.compose.material.icons.automirrored.rounded.Login
import androidx.compose.material.icons.automirrored.rounded.Logout
import androidx.compose.material.icons.rounded.Notifications
import androidx.compose.material.icons.automirrored.rounded.OpenInNew
import androidx.compose.material.icons.rounded.Palette
import androidx.compose.material.icons.rounded.PendingActions
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material.icons.rounded.Storage
import androidx.compose.material.icons.rounded.Terminal
import androidx.compose.material.icons.rounded.TextFields
import androidx.compose.material.icons.rounded.Workspaces
import androidx.compose.material.icons.automirrored.rounded.WrapText
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.cloud.CloudAccountState
import club.touchtech.s5code.kotlin.data.ClientCacheInventory
import club.touchtech.s5code.kotlin.data.ClientCacheKind
import club.touchtech.s5code.kotlin.data.ClientCacheStore
import club.touchtech.s5code.kotlin.data.TerminalThemePreference
import club.touchtech.s5code.kotlin.data.cacheSizeLabel
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5CodeBlock
import club.touchtech.s5code.kotlin.design.component.S5ConnectedButtonGroup
import club.touchtech.s5code.kotlin.design.component.S5Markdown
import club.touchtech.s5code.kotlin.design.component.S5LoadingState
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogRequest
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5RowGroup
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SectionHeader
import club.touchtech.s5code.kotlin.design.component.S5SelectableRow
import club.touchtech.s5code.kotlin.design.component.S5SettingsRow
import club.touchtech.s5code.kotlin.design.component.S5ShapeBadge
import club.touchtech.s5code.kotlin.design.component.S5StatusPill
import club.touchtech.s5code.kotlin.design.component.S5SwitchRow
import club.touchtech.s5code.kotlin.design.component.rowPosition
import club.touchtech.s5code.kotlin.design.component.statusPresentation
import club.touchtech.s5code.kotlin.design.theme.S5MaterialShapes
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.design.theme.S5ThemeMode
import club.touchtech.s5code.kotlin.model.ProjectGrouping
import club.touchtech.s5code.kotlin.model.ThreadStatus
import club.touchtech.s5code.kotlin.platform.notifications.AndroidLiveUpdateNotifications
import club.touchtech.s5code.kotlin.platform.notifications.PushRegistrationStatus
import club.touchtech.s5code.kotlin.platform.notifications.notificationsAllowed
import club.touchtech.s5code.kotlin.platform.notifications.openNotificationSettings
import com.clerk.ui.auth.AuthView
import com.clerk.ui.userprofile.UserProfileView
import kotlinx.coroutines.launch

/**
 * S5 account.
 *
 * Signed in, this is Clerk's own [UserProfileView]: email addresses, connected
 * accounts, passkeys, and sessions are Clerk-owned records, and a second UI over
 * them would be a second thing to keep correct. Signed out, the screen offers
 * sign-in and nothing else. An unconfigured build says so instead.
 */
@Composable
fun SettingsAccountScreen(store: AppStore, onBack: () -> Unit) {
    val account by store.cloud.state.collectAsStateWithLifecycle()
    var authOpen by remember { mutableStateOf(false) }

    when (val current = account) {
        CloudAccountState.Unconfigured ->
            S5Screen(title = "S5 account", subtitle = "Not configured in this build", onBack = onBack) {
                padding ->
                Box(Modifier.fillMaxSize().padding(padding).padding(S5Theme.spacing.gutter)) {
                    S5Notice(
                        icon = Icons.Rounded.Cloud,
                        text =
                            "This build has no S5 Connect configuration, so there is no account to " +
                                "manage. Direct pairing works without one.",
                    )
                }
            }
        CloudAccountState.Loading ->
            S5Screen(title = "S5 account", onBack = onBack) { padding ->
                S5LoadingState("Checking your account…", Modifier.padding(padding))
            }
        is CloudAccountState.SignedIn ->
            // Clerk's profile view brings its own navigation, so it replaces the
            // screen rather than sitting inside our scaffold. Its dismiss is the
            // way back to settings.
            UserProfileView(isDismissible = true, onDismiss = onBack)
        is CloudAccountState.SignedOut ->
            if (authOpen) {
                AuthView(isDismissible = true, onDismiss = { authOpen = false })
            } else {
                S5Screen(title = "S5 account", subtitle = "Not signed in", onBack = onBack) { padding ->
                    Column(
                        Modifier.fillMaxSize().padding(padding),
                        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                    ) {
                        Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                            S5Card(tone = S5CardTone.Hero, modifier = Modifier.fillMaxWidth()) {
                                Row(
                                    Modifier.padding(S5Theme.spacing.large),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                                ) {
                                    S5ShapeBadge(
                                        icon = Icons.Rounded.Person,
                                        contentDescription = null,
                                        shape = S5MaterialShapes.avatar(),
                                        containerColor = MaterialTheme.colorScheme.primaryContainer,
                                        contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                                        size = 52.dp,
                                        iconSize = 26.dp,
                                    )
                                    Column(Modifier.weight(1f)) {
                                        Text(
                                            "Signed out",
                                            style = MaterialTheme.typography.titleMediumEmphasized,
                                        )
                                        Text(
                                            current.error
                                                ?: "Sign in to reach machines through the relay",
                                            style = MaterialTheme.typography.bodySmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                    }
                                }
                            }
                        }
                        Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                            S5Button(
                                text = "Sign in",
                                onClick = { authOpen = true },
                                emphasis = S5ActionEmphasis.Primary,
                                icon = Icons.AutoMirrored.Rounded.Login,
                            )
                        }
                        Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                            S5Notice(
                                icon = Icons.Rounded.Cloud,
                                text =
                                    "Signing out clears relay tokens and the device key from the Keystore.",
                            )
                        }
                    }
                }
            }
    }
}

/** Theme, dynamic color, and text/code/terminal sizing. */
@Composable
fun SettingsAppearanceScreen(store: AppStore, onBack: () -> Unit) {
    val preferences by store.preferences.collectAsStateWithLifecycle()
    S5Screen(title = "Appearance", subtitle = preferences.themeMode.name, onBack = onBack) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            S5SectionHeader("Theme")
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                S5ConnectedButtonGroup(
                    options = S5ThemeMode.entries,
                    selected = preferences.themeMode,
                    onSelect = { mode -> store.updatePreferences { it.copy(themeMode = mode) } },
                    label = { it.name },
                )
            }

            S5RowGroup {
                S5SwitchRow(
                    icon = Icons.Rounded.Palette,
                    label = "Dynamic color",
                    supporting = "Follow the system wallpaper palette on Android 12+",
                    checked = preferences.dynamicColor,
                    onCheckedChange = { value ->
                        store.updatePreferences { it.copy(dynamicColor = value) }
                    },
                    position = rowPosition(0, 2),
                )
                S5SwitchRow(
                    icon = Icons.AutoMirrored.Rounded.WrapText,
                    label = "Wrap code",
                    supporting = "Wrap long lines instead of scrolling horizontally",
                    checked = preferences.wrapCode,
                    onCheckedChange = { value -> store.updatePreferences { it.copy(wrapCode = value) } },
                    position = rowPosition(1, 2),
                )
            }

            ScaleSlider(
                icon = Icons.Rounded.TextFields,
                label = "Text size",
                value = preferences.textScale,
                onChange = { value -> store.updatePreferences { it.copy(textScale = value) } },
            )
            ScaleSlider(
                icon = Icons.Rounded.Code,
                label = "Code size",
                value = preferences.codeScale,
                onChange = { value -> store.updatePreferences { it.copy(codeScale = value) } },
            )
            ScaleSlider(
                icon = Icons.Rounded.Terminal,
                label = "Terminal size",
                value = preferences.terminalScale,
                onChange = { value -> store.updatePreferences { it.copy(terminalScale = value) } },
            )
            S5SectionHeader("Terminal theme")
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                S5ConnectedButtonGroup(
                    options = TerminalThemePreference.entries,
                    selected = preferences.terminalTheme,
                    onSelect = { terminalTheme ->
                        store.updatePreferences { it.copy(terminalTheme = terminalTheme) }
                    },
                    label = TerminalThemePreference::label,
                )
            }

            S5SectionHeader("Preview")
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                S5Card(tone = S5CardTone.Standard, modifier = Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(S5Theme.spacing.large)) {
                        S5Markdown(source = "Agent reply with `inline code` and a [link](https://s5.dev).")
                        S5CodeBlock(
                            lines = listOf("fun main() {", "    println(\"hi\")", "}"),
                            language = "kotlin",
                            wrap = preferences.wrapCode,
                        )
                        Row(
                            Modifier.padding(top = S5Theme.spacing.small),
                            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                        ) {
                            ThreadStatus.entries.take(4).forEach { status ->
                                val presentation = statusPresentation(status)
                                S5StatusPill(
                                    label = presentation.label,
                                    containerColor = presentation.container,
                                    contentColor = presentation.content,
                                )
                            }
                        }
                    }
                }
            }
            Box(Modifier.padding(bottom = S5Theme.spacing.section))
        }
    }
}

@Composable
private fun ScaleSlider(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    label: String,
    value: Float,
    onChange: (Float) -> Unit,
) {
    Column(
        Modifier.fillMaxWidth().padding(horizontal = S5Theme.spacing.gutter),
        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Icon(icon, contentDescription = null, modifier = Modifier.size(18.dp))
            Text(label, style = MaterialTheme.typography.labelLarge)
            Box(Modifier.weight(1f))
            Text("${(value * 100).toInt()}%", style = MaterialTheme.typography.labelSmall)
        }
        Slider(
            value = value,
            onValueChange = onChange,
            valueRange = 0.8f..1.6f,
            steps = 7,
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

/** Default project grouping for the home list. */
@Composable
fun SettingsProjectGroupingScreen(store: AppStore, onBack: () -> Unit) {
    val preferences by store.preferences.collectAsStateWithLifecycle()
    S5Screen(title = "Project grouping", subtitle = preferences.projectGrouping.label, onBack = onBack) {
        padding ->
        Column(
            Modifier.fillMaxSize().padding(padding),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            S5RowGroup {
                ProjectGrouping.entries.forEachIndexed { index, grouping ->
                    S5SelectableRow(
                        label = grouping.label,
                        supporting =
                            when (grouping) {
                                ProjectGrouping.ByProject -> "One section per project"
                                ProjectGrouping.ByRepository -> "Group projects that share a repository"
                                ProjectGrouping.Flat -> "No sections, purely by recency"
                            },
                        selected = preferences.projectGrouping == grouping,
                        onClick = { store.updatePreferences { it.copy(projectGrouping = grouping) } },
                        leading = { Icon(Icons.Rounded.Workspaces, contentDescription = null) },
                        position = rowPosition(index, ProjectGrouping.entries.size),
                    )
                }
            }
        }
    }
}

/** Master permission plus per-event notification preferences. */
@Composable
fun SettingsNotificationsScreen(store: AppStore, onBack: () -> Unit) {
    val context = LocalContext.current
    val preferences by store.preferences.collectAsStateWithLifecycle()
    val push by store.pushRuntime.collectAsStateWithLifecycle()
    var permission by remember { mutableStateOf(notificationsAllowed(context)) }
    val permissionLauncher =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            permission = granted && notificationsAllowed(context)
            store.refreshPushRegistration()
        }

    fun requestOrOpenNotifications() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) !=
                PackageManager.PERMISSION_GRANTED
        ) {
            permissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
        } else {
            openNotificationSettings(context)
        }
    }

    S5Screen(
        title = "Notifications",
        subtitle =
            when {
                !permission -> "Blocked"
                push.status == PushRegistrationStatus.Registered -> "Registered"
                else -> "Allowed"
            },
        onBack = onBack,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            if (!permission || push.status != PushRegistrationStatus.Registered) {
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5Notice(
                        icon = Icons.Rounded.ErrorOutline,
                        text =
                            if (!permission) {
                                "Notifications are blocked for S5 Code. Allow them to receive agent alerts."
                            } else {
                                push.detail ?: "This device is not registered for remote notifications."
                            },
                        containerColor = MaterialTheme.colorScheme.errorContainer,
                        contentColor = MaterialTheme.colorScheme.onErrorContainer,
                    )
                }
            }
            S5RowGroup {
                S5SwitchRow(
                    icon = Icons.Rounded.Notifications,
                    label = "Allow notifications",
                    supporting = "Controls the Android permission for this app",
                    checked = permission,
                    onCheckedChange = { allowed ->
                        if (allowed) requestOrOpenNotifications() else openNotificationSettings(context)
                    },
                    position = rowPosition(0, 1),
                )
            }
            S5RowGroup(title = "Notify me when") {
                val rows =
                    listOf(
                        Quad(
                            Icons.Rounded.PendingActions,
                            "Approval needed",
                            preferences.notifyApprovals,
                        ) { value: Boolean ->
                            store.updatePreferences { it.copy(notifyApprovals = value) }
                        },
                        Quad(Icons.AutoMirrored.Rounded.HelpOutline, "Input needed", preferences.notifyInput) {
                            value: Boolean ->
                            store.updatePreferences { it.copy(notifyInput = value) }
                        },
                        Quad(Icons.Rounded.DoneAll, "Turn completed", preferences.notifyCompletion) {
                            value: Boolean ->
                            store.updatePreferences { it.copy(notifyCompletion = value) }
                        },
                        Quad(Icons.Rounded.ErrorOutline, "Turn failed", preferences.notifyFailures) {
                            value: Boolean ->
                            store.updatePreferences { it.copy(notifyFailures = value) }
                        },
                    )
                rows.forEachIndexed { index, row ->
                    S5SwitchRow(
                        icon = row.icon,
                        label = row.label,
                        checked = row.checked,
                        onCheckedChange = row.onChange,
                        enabled = permission,
                        position = rowPosition(index, rows.size),
                    )
                }
            }
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                S5Button(
                    text = "Open Android notification settings",
                    onClick = { openNotificationSettings(context) },
                    emphasis = S5ActionEmphasis.Prominent,
                    style = S5ButtonStyle.Tonal,
                    icon = Icons.AutoMirrored.Rounded.OpenInNew,
                )
            }
        }
    }
}

private data class Quad(
    val icon: androidx.compose.ui.graphics.vector.ImageVector,
    val label: String,
    val checked: Boolean,
    val onChange: (Boolean) -> Unit,
)

/** Live Updates enablement, promotion status, and fallback behavior. */
@Composable
fun SettingsLiveUpdatesScreen(store: AppStore, onBack: () -> Unit) {
    val context = LocalContext.current
    val preferences by store.preferences.collectAsStateWithLifecycle()
    // Re-read after returning from system promotion settings. The screen has no
    // invented defaults: every row below comes from Android or persisted native
    // delivery state.
    val diagnostics = AndroidLiveUpdateNotifications.diagnostics(context)
    val generation = diagnostics.generationId
    val generationLabel =
        when {
            generation == null -> "None armed"
            diagnostics.delivery != null -> "${generation.take(8)} · ${diagnostics.delivery}"
            else -> generation.take(8)
        }
    S5Screen(
        title = "Live Updates",
        subtitle =
            when {
                !diagnostics.supported -> "Requires Android 16"
                preferences.liveUpdatesEnabled -> "Enabled"
                else -> "Disabled"
            },
        onBack = onBack,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            S5RowGroup {
                S5SwitchRow(
                    icon = Icons.Rounded.Bolt,
                    label = "Live Updates",
                    supporting = "Show agent progress on the lock screen and status bar",
                    checked = diagnostics.supported && preferences.liveUpdatesEnabled,
                    enabled = diagnostics.supported && diagnostics.notificationPermission,
                    onCheckedChange = { value ->
                        store.updatePreferences { it.copy(liveUpdatesEnabled = value) }
                        if (!value) AndroidLiveUpdateNotifications.dismiss(context)
                    },
                )
            }
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                S5Card(tone = S5CardTone.Standard, modifier = Modifier.fillMaxWidth()) {
                    Column(
                        Modifier.padding(S5Theme.spacing.large),
                        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                    ) {
                        Text("Diagnostics", style = MaterialTheme.typography.titleSmallEmphasized)
                        listOf(
                                "Promotion permission" to
                                    when {
                                        !diagnostics.supported -> "Unsupported"
                                        diagnostics.promotionPermission -> "Granted"
                                        else -> "Not granted"
                                    },
                                "API level" to
                                    "${diagnostics.apiLevel} (${if (diagnostics.supported) "promoted ongoing" else "standard alerts"})",
                                "Fallback" to
                                    if (diagnostics.notificationPermission) "Ongoing notification" else "Notifications blocked",
                                "Last generation" to generationLabel,
                            )
                            .forEach { (label, value) ->
                                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                                    Text(label, style = MaterialTheme.typography.bodySmall)
                                    Text(value, style = S5Theme.code.inlineTechnical)
                                }
                            }
                    }
                }
            }
            if (diagnostics.supported && !diagnostics.promotionPermission) {
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5Button(
                        text = "Open Live Update settings",
                        onClick = { openNotificationSettings(context, promotion = true) },
                        emphasis = S5ActionEmphasis.Prominent,
                        style = S5ButtonStyle.Tonal,
                        icon = Icons.AutoMirrored.Rounded.OpenInNew,
                    )
                }
            }
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                S5Notice(
                    icon = Icons.Rounded.Bolt,
                    text = "Dismissing a Live Update keeps it hidden until the next turn arms a new one.",
                )
            }
        }
    }
}

/**
 * Cache inventory with per-category and global clear.
 *
 * Real measured bytes, not a hardcoded list: this screen used to show four invented
 * figures and a button that only changed local state, which is a worse lie than
 * having no screen — the user clears, believes the space came back, and nothing
 * happened. Categories come from [ClientCacheKind], which is the client's actual
 * on-disk footprint (see `data/ClientCache.kt`); cached shell rows and opened
 * transcripts are included, while diffs, highlighted lines, and terminal
 * scrollback remain memory-only.
 */
@Composable
fun SettingsClientStorageScreen(
    store: AppStore,
    onBack: () -> Unit,
    confirmController: S5ConfirmDialogController,
) {
    val scope = rememberCoroutineScope()
    val caches = remember { ClientCacheStore(store.getApplication<Application>()) }
    var inventory by remember { mutableStateOf<ClientCacheInventory?>(null) }
    var busy by remember { mutableStateOf(false) }

    // Measured on open and after every clear, because the numbers are only useful if
    // they are current: a stale total is the same failure as the invented one.
    LaunchedEffect(caches) { inventory = caches.inventory() }

    S5Screen(
        title = "Client storage",
        subtitle = inventory?.let { "${cacheSizeLabel(it.totalBytes)} on this device" }
            ?: "Measuring…",
        onBack = onBack,
    ) { padding ->
        val current = inventory
        if (current == null) {
            S5LoadingState("Measuring caches…", Modifier.padding(padding))
            return@S5Screen
        }
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            S5RowGroup(title = "Caches") {
                current.entries.forEachIndexed { index, entry ->
                    S5SettingsRow(
                        icon =
                            when (entry.kind) {
                                ClientCacheKind.Attachments -> Icons.Rounded.Image
                                ClientCacheKind.Images -> Icons.Rounded.Difference
                                ClientCacheKind.Workspace -> Icons.Rounded.Storage
                            },
                        label = entry.label,
                        supporting = entry.detail,
                        value = cacheSizeLabel(entry.bytes),
                        // An empty category has nothing to clear, and a row that
                        // responds by doing nothing is the behaviour this screen was
                        // rewritten to remove.
                        onClick =
                            if (entry.bytes == 0L || busy) null
                            else
                                {
                                    {
                                        confirmController.show(
                                            S5ConfirmDialogRequest(
                                                title = "Clear ${entry.label.lowercase()}?",
                                                message =
                                                    "This removes ${entry.detail.lowercase()} from this device. It does not change anything on the machine.",
                                                confirmText = "Clear cache",
                                                destructive = true,
                                                onConfirm = {
                                                    busy = true
                                                    scope.launch {
                                                        try {
                                                            inventory = caches.clear(entry.kind)
                                                        } catch (error: Exception) {
                                                            store.showError(
                                                                error.message ?: "That cache could not be cleared."
                                                            )
                                                        } finally {
                                                            busy = false
                                                        }
                                                    }
                                                },
                                            )
                                        )
                                    }
                                },
                        position = rowPosition(index, current.entries.size),
                    )
                }
            }
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                S5Button(
                    text = if (busy) "Clearing…" else "Clear all caches",
                    onClick = {
                        confirmController.show(
                            S5ConfirmDialogRequest(
                                title = "Clear all caches?",
                                message =
                                    "This removes attachments, image previews, and offline chat snapshots from this device. Credentials and drafts are kept.",
                                confirmText = "Clear all",
                                destructive = true,
                                onConfirm = {
                                    busy = true
                                    scope.launch {
                                        try {
                                            inventory = caches.clearAll()
                                        } catch (error: Exception) {
                                            store.showError(
                                                error.message ?: "The caches could not be cleared."
                                            )
                                        } finally {
                                            busy = false
                                        }
                                    }
                                },
                            )
                        )
                    },
                    enabled = !busy && !current.isEmpty,
                    emphasis = S5ActionEmphasis.Primary,
                    style = S5ButtonStyle.Outlined,
                    icon = Icons.Rounded.Delete,
                )
            }
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.small)) {
                S5Notice(
                    icon = Icons.Rounded.Storage,
                    text =
                        "Clearing caches never touches credentials, drafts, or anything on the machine. " +
                            "Offline chat snapshots will load again after the environment reconnects.",
                )
            }
        }
    }
}
