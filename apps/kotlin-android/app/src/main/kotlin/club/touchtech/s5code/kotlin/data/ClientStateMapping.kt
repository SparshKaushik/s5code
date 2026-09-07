package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.design.theme.S5ThemeMode
import club.touchtech.s5code.kotlin.model.ApprovalPolicy
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.ProjectGrouping
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ProviderOptionSelection
import club.touchtech.s5code.kotlin.model.ProviderOptionValue
import club.touchtech.s5code.kotlin.model.RuntimeMode
import club.touchtech.s5code.kotlin.model.ThreadSettings
import club.touchtech.s5code.kotlin.model.ThreadSort
import club.touchtech.s5code.kotlin.model.WorkspaceMode

/**
 * Persisted state to runtime state, and back.
 *
 * Enums cross this boundary as names rather than ordinals: an ordinal is a
 * position in a source file, and inserting a theme mode would silently repoint
 * every saved preference. An unknown name falls back to the default, which is what
 * lets a build downgrade without wiping someone's settings.
 */
internal fun <T : Enum<T>> List<T>.byNameOr(name: String, fallback: T): T =
    firstOrNull { it.name == name } ?: fallback

enum class TerminalThemePreference(val label: String) {
    App("Match app"),
    Light("Pierre Light"),
    Dark("Pierre Dark"),
}

fun StoredPreferences.toRuntime(): RuntimePreferences =
    RuntimePreferences(
        themeMode = S5ThemeMode.entries.byNameOr(themeMode, S5ThemeMode.System),
        dynamicColor = dynamicColor,
        projectGrouping = ProjectGrouping.entries.byNameOr(projectGrouping, ProjectGrouping.ByProject),
        threadSort = ThreadSort.entries.byNameOr(threadSort, ThreadSort.Recent),
        snoozedThreadsExpanded = snoozedThreadsExpanded,
        settledThreadsExpanded = settledThreadsExpanded,
        textScale = textScale,
        codeScale = codeScale,
        wrapCode = wrapCode,
        terminalScale = terminalScale,
        terminalTheme = TerminalThemePreference.entries.byNameOr(terminalTheme, TerminalThemePreference.App),
        autoSettleOnMerge = autoSettleOnMerge,
        notifyApprovals = notifyApprovals,
        notifyInput = notifyInput,
        notifyCompletion = notifyCompletion,
        notifyFailures = notifyFailures,
        liveUpdatesEnabled = liveUpdatesEnabled,
    )

/**
 * The runtime preference shape, defined here rather than in `app/` so the mapping
 * and its tests do not need a `ViewModel`.
 */
data class RuntimePreferences(
    val themeMode: S5ThemeMode = S5ThemeMode.System,
    val dynamicColor: Boolean = true,
    val projectGrouping: ProjectGrouping = ProjectGrouping.ByProject,
    val threadSort: ThreadSort = ThreadSort.Recent,
    /**
     * Shelf state, remembered per device.
     *
     * Both default to collapsed. Snoozed and settled are the two piles a user has
     * explicitly put away, and home should open on the work that is still moving.
     * RN keeps these in component state, so they reset on every mount; persisting
     * them is the better behaviour on a phone, where the process is killed and
     * restored constantly and a reset would read as the app forgetting.
     */
    val snoozedThreadsExpanded: Boolean = false,
    val settledThreadsExpanded: Boolean = false,
    val textScale: Float = 1f,
    val codeScale: Float = 1f,
    val wrapCode: Boolean = false,
    val terminalScale: Float = 1f,
    val terminalTheme: TerminalThemePreference = TerminalThemePreference.App,
    val autoSettleOnMerge: Boolean = true,
    val notifyApprovals: Boolean = true,
    val notifyInput: Boolean = true,
    val notifyCompletion: Boolean = true,
    val notifyFailures: Boolean = true,
    val liveUpdatesEnabled: Boolean = true,
)

fun RuntimePreferences.toStored(): StoredPreferences =
    StoredPreferences(
        themeMode = themeMode.name,
        dynamicColor = dynamicColor,
        projectGrouping = projectGrouping.name,
        threadSort = threadSort.name,
        snoozedThreadsExpanded = snoozedThreadsExpanded,
        settledThreadsExpanded = settledThreadsExpanded,
        textScale = textScale,
        codeScale = codeScale,
        wrapCode = wrapCode,
        terminalScale = terminalScale,
        terminalTheme = terminalTheme.name,
        autoSettleOnMerge = autoSettleOnMerge,
        notifyApprovals = notifyApprovals,
        notifyInput = notifyInput,
        notifyCompletion = notifyCompletion,
        notifyFailures = notifyFailures,
        liveUpdatesEnabled = liveUpdatesEnabled,
    )

fun ComposerAttachment.toStored(): StoredAttachment =
    StoredAttachment(id = id, name = name, mimeType = mimeType, sizeBytes = sizeBytes, uri = uri)

fun StoredAttachment.toRuntime(): ComposerAttachment =
    ComposerAttachment(id = id, name = name, mimeType = mimeType, sizeBytes = sizeBytes, uri = uri)

fun StoredThreadSettings.toRuntimeThreadSettings(): ThreadSettings =
    ThreadSettings(
        provider = storedProvider(provider, providerDriver),
        model = model.ifBlank { ThreadSettings().model },
        runtimeMode = storedRuntimeMode(runtimeMode),
        approvalPolicy = storedApprovalPolicy(approvalPolicy),
        options = storedProviderOptions(options),
    )

fun ThreadSettings.toStoredThreadSettings(): StoredThreadSettings =
    StoredThreadSettings(
        provider = provider.instanceId,
        providerDriver = provider.driver,
        model = model,
        runtimeMode = runtimeMode.name,
        approvalPolicy = approvalPolicy.name,
        options = options.associate { it.toStored() },
    )

/** Runtime workspace mode and thread settings, decoded from their stored names. */
fun storedWorkspaceMode(name: String): WorkspaceMode =
    WorkspaceMode.entries.byNameOr(name, WorkspaceMode.CurrentCheckout)

/**
 * The stored provider selection back to a runtime instance.
 *
 * Files written before providers became open-ended hold a driver enum name
 * ("Codex", "OpenCode") where the instance id now lives. Those are translated
 * rather than dropped: a user upgrading should not find their draft pointing at a
 * different agent. An id this build cannot classify still round-trips — the id is
 * what routes, and the label degrades to the id itself.
 */
fun storedProvider(instanceId: String, driver: String): ProviderInstance {
    val legacyDriver = LEGACY_PROVIDER_ENUM_NAMES[instanceId]
    if (legacyDriver != null) {
        return ProviderInstance(instanceId = legacyDriver, driver = legacyDriver)
    }
    val resolved = instanceId.ifBlank { ProviderInstance.Default.instanceId }
    return ProviderInstance(
        instanceId = resolved,
        driver = driver.ifBlank { resolved },
    )
}

/** Driver enum names this app persisted before the open provider model. */
private val LEGACY_PROVIDER_ENUM_NAMES =
    mapOf(
        "Codex" to "codex",
        "Claude" to "claudeAgent",
        "Cursor" to "cursor",
        "Grok" to "grok",
        "OpenCode" to "opencode",
    )

fun storedRuntimeMode(name: String): RuntimeMode =
    RuntimeMode.entries.byNameOr(name, RuntimeMode.Default)

fun storedApprovalPolicy(name: String): ApprovalPolicy =
    ApprovalPolicy.entries.byNameOr(name, ApprovalPolicy.Ask)

/**
 * Stored provider option values back to the contract's canonical array shape.
 *
 * Values with neither field set are dropped: the file was written by a build that
 * knew a third value type, and guessing one would send the provider something it
 * never advertised.
 */
fun storedProviderOptions(stored: Map<String, StoredOptionValue>): List<ProviderOptionSelection> =
    stored.entries
        .sortedBy { it.key }
        .mapNotNull { (id, value) ->
            val runtime =
                value.text?.takeIf { it.isNotBlank() }?.let(ProviderOptionValue::Text)
                    ?: value.flag?.let(ProviderOptionValue::Flag)
            runtime?.let { ProviderOptionSelection(id, it) }
        }

fun ProviderOptionSelection.toStored(): Pair<String, StoredOptionValue> =
    id to
        when (val v = value) {
            is ProviderOptionValue.Text -> StoredOptionValue(text = v.value)
            is ProviderOptionValue.Flag -> StoredOptionValue(flag = v.value)
        }
