package club.touchtech.s5code.kotlin.feature.settings

import club.touchtech.s5code.kotlin.model.ProviderCatalogEntry
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ProviderOptionDescriptor
import club.touchtech.s5code.kotlin.model.ProviderOptionSelection
import club.touchtech.s5code.kotlin.model.ProviderOptionValue

/**
 * Resolving and applying the knobs a model advertises.
 *
 * Ported from `getProviderOptionDescriptors`,
 * `buildProviderOptionSelectionsFromDescriptors` and
 * `getProviderOptionCurrentLabel` in `packages/shared/src/model.ts`, plus
 * `resolveProviderOptionDescriptors` / `applyProviderOptionSelection` in
 * `apps/mobile/src/lib/providerOptions.ts`.
 *
 * The whole point is that nothing here knows what a reasoning effort is. Codex
 * advertises `reasoningEffort` and `serviceTier`, Claude advertises `effort`,
 * `fastMode` and `contextWindow`, OpenCode and Pi advertise nothing, and each
 * model within a provider differs. A picker with a fixed Low/Medium/High row
 * offers values half the models reject and hides the ones they have.
 */

/**
 * The descriptors to render for one model, with the thread's stored values folded
 * in.
 *
 * A stored value that no longer matches any advertised choice is dropped rather
 * than shown: the model changed, and offering a value the provider would refuse
 * is worse than falling back to its default.
 */
fun providerOptionDescriptors(
    catalog: List<ProviderCatalogEntry>,
    provider: ProviderInstance,
    model: String,
    selections: List<ProviderOptionSelection>,
): List<ProviderOptionDescriptor> {
    val advertised =
        catalog
            .firstOrNull { it.instance.instanceId == provider.instanceId }
            ?.optionDescriptors
            ?.get(model)
            .orEmpty()
    if (advertised.isEmpty()) return emptyList()
    return advertised.map { descriptor -> descriptor.withSelection(selections) }
}

private fun ProviderOptionDescriptor.withSelection(
    selections: List<ProviderOptionSelection>
): ProviderOptionDescriptor {
    val stored = selections.firstOrNull { it.id == id }?.value
    return when (this) {
        is ProviderOptionDescriptor.Select -> {
            val text = (stored as? ProviderOptionValue.Text)?.value
            // Only a value the model still lists survives; see the note above.
            if (text != null && options.any { it.id == text }) copy(currentValue = text) else this
        }
        is ProviderOptionDescriptor.Toggle -> {
            val flag = (stored as? ProviderOptionValue.Flag)?.value
            if (flag != null) copy(currentValue = flag) else this
        }
    }
}

/**
 * The selections to persist after one change, or null when the change does not
 * match an advertised descriptor or choice.
 *
 * Null rather than a best effort: an id or value the model does not advertise is
 * a stale UI acting on a catalog that has since changed, and writing it would
 * hand the server a value it refuses on the next turn.
 *
 * The result is rebuilt from *every* descriptor, not patched into the existing
 * list, so a value left at its advertised default is written explicitly. That is
 * what `buildProviderOptionSelectionsFromDescriptors` does, and it means a later
 * change to the model's default cannot silently move a thread that was already
 * configured.
 */
fun applyProviderOption(
    descriptors: List<ProviderOptionDescriptor>,
    id: String,
    value: ProviderOptionValue,
): List<ProviderOptionSelection>? {
    val target = descriptors.firstOrNull { it.id == id } ?: return null
    val accepted =
        when (target) {
            is ProviderOptionDescriptor.Select ->
                value is ProviderOptionValue.Text && target.options.any { it.id == value.value }
            is ProviderOptionDescriptor.Toggle -> value is ProviderOptionValue.Flag
        }
    if (!accepted) return null

    return descriptors.mapNotNull { descriptor ->
        val next = if (descriptor.id == id) value else descriptor.effectiveValue()
        next?.let { ProviderOptionSelection(descriptor.id, it) }
    }
}

/** The value in effect, as a selection value. Null for a select with no default. */
private fun ProviderOptionDescriptor.effectiveValue(): ProviderOptionValue? =
    when (this) {
        is ProviderOptionDescriptor.Select ->
            effectiveValue?.let(ProviderOptionValue::Text)
        is ProviderOptionDescriptor.Toggle -> ProviderOptionValue.Flag(currentValue)
    }

/**
 * The labels that summarize a thread's options on a trigger pill, matching
 * `providerOptionValueLabels`: a toggle contributes only when it is on, because
 * "Fast Mode: Off" on a pill is noise.
 */
fun providerOptionSummaryLabels(descriptors: List<ProviderOptionDescriptor>): List<String> =
    descriptors.mapNotNull { descriptor ->
        when (descriptor) {
            is ProviderOptionDescriptor.Select -> descriptor.effectiveLabel
            is ProviderOptionDescriptor.Toggle ->
                descriptor.label.takeIf { descriptor.currentValue }
        }
    }
