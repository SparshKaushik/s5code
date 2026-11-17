package club.touchtech.s5code.kotlin.feature.settings

import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ProviderOptionDescriptor
import club.touchtech.s5code.kotlin.model.ProviderOptionSelection
import club.touchtech.s5code.kotlin.model.ProviderOptionValue

/**
 * Model traits that live in the prompt rather than in stored options, ported
 * from `packages/client-runtime/src/modelTraits.ts`.
 *
 * Claude's `ultrathink` is a word in the message, not a setting: the provider
 * advertises it as a `promptInjectedValues` choice on its effort select, and
 * picking it writes an `Ultrathink:` prefix into the draft. Persisting it as an
 * option instead would hand the server a value it does not store.
 */

const val ULTRATHINK_PROMPT_PREFIX = "Ultrathink:\n"

/** `REASONING_EFFORT_OPTION_IDS` plus `thinking`: the selects a speed label joins. */
private val REASONING_SELECT_DESCRIPTOR_IDS =
    setOf("reasoningEffort", "effort", "reasoning", "variant", "reasoning_effort", "thinking")

private val ULTRATHINK_WORD = Regex("""\bultrathink\b""", RegexOption.IGNORE_CASE)
private val ULTRATHINK_PREFIX = Regex("""^Ultrathink:\s*""", RegexOption.IGNORE_CASE)
private val SLASH_COMMAND = Regex("""^/[^\s/]+(?:\s|$)""")

/** `isClaudeUltrathinkPrompt`. */
fun isUltrathinkPrompt(text: String): Boolean = ULTRATHINK_WORD.containsMatchIn(text)

/**
 * `applyClaudePromptEffortPrefix` for `ultrathink`. A slash command is left
 * alone: prefixing it turns it into prose and the provider never runs it.
 */
fun applyUltrathinkPromptPrefix(text: String): String {
    val trimmed = text.trim()
    if (trimmed.isEmpty() || SLASH_COMMAND.containsMatchIn(trimmed)) return trimmed
    if (trimmed.startsWith("Ultrathink:")) return trimmed
    return ULTRATHINK_PROMPT_PREFIX + trimmed
}

/** `getPromptControlledTraits`. */
data class PromptControlledTraits(
    val primarySelect: ProviderOptionDescriptor.Select?,
    /** The draft says ultrathink, so the primary select reads "Ultrathink". */
    val ultrathinkPromptControlled: Boolean,
    /** The word is in the body, not just the prefix: only editing the text changes it. */
    val ultrathinkInBodyText: Boolean,
) {
    fun controls(descriptor: ProviderOptionDescriptor): Boolean =
        ultrathinkPromptControlled && descriptor.id == primarySelect?.id
}

fun promptControlledTraits(
    descriptors: List<ProviderOptionDescriptor>,
    prompt: String,
): PromptControlledTraits {
    val primary = descriptors.firstOrNull { it is ProviderOptionDescriptor.Select } as ProviderOptionDescriptor.Select?
    val controlled = primary?.promptInjectedValues?.isNotEmpty() == true && isUltrathinkPrompt(prompt)
    return PromptControlledTraits(
        primarySelect = primary,
        ultrathinkPromptControlled = controlled,
        ultrathinkInBodyText = controlled && isUltrathinkPrompt(prompt.replace(ULTRATHINK_PREFIX, "")),
    )
}

/**
 * The result of picking a select choice: the draft text to write and, unless the
 * choice lives in the prompt, the options to persist.
 */
data class TraitSelectChange(
    val prompt: String,
    val options: List<ProviderOptionSelection>? = null,
)

/**
 * `getTraitSelectChange`. Null when the pick is refused: an `ultrathink` in the
 * body locks the primary select, and a value the model no longer lists is stale.
 */
fun traitSelectChange(
    descriptors: List<ProviderOptionDescriptor>,
    descriptor: ProviderOptionDescriptor.Select,
    prompt: String,
    value: String,
): TraitSelectChange? {
    if (value.isEmpty()) return null
    if (value in descriptor.promptInjectedValues) {
        return TraitSelectChange(
            if (prompt.isBlank()) ULTRATHINK_PROMPT_PREFIX else applyUltrathinkPromptPrefix(prompt),
        )
    }
    val traits = promptControlledTraits(descriptors, prompt)
    val primary = descriptor.id == traits.primarySelect?.id
    if (primary && traits.ultrathinkInBodyText) return null
    val options = applyProviderOption(descriptors, descriptor.id, ProviderOptionValue.Text(value)) ?: return null
    return TraitSelectChange(
        prompt = if (primary && traits.ultrathinkPromptControlled) prompt.replace(ULTRATHINK_PREFIX, "") else prompt,
        options = options,
    )
}

/**
 * `buildTraitsTriggerDisplay`: the short "High · Fast" summary beside the model
 * on the composer's model control. A speed setting joins the reasoning label
 * rather than taking a slot of its own, and stands alone only when it is the
 * one thing the model offers.
 */
fun traitsSummaryLabel(
    provider: ProviderInstance,
    descriptors: List<ProviderOptionDescriptor>,
    traits: PromptControlledTraits,
): String {
    var fastModeFallback: String? = null
    var speedLabel: String? = null
    var reasoningIndex = -1
    val labels = mutableListOf<String>()
    for (descriptor in descriptors) {
        if (descriptor.id == "fastMode" && descriptor is ProviderOptionDescriptor.Toggle) {
            speedLabel = if (descriptor.currentValue) "Fast" else null
            fastModeFallback = speedLabel ?: "Normal"
            continue
        }
        if (provider.driver == "codex" && descriptor.id == "serviceTier" &&
            descriptor is ProviderOptionDescriptor.Select
        ) {
            val current = descriptor.effectiveValue
            val fast = descriptor.options.firstOrNull { it.label == "Fast" }
            val ultrafast = descriptor.options.firstOrNull { it.label == "Ultrafast" }
            if (((fast != null || ultrafast != null) && current == "default") ||
                (fast != null && current == fast.id) ||
                (ultrafast != null && current == ultrafast.id)
            ) {
                speedLabel =
                    when {
                        ultrafast != null && current == ultrafast.id -> "Ultrafast"
                        fast != null && current == fast.id -> "Fast"
                        else -> null
                    }
                fastModeFallback = descriptor.options.firstOrNull { it.id == current }?.label ?: "Normal"
                continue
            }
        }
        val label =
            when {
                traits.controls(descriptor) -> "Ultrathink"
                descriptor is ProviderOptionDescriptor.Toggle ->
                    "${descriptor.label} ${if (descriptor.currentValue) "On" else "Off"}"
                descriptor is ProviderOptionDescriptor.Select -> descriptor.effectiveLabel
                else -> null
            }
        if (!label.isNullOrEmpty()) {
            if (reasoningIndex == -1 && descriptor is ProviderOptionDescriptor.Select &&
                descriptor.id in REASONING_SELECT_DESCRIPTOR_IDS
            ) {
                reasoningIndex = labels.size
            }
            labels += label
        }
    }
    if (labels.isEmpty() && fastModeFallback != null) return fastModeFallback
    speedLabel?.let { speed ->
        if (reasoningIndex >= 0) labels[reasoningIndex] = "${labels[reasoningIndex]} $speed" else labels += speed
    }
    return labels.joinToString(" · ")
}

/** The composer's trait summary for [settings], read against the draft [prompt]. */
fun composerTraitsSummary(
    catalog: List<club.touchtech.s5code.kotlin.model.ProviderCatalogEntry>,
    settings: club.touchtech.s5code.kotlin.model.ThreadSettings,
    prompt: String,
): String {
    val descriptors = providerOptionDescriptors(catalog, settings.provider, settings.model, settings.options)
    return traitsSummaryLabel(settings.provider, descriptors, promptControlledTraits(descriptors, prompt))
}
