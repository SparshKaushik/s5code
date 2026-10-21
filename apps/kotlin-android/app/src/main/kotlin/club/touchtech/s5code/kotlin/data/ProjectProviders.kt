package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ProviderCatalogEntry
import club.touchtech.s5code.kotlin.transport.v2String
import club.touchtech.s5code.kotlin.transport.wire.ServerConfigSettingsDto
import club.touchtech.s5code.kotlin.transport.wire.ServerProviderDto
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull

/**
 * Per-project provider switches, ported from `resolveProjectProviderInstanceEnabled`
 * in `packages/contracts/src/settings.ts`.
 */

/** Matches the `enabled` decoding default of each driver's settings schema. */
private val DEFAULT_DISABLED_PROVIDER_DRIVERS = setOf("cursor", "grok", "muse", "pi", "opencode", "antigravity")

/** Built-in instances that run without any configuration entry. */
private val DEFAULT_ENABLED_DEFAULT_INSTANCES = setOf("codex", "claudeAgent")

private fun JsonObject.booleanOrNull(key: String): Boolean? = (get(key) as? JsonPrimitive)?.booleanOrNull

/** `resolveProviderInstanceEnabled`: either switch off wins; otherwise the instance, its config, then the driver default. */
internal fun resolveProviderInstanceEnabled(instance: JsonObject): Boolean {
    val enabled = instance.booleanOrNull("enabled")
    val configEnabled = (instance["config"] as? JsonObject)?.booleanOrNull("enabled")
    if (enabled == false || configEnabled == false) return false
    return enabled ?: configEnabled ?: (instance.v2String("driver") !in DEFAULT_DISABLED_PROVIDER_DRIVERS)
}

/** The project's explicit switch for [instanceId], or null when it inherits. */
internal fun projectProviderInstanceOverride(
    settings: ServerConfigSettingsDto,
    projectId: String,
    instanceId: String,
): Boolean? =
    ((((settings.projectSettingsOverrides?.get(projectId) as? JsonObject)
        ?.get("providerInstances") as? JsonObject)
        ?.get(instanceId) as? JsonObject))
        ?.booleanOrNull("enabled")

/**
 * A project's own switch wins; otherwise the machine-wide instance config, and an
 * instance with no config at all is on only when it is a built-in default.
 */
internal fun resolveProjectProviderInstanceEnabled(
    settings: ServerConfigSettingsDto,
    projectId: String?,
    instanceId: String,
): Boolean {
    projectId?.let { projectProviderInstanceOverride(settings, it, instanceId) }?.let { return it }
    val instance = settings.providerInstances[instanceId]
    return if (instance != null) resolveProviderInstanceEnabled(instance)
    else instanceId in DEFAULT_ENABLED_DEFAULT_INSTANCES
}

/** Every project's explicit switch for [instanceId], by project id. */
internal fun projectProviderInstanceOverrides(
    settings: ServerConfigSettingsDto,
    instanceId: String,
): Map<String, Boolean> =
    settings.projectSettingsOverrides.orEmpty().keys.mapNotNull { projectId ->
        projectProviderInstanceOverride(settings, projectId, instanceId)?.let { projectId to it }
    }.toMap()

/**
 * `isDisabledForProject` in `apps/mobile/src/lib/modelOptions.ts`: a project's
 * explicit off switch, or a known instance that resolves off for the project.
 */
internal fun isModelTurnedOffForProject(
    settings: ServerConfigSettingsDto,
    providers: List<ServerProviderDto>,
    projectId: String?,
    instanceId: String,
): Boolean {
    if (projectId != null && projectProviderInstanceOverride(settings, projectId, instanceId) == false) return true
    return providers.any { it.instanceId == instanceId } &&
        !resolveProjectProviderInstanceEnabled(settings, projectId, instanceId)
}

/**
 * `isModelSelectionUnavailable` in `apps/mobile/src/lib/modelOptions.ts`: the
 * next send would skip this selection, because the instance is turned off for
 * the project or it is an Antigravity instance that cannot run the model.
 */
internal fun isModelSelectionUnavailable(
    settings: ServerConfigSettingsDto,
    providers: List<ServerProviderDto>,
    projectId: String?,
    instanceId: String,
    model: String?,
): Boolean {
    if (isModelTurnedOffForProject(settings, providers, projectId, instanceId)) return true
    val provider = providers.firstOrNull { it.instanceId == instanceId }
    val driver = provider?.driver ?: settings.providerInstances[instanceId]?.v2String("driver")
    return driver == "antigravity" &&
        (provider == null ||
            !provider.installed ||
            provider.auth.status == "unauthenticated" ||
            provider.availability == "unavailable" ||
            provider.models.none { it.slug == model })
}

/** The catalog a composer scoped to [projectId] may offer. */
fun List<ProviderCatalogEntry>.forProject(projectId: String?): List<ProviderCatalogEntry> =
    filter { it.enabledFor(projectId) }

/** `describeUnavailableModelSelection`'s project branch, for a send that names a switched-off model. */
const val MODEL_TURNED_OFF_TITLE = "Model turned off for this project"
const val MODEL_TURNED_OFF_DETAIL = "Turn it back on from web or desktop, or choose another model."
