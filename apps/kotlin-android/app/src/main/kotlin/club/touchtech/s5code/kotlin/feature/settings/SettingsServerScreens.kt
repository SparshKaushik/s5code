package club.touchtech.s5code.kotlin.feature.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.CallMerge
import androidx.compose.material.icons.rounded.Archive
import androidx.compose.material.icons.rounded.ArrowDownward
import androidx.compose.material.icons.rounded.CallSplit
import androidx.compose.material.icons.rounded.Commit
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.History
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Public
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Smartphone
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5RowGroup
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SelectableRow
import club.touchtech.s5code.kotlin.design.component.S5SwitchRow
import club.touchtech.s5code.kotlin.design.component.S5TextField
import club.touchtech.s5code.kotlin.design.component.rowPosition
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.model.Environment
import club.touchtech.s5code.kotlin.transport.v2String
import club.touchtech.s5code.kotlin.transport.wire.ServerStorageCleanupDto
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonObject

/**
 * The Settings → Server settings pages, ported from
 * `SettingsServerControlsRouteScreen.tsx` in the RN client.
 *
 * Kotlin has no per-project settings scope, so every page operates at
 * environment scope only: the first connected environment supplies the
 * displayed values and each write lands on all of them — the same
 * "every connected, capable environment takes the write" rule the auto-settle
 * rows already use.
 */

/** The environments a server-settings write fans out to. */
private class ServerSettingsSync(
    private val scope: CoroutineScope,
    private val store: AppStore,
    val targets: List<Environment>,
) {
    /** The environment whose values render; null while nothing is connected. */
    val reference get() = targets.firstOrNull()

    /** Sends [patch] through `server.updateSettings` on every target. */
    fun write(patch: JsonObject) {
        for (target in targets) {
            scope.launch {
                runCatching { store.workspace.updateServerSettings(target.id, patch) }
                    .onFailure { store.showError(it.message ?: "Couldn't update settings.") }
            }
        }
    }
}

@Composable
private fun rememberServerSettingsSync(
    store: AppStore,
    filter: (Environment) -> Boolean = { true },
): ServerSettingsSync {
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val targets =
        remember(environments) {
            environments.filter { it.state == ConnectionState.Connected && filter(it) }
        }
    return remember(scope, targets) { ServerSettingsSync(scope, store, targets) }
}

/** Shared empty state for a page that has nothing to edit while disconnected. */
@Composable
private fun NoConnectedEnvironment() {
    Text(
        "Connect an environment to change its settings.",
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = S5Theme.spacing.gutter),
    )
}

/** One option of an environment-scoped choice row, matching RN's SettingsChoiceRow. */
private class ServerChoice(
    /** The patch value; null means write `null` (inherit) rather than a string. */
    val value: String?,
    val label: String,
    val description: String,
)

@Composable
private fun ServerSettingChoices(
    title: String,
    choices: List<ServerChoice>,
    current: String?,
    onSelect: (String?) -> Unit,
) {
    S5RowGroup(title = title) {
        choices.forEachIndexed { index, choice ->
            S5SelectableRow(
                label = choice.label,
                supporting = choice.description,
                selected = current == choice.value,
                onClick = { onSelect(choice.value) },
                position = rowPosition(index, choices.size),
            )
        }
    }
}

/**
 * A text-backed server setting. Writes on the Save affordance once the draft
 * actually differs — `TrimmedString` semantics, so a blank field clears the
 * value rather than being refused.
 */
@Composable
private fun ServerTextSetting(
    label: String,
    placeholder: String,
    value: String,
    multiline: Boolean = false,
    supporting: String? = null,
    onCommit: (String) -> Unit,
) {
    var draft by remember(value) { mutableStateOf(value) }
    val trimmed = draft.trim()
    S5TextField(
        value = draft,
        onValueChange = { draft = it },
        modifier = Modifier.padding(horizontal = S5Theme.spacing.gutter),
        label = label,
        placeholder = placeholder,
        supporting = supporting,
        singleLine = !multiline,
        trailing =
            if (trimmed != value.trim()) {
                { S5Button(text = "Save", style = S5ButtonStyle.Text, onClick = { onCommit(trimmed) }) }
            } else {
                null
            },
    )
}

/**
 * A whole-days value like the auto-settle row's: a small number field that
 * commits on keyboard Done. Blank or out-of-range input means "off" when
 * [allowClear] — `StorageRetentionDays` accepts null to clear the rule.
 */
@Composable
private fun ServerDaysRow(
    label: String,
    value: Int?,
    range: IntRange,
    allowClear: Boolean = false,
    onCommit: (Int?) -> Unit,
) {
    var draft by remember(value) { mutableStateOf<String?>(null) }
    Row(
        Modifier.fillMaxWidth().padding(horizontal = S5Theme.spacing.gutter),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        Text(label, modifier = Modifier.weight(1f))
        OutlinedTextField(
            value = draft ?: value?.toString().orEmpty(),
            onValueChange = { draft = it },
            modifier = Modifier.width(96.dp),
            placeholder = { Text("Off") },
            keyboardOptions =
                KeyboardOptions(keyboardType = KeyboardType.Number, imeAction = ImeAction.Done),
            keyboardActions =
                KeyboardActions(
                    onDone = {
                        // Whole-string check so "3.5" is rejected rather than
                        // silently becoming 3 on every sync target.
                        val typed = draft
                        draft = null
                        when {
                            typed == null -> Unit
                            typed.trim().isEmpty() && allowClear ->
                                if (value != null) onCommit(null)
                            else -> {
                                val parsed = typed.trim().toIntOrNull()
                                if (parsed != null && parsed in range && parsed != value) {
                                    onCommit(parsed)
                                }
                            }
                        }
                    }
                ),
            singleLine = true,
            textStyle = MaterialTheme.typography.bodyLarge,
        )
    }
}

/* ── New threads ───────────────────────────────────────────────────── */

private val RUNTIME_MODE_CHOICES =
    listOf(
        ServerChoice(
            "approval-required",
            "Supervised",
            "Ask before commands and file changes.",
        ),
        ServerChoice(
            "auto-accept-edits",
            "Auto-accept edits",
            "Auto-approve edits, ask before other actions.",
        ),
        ServerChoice(
            "auto",
            "Auto",
            "Supported providers approve routine actions; others still ask.",
        ),
        ServerChoice(
            "full-access",
            "Full access",
            "Allow commands and edits without prompts.",
        ),
    )

private val WORKSPACE_CHOICES =
    listOf(
        ServerChoice(
            null,
            "Inherit",
            "Use the repository's t3.json, or the current checkout.",
        ),
        ServerChoice(
            "local",
            "Current checkout",
            "Start new threads in the existing workspace.",
        ),
        ServerChoice(
            "worktree",
            "New worktree",
            "Give each new thread a separate checkout.",
        ),
    )

private val SUBMODULE_CHOICES =
    listOf(
        ServerChoice(
            null,
            "Inherit",
            "Use the repository's t3.json, or initialize recursively.",
        ),
        ServerChoice("recursive", "Recursive", "Initialize nested submodules too."),
        ServerChoice(
            "top-level",
            "Top level only",
            "Skip submodules declared inside other submodules.",
        ),
        ServerChoice("none", "Skip", "Leave submodules empty for a setup script."),
    )

/** RN's `SettingsEnvironmentNewThreadsRouteScreen`, environment scope only. */
@Composable
fun SettingsNewThreadsScreen(store: AppStore, onBack: () -> Unit) {
    val sync = rememberServerSettingsSync(store)
    val settings = sync.reference?.serverSettings
    S5Screen(title = "New threads", onBack = onBack) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            if (settings == null) {
                NoConnectedEnvironment()
            } else {
                ServerSettingChoices(
                    title = "Default interaction mode",
                    choices = RUNTIME_MODE_CHOICES,
                    current = settings.defaultRuntimeMode,
                    onSelect = { mode ->
                        sync.write(buildJsonObject { put("defaultRuntimeMode", mode) })
                    },
                )
                ServerSettingChoices(
                    title = "Default workspace",
                    choices = WORKSPACE_CHOICES,
                    current = settings.defaultThreadEnvMode,
                    onSelect = { mode ->
                        sync.write(
                            buildJsonObject {
                                put("defaultThreadEnvMode", mode?.let(::JsonPrimitive) ?: JsonNull)
                            }
                        )
                    },
                )
                S5RowGroup(title = "Worktrees") {
                    S5SwitchRow(
                        icon = Icons.Rounded.CallSplit,
                        label = "Start from origin",
                        supporting = "Base new worktrees on the remote branch.",
                        checked = settings.newWorktreesStartFromOrigin,
                        onCheckedChange = { enabled ->
                            sync.write(
                                buildJsonObject { put("newWorktreesStartFromOrigin", enabled) }
                            )
                        },
                    )
                }
                ServerSettingChoices(
                    title = "Worktree submodules",
                    choices = SUBMODULE_CHOICES,
                    current = settings.worktreeSubmodules,
                    onSelect = { mode ->
                        sync.write(
                            buildJsonObject {
                                put("worktreeSubmodules", mode?.let(::JsonPrimitive) ?: JsonNull)
                            }
                        )
                    },
                )
                S5RowGroup(title = "Default branch") {
                    S5SwitchRow(
                        icon = Icons.Rounded.ArrowDownward,
                        label = "Pull latest changes automatically",
                        supporting = "Keep the default branch current when there are no local changes.",
                        checked = settings.defaultAutoPull,
                        onCheckedChange = { enabled ->
                            sync.write(buildJsonObject { put("defaultAutoPull", enabled) })
                        },
                    )
                }
            }
            Box(Modifier.padding(bottom = S5Theme.spacing.section))
        }
    }
}

/* ── Source control ────────────────────────────────────────────────── */

private val BRANCH_NAMING_CHOICES =
    listOf(
        ServerChoice(
            "static",
            "Static prefix",
            "Add your prefix to the generated branch name.",
        ),
        ServerChoice(
            "semantic",
            "Semantic prefix",
            "Let the model choose feat/, fix/, refactor/, or another prefix.",
        ),
        ServerChoice(
            "custom",
            "Custom instructions",
            "Generate the complete name with no added prefix or suffix.",
        ),
    )

/**
 * RN's `SettingsEnvironmentSourceControlRouteScreen`: `BranchNamingSettings`
 * ported row-for-row, with the prefix/instructions fields writing trimmed,
 * non-empty diffs.
 */
@Composable
fun SettingsSourceControlScreen(store: AppStore, onBack: () -> Unit) {
    val sync = rememberServerSettingsSync(store)
    val settings = sync.reference?.serverSettings
    S5Screen(title = "Source control", onBack = onBack) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            if (settings == null) {
                NoConnectedEnvironment()
            } else {
                ServerSettingChoices(
                    title = "Worktree branch naming",
                    choices = BRANCH_NAMING_CHOICES,
                    current = settings.branchNamingMode,
                    onSelect = { mode ->
                        sync.write(buildJsonObject { put("branchNamingMode", mode) })
                    },
                )
                if (settings.branchNamingMode == "static") {
                    ServerTextSetting(
                        label = "Branch prefix",
                        placeholder = "No prefix",
                        supporting =
                            "Use t3code or t3code/ for t3code/add-search. Leave empty for no prefix.",
                        value = settings.branchNamePrefix,
                        onCommit = { text ->
                            sync.write(buildJsonObject { put("branchNamePrefix", text) })
                        },
                    )
                }
                if (settings.branchNamingMode == "custom") {
                    ServerTextSetting(
                        label = "Branch naming instructions",
                        placeholder =
                            "Use julius/ followed by the issue ID and a short description.",
                        supporting = "Append instructions to the naming prompt.",
                        value = settings.branchNameInstructions,
                        multiline = true,
                        onCommit = { text ->
                            sync.write(buildJsonObject { put("branchNameInstructions", text) })
                        },
                    )
                }
            }
            Box(Modifier.padding(bottom = S5Theme.spacing.section))
        }
    }
}

/* ── Agent behavior ────────────────────────────────────────────────── */

private val STREAMING_CHOICES =
    listOf(
        ServerChoice(
            "turn",
            "After the turn",
            "Show the answer when the agent finishes.",
        ),
        ServerChoice(
            "paragraph",
            "Finished paragraphs",
            "Show each paragraph or code block as it completes.",
        ),
    )

/** RN's `SettingsEnvironmentAgentBehaviorRouteScreen`, environment scope only. */
@Composable
fun SettingsAgentBehaviorScreen(store: AppStore, onBack: () -> Unit) {
    val sync = rememberServerSettingsSync(store)
    val settings = sync.reference?.serverSettings
    val targets = sync.targets
    // RN's `supportsContinuation`: every target must persist the preference or
    // the switch would silently do nothing on the stragglers.
    val supportsContinuation =
        targets.isNotEmpty() && targets.all { it.capabilities.threadRestartContinuation }
    S5Screen(title = "Agent behavior", onBack = onBack) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            if (settings == null) {
                NoConnectedEnvironment()
            } else {
                ServerSettingChoices(
                    title = "Response streaming",
                    choices = STREAMING_CHOICES,
                    current = settings.responseStreamingMode,
                    onSelect = { mode ->
                        sync.write(buildJsonObject { put("responseStreamingMode", mode) })
                    },
                )
                S5RowGroup(title = "Previews and devices") {
                    S5SwitchRow(
                        icon = Icons.Rounded.Public,
                        label = "Agent browser access",
                        supporting = "Allow agents to use the in-app preview browser.",
                        checked = settings.enableAgentBrowserAccess,
                        onCheckedChange = { enabled ->
                            sync.write(
                                buildJsonObject { put("enableAgentBrowserAccess", enabled) }
                            )
                        },
                        position = rowPosition(0, 2),
                    )
                    S5SwitchRow(
                        icon = Icons.Rounded.Smartphone,
                        label = "Agent device access",
                        supporting = "Allow agents to drive simulators and emulators.",
                        checked = settings.enableAgentDeviceAccess,
                        onCheckedChange = { enabled ->
                            sync.write(buildJsonObject { put("enableAgentDeviceAccess", enabled) })
                        },
                        position = rowPosition(1, 2),
                    )
                }
                S5RowGroup(title = "Updates") {
                    S5SwitchRow(
                        icon = Icons.Rounded.Refresh,
                        label = "Check provider updates",
                        supporting = "Check installed provider CLIs for newer versions.",
                        checked = settings.enableProviderUpdateChecks,
                        onCheckedChange = { enabled ->
                            sync.write(
                                buildJsonObject { put("enableProviderUpdateChecks", enabled) }
                            )
                        },
                        position = rowPosition(0, 2),
                    )
                    S5SwitchRow(
                        icon = Icons.Rounded.PlayArrow,
                        label = "Continue after restart",
                        supporting =
                            if (supportsContinuation) {
                                "Resume interrupted threads after an update or restart."
                            } else {
                                "Update older servers to control restart continuation."
                            },
                        checked = settings.continueThreadsAfterServerUpdate == true,
                        enabled = supportsContinuation,
                        onCheckedChange = { enabled ->
                            sync.write(
                                buildJsonObject {
                                    put("continueThreadsAfterServerUpdate", enabled)
                                }
                            )
                        },
                        position = rowPosition(1, 2),
                    )
                }
            }
            Box(Modifier.padding(bottom = S5Theme.spacing.section))
        }
    }
}

/* ── Maintenance ───────────────────────────────────────────────────── */

private val WORKTREE_CLEANUP_MODES =
    listOf(
        ServerChoice(null, "Inherit", "Use the storage cleanup rules below."),
        ServerChoice("off", "Off", "Keep worktrees until you delete them manually."),
        ServerChoice("custom", "Custom", "Use dedicated worktree cleanup rules."),
    )

/** `worktreeCleanup.mode` on the wire; null (no policy object) means inherit. */
private fun worktreeCleanupMode(cleanup: JsonObject?): String? =
    cleanup?.v2String("mode")

/**
 * The rules a worktree cleanup write seeds from: the custom rules when set,
 * otherwise the `storageCleanup` worktree fields, matching
 * `resolveWorktreeCleanup` in packages/shared.
 */
private fun resolvedWorktreeRules(
    cleanup: JsonObject?,
    storage: ServerStorageCleanupDto,
): ServerStorageCleanupDto {
    val rules = (cleanup?.get("rules") as? JsonObject) ?: return storage
    return ServerStorageCleanupDto(
        worktreeAfterDays = (rules["worktreeAfterDays"] as? JsonPrimitive)?.intOrNull,
        worktreeOnMerge =
            (rules["worktreeOnMerge"] as? JsonPrimitive)?.booleanOrNull
                ?: storage.worktreeOnMerge,
        worktreeOnDelete =
            (rules["worktreeOnDelete"] as? JsonPrimitive)?.booleanOrNull
                ?: storage.worktreeOnDelete,
        worktreeUnchanged =
            (rules["worktreeUnchanged"] as? JsonPrimitive)?.booleanOrNull
                ?: storage.worktreeUnchanged,
        browserArtifactsAfterDays = storage.browserArtifactsAfterDays,
        logsAfterDays = storage.logsAfterDays,
    )
}

/**
 * RN's `SettingsEnvironmentMaintenanceRouteScreen` plus the worktree/storage
 * cleanup controls web renders in `StorageSettings.tsx`. Only targets with the
 * `storageCleanup` capability take writes — an older server would drop the
 * patch silently.
 */
@Composable
fun SettingsMaintenanceScreen(store: AppStore, onBack: () -> Unit) {
    val sync = rememberServerSettingsSync(store)
    val capable = rememberServerSettingsSync(store) { it.capabilities.storageCleanup }
    val settings = sync.reference?.serverSettings
    val cleanupCapable = capable.reference != null && capable.targets.size == sync.targets.size
    S5Screen(title = "Maintenance", onBack = onBack) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            if (settings == null) {
                NoConnectedEnvironment()
            } else {
                val cleanup = settings.worktreeCleanup
                val mode = worktreeCleanupMode(cleanup)
                val rules = resolvedWorktreeRules(cleanup, settings.storageCleanup)
                val storage = settings.storageCleanup

                fun writeWorktreePatch(key: String, element: kotlinx.serialization.json.JsonElement) {
                    if (mode == "custom") {
                        capable.write(
                            buildJsonObject {
                                putJsonObject("worktreeCleanup") {
                                    put("mode", "custom")
                                    putJsonObject("rules") { put(key, element) }
                                }
                            }
                        )
                    } else {
                        // Inherit keeps the worktree rules inside
                        // `storageCleanup`, like web's StorageSettings.
                        capable.write(
                            buildJsonObject { putJsonObject("storageCleanup") { put(key, element) } }
                        )
                    }
                }

                if (cleanupCapable) {
                    ServerSettingChoices(
                        title = "Worktree cleanup",
                        choices = WORKTREE_CLEANUP_MODES,
                        current = mode,
                        onSelect = { next ->
                            when (next) {
                                null ->
                                    capable.write(buildJsonObject { put("worktreeCleanup", JsonNull) })
                                "off" ->
                                    capable.write(
                                        buildJsonObject {
                                            putJsonObject("worktreeCleanup") { put("mode", "off") }
                                        }
                                    )
                                else ->
                                    // Seed custom from the resolved rules so
                                    // flipping modes never loses the current
                                    // schedule.
                                    capable.write(
                                        buildJsonObject {
                                            putJsonObject("worktreeCleanup") {
                                                put("mode", "custom")
                                                putJsonObject("rules") {
                                                    put("worktreeOnMerge", rules.worktreeOnMerge)
                                                    put("worktreeOnDelete", rules.worktreeOnDelete)
                                                    put("worktreeUnchanged", rules.worktreeUnchanged)
                                                    put(
                                                        "worktreeAfterDays",
                                                        rules.worktreeAfterDays?.let(::JsonPrimitive)
                                                            ?: JsonNull,
                                                    )
                                                }
                                            }
                                        }
                                    )
                            }
                        },
                    )
                    if (mode != "off") {
                        S5RowGroup(title = "Worktree rules") {
                            val rows = 3
                            var position = 0
                            S5SwitchRow(
                                icon = Icons.AutoMirrored.Rounded.CallMerge,
                                label = "Delete merged worktrees",
                                supporting =
                                    "Remove worktrees whose pull request is merged and whose commits are included in the default branch.",
                                checked = rules.worktreeOnMerge,
                                onCheckedChange = { enabled ->
                                    writeWorktreePatch("worktreeOnMerge", JsonPrimitive(enabled))
                                },
                                position = rowPosition(position++, rows),
                            )
                            S5SwitchRow(
                                icon = Icons.Rounded.Delete,
                                label = "Delete worktrees with deleted threads",
                                supporting =
                                    "Remove unused worktrees when active or archived threads are deleted. Worktrees with local changes are kept.",
                                checked = rules.worktreeOnDelete,
                                onCheckedChange = { enabled ->
                                    writeWorktreePatch("worktreeOnDelete", JsonPrimitive(enabled))
                                },
                                position = rowPosition(position++, rows),
                            )
                            S5SwitchRow(
                                icon = Icons.Rounded.Commit,
                                label = "Delete unchanged worktrees",
                                supporting = "Remove worktrees with no commits beyond the default branch.",
                                checked = rules.worktreeUnchanged,
                                onCheckedChange = { enabled ->
                                    writeWorktreePatch("worktreeUnchanged", JsonPrimitive(enabled))
                                },
                                position = rowPosition(position++, rows),
                            )
                        }
                        ServerDaysRow(
                            label = "Delete inactive worktrees after (days)",
                            value = rules.worktreeAfterDays,
                            range = 1..3650,
                            allowClear = true,
                            onCommit = { days ->
                                writeWorktreePatch(
                                    "worktreeAfterDays",
                                    days?.let(::JsonPrimitive) ?: JsonNull,
                                )
                            },
                        )
                    }
                    S5RowGroup(title = "Storage cleanup") {
                        S5SwitchRow(
                            icon = Icons.Rounded.History,
                            label = "Delete old browser artifacts",
                            supporting =
                                "Delete saved browser captures after this many days. Older capture links will no longer open.",
                            checked = storage.browserArtifactsAfterDays != null,
                            onCheckedChange = { enabled ->
                                capable.write(
                                    buildJsonObject {
                                        putJsonObject("storageCleanup") {
                                            put(
                                                "browserArtifactsAfterDays",
                                                if (enabled) JsonPrimitive(8) else JsonNull,
                                            )
                                        }
                                    }
                                )
                            },
                            position = rowPosition(0, 2),
                        )
                        S5SwitchRow(
                            icon = Icons.Rounded.Archive,
                            label = "Delete old rotated logs",
                            supporting =
                                "Delete inactive rotated log files after this many days. Current logs are kept.",
                            checked = storage.logsAfterDays != null,
                            onCheckedChange = { enabled ->
                                capable.write(
                                    buildJsonObject {
                                        putJsonObject("storageCleanup") {
                                            put(
                                                "logsAfterDays",
                                                if (enabled) JsonPrimitive(8) else JsonNull,
                                            )
                                        }
                                    }
                                )
                            },
                            position = rowPosition(1, 2),
                        )
                    }
                    if (storage.browserArtifactsAfterDays != null) {
                        ServerDaysRow(
                            label = "Days before browser artifacts are deleted",
                            value = storage.browserArtifactsAfterDays,
                            range = 1..3650,
                            allowClear = true,
                            onCommit = { days ->
                                capable.write(
                                    buildJsonObject {
                                        putJsonObject("storageCleanup") {
                                            put(
                                                "browserArtifactsAfterDays",
                                                days?.let(::JsonPrimitive) ?: JsonNull,
                                            )
                                        }
                                    }
                                )
                            },
                        )
                    }
                    if (storage.logsAfterDays != null) {
                        ServerDaysRow(
                            label = "Days before rotated logs are deleted",
                            value = storage.logsAfterDays,
                            range = 1..3650,
                            allowClear = true,
                            onCommit = { days ->
                                capable.write(
                                    buildJsonObject {
                                        putJsonObject("storageCleanup") {
                                            put(
                                                "logsAfterDays",
                                                days?.let(::JsonPrimitive) ?: JsonNull,
                                            )
                                        }
                                    }
                                )
                            },
                        )
                    }
                } else {
                    Text(
                        "This environment does not support storage cleanup.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(horizontal = S5Theme.spacing.gutter),
                    )
                }
            }
            Box(Modifier.padding(bottom = S5Theme.spacing.section))
        }
    }
}
