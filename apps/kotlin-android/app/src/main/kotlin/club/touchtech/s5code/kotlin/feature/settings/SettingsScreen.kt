package club.touchtech.s5code.kotlin.feature.settings

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
import androidx.compose.foundation.layout.width
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
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.rounded.Build
import androidx.compose.material.icons.rounded.CallSplit
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.Schedule
import androidx.compose.material.icons.rounded.ViewSidebar
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
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
import club.touchtech.s5code.kotlin.design.component.S5SwitchRow
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.rowPosition
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.platform.notifications.PushRegistrationStatus
import club.touchtech.s5code.kotlin.platform.notifications.AndroidLiveUpdateNotifications
import club.touchtech.s5code.kotlin.platform.notifications.notificationsAllowed
import club.touchtech.s5code.kotlin.platform.notifications.openNotificationSettings
import androidx.core.content.ContextCompat
import club.touchtech.s5code.kotlin.platform.updates.AppUpdateStatus
import club.touchtech.s5code.kotlin.model.ConnectionState
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/** Settings root. Every configuration destination plus build identity. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun SettingsScreen(store: AppStore, onBack: () -> Unit, onOpen: (String) -> Unit) {
    val preferences by store.preferences.collectAsStateWithLifecycle()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val account by store.cloud.state.collectAsStateWithLifecycle()
    val updateStatus by store.updates.status.collectAsStateWithLifecycle()
    val push by store.pushRuntime.collectAsStateWithLifecycle()
    val context = LocalContext.current
    // Re-read on every composition path back from system settings; the OS does
    // not push permission changes to us.
    var notificationsAllowedNow by remember { mutableStateOf(notificationsAllowed(context)) }
    LaunchedEffect(Unit) {
        notificationsAllowedNow = notificationsAllowed(context)
    }
    // RN's settings switches read as on only when the device is registered with
    // the relay — a saved preference without delivery is meaningless there too.
    val deviceRegistered = push.status == PushRegistrationStatus.Registered
    val notificationsOn = deviceRegistered && notificationsAllowedNow
    val notificationPermissionLauncher =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            notificationsAllowedNow = granted && notificationsAllowed(context)
            store.refreshPushRegistration()
        }

    fun requestOrOpenNotifications() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) !=
                PackageManager.PERMISSION_GRANTED
        ) {
            notificationPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
        } else {
            openNotificationSettings(context)
        }
    }

    // RN's AutoSettleSettingsRows: every connected, capable environment takes
    // the write; the first supplies the displayed values.
    val syncTargets =
        remember(environments) {
            environments.filter {
                it.state == ConnectionState.Connected && it.capabilities.threadAutoSettlement
            }
        }
    val autoSettleReference = syncTargets.firstOrNull()
    val autoSettleMismatches =
        remember(syncTargets, autoSettleReference?.autoSettleOnMerge, autoSettleReference?.autoSettleAfterDays) {
            if (autoSettleReference == null) emptyList()
            else
                syncTargets.drop(1).filter {
                    it.autoSettleOnMerge != autoSettleReference.autoSettleOnMerge ||
                        it.autoSettleAfterDays != autoSettleReference.autoSettleAfterDays
                }
        }
    val scope = rememberCoroutineScope()

    fun writeAutoSettle(patch: JsonObject) {
        for (target in syncTargets) {
            scope.launch {
                runCatching { store.workspace.updateServerSettings(target.id, patch) }
                    .onFailure { store.showError(it.message ?: "Couldn't update settings.") }
            }
        }
    }

    S5Screen(
        title = "Settings",
        prominence = S5TopBarProminence.Hero,
        onBack = onBack,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            // RN's ConfiguredSettingsRouteScreen: account rows only exist when
            // cloud is configured; the switches carry no subtitle on Android.
            if (account != CloudAccountState.Unconfigured) {
                Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
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
                            label = "S5 Account",
                            value = accountLabel,
                            onClick = { onOpen(Routes.SettingsAccount) },
                        )
                    }
                    Text(
                        "S5 Code works locally without signing in. Cloud features are optional.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(horizontal = S5Theme.spacing.gutter),
                    )
                }
            }

            S5RowGroup(title = "Configuration") {
                val configRowCount = if (account == CloudAccountState.Unconfigured) 1 else 3
                S5SettingsRow(
                    icon = Icons.Rounded.Hub,
                    label = "Environments",
                    value = "${environments.size}",
                    onClick = { onOpen(Routes.SettingsEnvironments) },
                    position = rowPosition(0, configRowCount),
                )
                if (account != CloudAccountState.Unconfigured) {
                    // RN renders these as inline switches on this screen, not
                    // sub-pages. Both read as on only once the device is actually
                    // registered with the relay.
                    S5SwitchRow(
                        icon = Icons.Rounded.Notifications,
                        label = "Device Notifications",
                        checked = notificationsOn,
                        onCheckedChange = { enabled ->
                            if (enabled) {
                                // Enabling needs an account to register against;
                                // RN routes a signed-out user to sign-in.
                                if (account is CloudAccountState.SignedIn) {
                                    requestOrOpenNotifications()
                                } else {
                                    onOpen(Routes.SettingsAccount)
                                }
                            } else {
                                openNotificationSettings(context)
                            }
                        },
                        position = rowPosition(1, configRowCount),
                    )
                    S5SwitchRow(
                        icon = Icons.Rounded.Bolt,
                        label = "Ongoing Agent Activity",
                        checked = preferences.liveUpdatesEnabled && deviceRegistered,
                        onCheckedChange = { enabled ->
                            if (enabled && account !is CloudAccountState.SignedIn) {
                                onOpen(Routes.SettingsAccount)
                                return@S5SwitchRow
                            }
                            store.updatePreferences { it.copy(liveUpdatesEnabled = enabled) }
                            if (!enabled) {
                                AndroidLiveUpdateNotifications.dismiss(context)
                            }
                        },
                        position = rowPosition(2, configRowCount),
                    )
                }
            }

            // RN's GeneralSettingsSection.
            S5RowGroup(title = "General") {
                var generalRows = 2
                if (autoSettleReference != null) generalRows += 2
                var position = 0
                S5SettingsRow(
                    icon = Icons.Rounded.Workspaces,
                    label = "Project Grouping",
                    onClick = { onOpen(Routes.SettingsProjectGrouping) },
                    position = rowPosition(position++, generalRows),
                )
                if (autoSettleReference != null) {
                    S5SwitchRow(
                        icon = Icons.Rounded.CallSplit,
                        label = "Auto-settle merged threads",
                        checked = autoSettleReference.autoSettleOnMerge,
                        onCheckedChange = { enabled ->
                            writeAutoSettle(
                                buildJsonObject { put("sidebarAutoSettleOnMerge", enabled) }
                            )
                        },
                        position = rowPosition(position++, generalRows),
                    )
                    var daysDraft by remember { mutableStateOf<String?>(null) }
                    val afterDays = autoSettleReference.autoSettleAfterDays
                    S5SwitchRow(
                        icon = Icons.Rounded.Schedule,
                        label = "Auto-settle inactive threads",
                        supporting =
                            afterDays?.let { "After $it days without activity" },
                        checked = afterDays != null,
                        onCheckedChange = { enabled ->
                            writeAutoSettle(
                                buildJsonObject {
                                    // Null days disables the idle sweep; the
                                    // server default (3) restores it.
                                    put(
                                        "sidebarAutoSettleAfterDays",
                                        if (enabled) JsonPrimitive(3) else JsonNull,
                                    )
                                }
                            )
                        },
                        position = rowPosition(position++, generalRows),
                    )
                    if (afterDays != null) {
                        Row(
                            Modifier.fillMaxWidth().padding(horizontal = S5Theme.spacing.gutter),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                        ) {
                            Text(
                                "Days before auto-settle",
                                modifier = Modifier.weight(1f),
                            )
                            OutlinedTextField(
                                value = daysDraft ?: afterDays.toString(),
                                onValueChange = { daysDraft = it },
                                modifier = Modifier.width(96.dp),
                                keyboardOptions =
                                    KeyboardOptions(
                                        keyboardType = KeyboardType.Number,
                                        imeAction = ImeAction.Done,
                                    ),
                                keyboardActions =
                                    KeyboardActions(
                                        onDone = {
                                            // Whole-string check so "3.5" is
                                            // rejected rather than silently
                                            // becoming 3 on every sync target.
                                            val parsed =
                                                daysDraft?.trim()?.toIntOrNull()
                                            daysDraft = null
                                            if (parsed != null && parsed in 1..90 &&
                                                parsed != afterDays
                                            ) {
                                                writeAutoSettle(
                                                    buildJsonObject {
                                                        put(
                                                            "sidebarAutoSettleAfterDays",
                                                            JsonPrimitive(parsed),
                                                        )
                                                    }
                                                )
                                            }
                                        }
                                    ),
                                singleLine = true,
                                textStyle = MaterialTheme.typography.bodyLarge,
                            )
                        }
                    }
                    if (autoSettleMismatches.isNotEmpty()) {
                        Row(
                            Modifier.fillMaxWidth().padding(horizontal = S5Theme.spacing.gutter),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                        ) {
                            Column(Modifier.weight(1f)) {
                                Text("Auto-settle defaults differ")
                                Text(
                                    autoSettleMismatches.joinToString(", ") { it.label },
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                            S5Button(
                                text = "Apply auto-settle defaults",
                                style = S5ButtonStyle.Tonal,
                                onClick = {
                                    writeAutoSettle(
                                        buildJsonObject {
                                            put(
                                                "sidebarAutoSettleOnMerge",
                                                autoSettleReference.autoSettleOnMerge,
                                            )
                                            autoSettleReference.autoSettleAfterDays.let { days ->
                                                put(
                                                    "sidebarAutoSettleAfterDays",
                                                    days?.let { JsonPrimitive(it) } ?: JsonNull,
                                                )
                                            }
                                        }
                                    )
                                },
                            )
                        }
                    }
                }
                S5SettingsRow(
                    icon = Icons.Rounded.Analytics,
                    label = "Usage",
                    onClick = { onOpen(Routes.Usage) },
                    position = rowPosition(position, generalRows),
                )
            }

            S5RowGroup(title = "Appearance") {
                S5SettingsRow(
                    icon = Icons.Rounded.Palette,
                    label = "Appearance",
                    onClick = { onOpen(Routes.SettingsAppearance) },
                )
            }

            // RN's LegacySettingsSection plus its footnote.
            Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                S5RowGroup(title = "Legacy") {
                    S5SwitchRow(
                        icon = Icons.Rounded.ViewSidebar,
                        label = "Legacy Thread List",
                        checked = preferences.legacyThreadListEnabled,
                        onCheckedChange = { enabled ->
                            store.updatePreferences { it.copy(legacyThreadListEnabled = enabled) }
                        },
                        position = rowPosition(0, 2),
                    )
                    S5SwitchRow(
                        icon = Icons.Rounded.Build,
                        label = "Plan Mode",
                        checked = preferences.planModeEnabled,
                        onCheckedChange = { enabled ->
                            store.updatePreferences { it.copy(planModeEnabled = enabled) }
                        },
                        position = rowPosition(1, 2),
                    )
                }
                Text(
                    "Opt into retired interfaces kept for compatibility. Plan Mode restores the Build/Plan control; otherwise every task runs in Build mode.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = S5Theme.spacing.gutter),
                )
            }

            S5RowGroup(title = "Threads") {
                S5SettingsRow(
                    icon = Icons.Rounded.Archive,
                    label = "Archived Threads",
                    onClick = { onOpen(Routes.Archive) },
                )
            }

            // RN's AppSettingsSection: storage + the version row. The update
            // card below it is this client's own updater, which RN reaches
            // through a hidden gesture on the version row.
            S5RowGroup(title = "App") {
                S5SettingsRow(
                    icon = Icons.Rounded.Storage,
                    label = "Client Storage",
                    onClick = { onOpen(Routes.SettingsClientStorage) },
                    position = rowPosition(0, 2),
                )
                S5SettingsRow(
                    icon = Icons.Rounded.Info,
                    label = "Version",
                    value = BuildConfig.VERSION_NAME,
                    onClick = null,
                    position = rowPosition(1, 2),
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
