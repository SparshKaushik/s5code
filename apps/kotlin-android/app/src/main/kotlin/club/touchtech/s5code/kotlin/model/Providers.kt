package club.touchtech.s5code.kotlin.model

/**
 * A configured provider instance, as the UI names it.
 *
 * This is deliberately not an enum. `ProviderDriverKind` in
 * `packages/contracts/src/providerInstance.ts` is an *open* branded slug: the
 * server ships whatever drivers its build knows about, and a fork can add one.
 * A closed enum here has exactly one failure mode, and it is the one that shipped
 * — every unrecognised slug collapsed onto the first entry, so `pi` threads were
 * labeled "Codex".
 *
 * [instanceId] is the routing key, and the only identity the server accepts for
 * turn starts. [driver] is metadata: it picks the glyph and the fallback label,
 * and nothing routes on it, because two instances of the same CLI share a driver.
 */
data class ProviderInstance(
    val instanceId: String,
    val driver: String,
    /** The user's own name for this instance, when they set one. */
    val displayName: String? = null,
) {
    val label: String
        get() = formatProviderInstanceLabel(instanceId, driver, displayName)

    companion object {
        /**
         * The instance every server ships, used as the pre-connection default so a
         * draft opened before any config arrives has something coherent to show.
         */
        val Default = ProviderInstance(instanceId = "codex", driver = "codex")
    }
}

/**
 * Driver slugs whose display name is not just a title-cased slug. Ported from
 * `packages/shared/src/providerLabels.ts`, and kept in the same shape so the two
 * tables can be diffed by eye when a driver is added.
 */
private val DRIVER_DISPLAY_NAMES =
    mapOf(
        "codex" to "Codex",
        "claudeAgent" to "Claude",
        // Threads persisted before the driver rename still carry the short form.
        "claude" to "Claude",
        "cursor" to "Cursor",
        "grok" to "Grok",
        "opencode" to "OpenCode",
        "antigravity" to "Antigravity",
        // pi brands itself lowercase; title-casing would render "Pi".
        "pi" to "pi",
    )

/** Fallback name for a driver with no entry in the table. */
private const val UNKNOWN_DRIVER_NAME = "This agent"

/** Human-readable name for a provider driver kind. */
fun formatProviderDriverName(driver: String?): String {
    if (driver.isNullOrBlank()) return UNKNOWN_DRIVER_NAME
    DRIVER_DISPLAY_NAMES[driver]?.let { return it }
    // Title-case unknown driver kinds so a fork's driver still reads reasonably.
    val trimmed = driver.replace(Regex("Agent$", RegexOption.IGNORE_CASE), "").trim()
    if (trimmed.isEmpty()) return driver
    return trimmed.replaceFirstChar { it.uppercaseChar() }
}

/**
 * Label for a configured provider instance: the user's name wins, then the
 * driver's, then the instance id. The id is unique but not descriptive, so it
 * only surfaces for a driver this build has never heard of.
 */
fun formatProviderInstanceLabel(instanceId: String, driver: String, displayName: String?): String {
    displayName?.trim()?.takeIf { it.isNotEmpty() }?.let { return it }
    val driverName = formatProviderDriverName(driver)
    return if (driverName == UNKNOWN_DRIVER_NAME) instanceId else driverName
}
