package club.touchtech.s5code.kotlin.data

/**
 * The name you would say out loud: "Opus 5.5" for "Claude Opus 5.5". Ports
 * `modelShortName` in `packages/provider-core` and `resolveModelShortName` in
 * `packages/client-runtime/src/providerClients.ts`. Null when the full name is
 * already the short one.
 */
fun resolveModelShortName(name: String, shortName: String?, driver: String?): String? =
    shortName?.takeIf { it.isNotEmpty() } ?: modelShortName(name, MODEL_FAMILIES_BY_DRIVER[driver].orEmpty())

fun modelShortName(name: String, nativeFamilies: List<String>): String? {
    val unprefixed = name.replaceFirst(VENDOR_PREFIX, "")
    val short = withoutFamily(unprefixed, nativeFamilies) ?: if (namesModel(unprefixed)) unprefixed else name
    return short.takeIf { it != name }
}

private val MODEL_FAMILIES_BY_DRIVER =
    mapOf(
        "codex" to listOf("GPT-", "GPT "),
        "claudeAgent" to listOf("Claude "),
        "cursor" to listOf("Grok "),
        "grok" to listOf("Grok "),
        "antigravity" to listOf("Gemini "),
        "muse" to listOf("Muse "),
    )

private val FAMILY_PREFIXES = listOf("Claude ", "GPT-", "GPT ", "Gemini ", "Muse ", "Grok ")

// Words that describe a variant within a family rather than name a model. Off the
// family's native provider there is no icon saying which family "4 Pro" belongs to.
private val VARIANT_WORDS =
    setOf(
        "pro", "flash", "mini", "max", "lite", "nano", "ultra", "plus", "fast", "turbo",
        "preview", "latest", "thinking", "reasoning", "build", "code", "image", "vision",
        "audio", "live", "custom", "tools", "exp", "experimental", "chat", "tts", "stt",
        "non", "transcribe",
    )

private val VERSION = Regex("^v?\\d+(?:\\.\\d+)*[a-z]?$", RegexOption.IGNORE_CASE)

private val VENDOR_PREFIX = Regex("^[A-Z][\\w.&\\- ]{0,24}: (?=\\S)")

private fun withoutFamily(name: String, nativeFamilies: List<String>): String? {
    val prefix = FAMILY_PREFIXES.firstOrNull { name.startsWith(it) } ?: return null
    val rest = name.substring(prefix.length).trim()
    val keep = if (prefix in nativeFamilies) nonVersionWords(rest).isNotEmpty() else namesModel(rest)
    if (!keep) return null
    return if (rest.any { it.isWhitespace() }) rest else rest.replace("-", " ")
}

private fun nonVersionWords(text: String): List<String> =
    text
        .replace(Regex("\\(.*?\\)"), " ")
        .split(Regex("[\\s-]+"))
        .filter { it.isNotEmpty() && !VERSION.matches(it) }

private fun namesModel(text: String): Boolean =
    nonVersionWords(text).any { it.lowercase() !in VARIANT_WORDS }
