package club.touchtech.s5code.kotlin.feature.settings

import club.touchtech.s5code.kotlin.model.ProviderCatalogEntry
import club.touchtech.s5code.kotlin.model.ProviderInstance

/**
 * How wide a model search reaches.
 *
 * A new task has no history to lose, so searching every configured agent is the
 * fast path to "whatever can do this". An existing thread is bound to its current
 * provider instance: sessions cannot move between harnesses, matching RN's
 * `threadProviderGroups` filtering. The scope therefore limits both model search
 * and the provider rows rendered by the settings sheet.
 */
enum class ModelSearchScope {
    AllProviders,
    ActiveProvider,
}

/** Models from one provider instance, as the picker renders them. */
data class ModelGroup(val instance: ProviderInstance, val models: List<String>)

/**
 * Match the terms a user can actually see in the picker. Ported from
 * `modelMatchesCatalogQuery` in
 * `apps/mobile/src/features/threads/thread-settings-sheet-state.ts`.
 */
fun modelMatchesQuery(model: String, providerLabel: String, query: String): Boolean {
    val needle = query.trim().lowercase()
    if (needle.isEmpty()) return true
    return model.lowercase().contains(needle) || providerLabel.lowercase().contains(needle)
}

/**
 * The model rows to show for a query.
 *
 * With no query this is just the selected agent's models, which is the whole list
 * in the common case of picking a different model for the agent you are already
 * using. A query widens it as far as [scope] allows, and the selected agent's
 * group always leads so the familiar names stay where the eye expects them.
 *
 * An empty return with a non-blank query means "no matches" and is a real state
 * the caller should render; an empty return with a blank query means the catalog
 * has not arrived yet.
 */
fun modelSearchResults(
    catalog: List<ProviderCatalogEntry>,
    selected: ProviderInstance,
    query: String,
    scope: ModelSearchScope,
): List<ModelGroup> {
    val needle = query.trim()
    val selectedEntry = catalog.firstOrNull { it.instance.instanceId == selected.instanceId }
    if (needle.isEmpty()) {
        val models = selectedEntry?.models.orEmpty()
        return if (models.isEmpty()) emptyList() else listOf(ModelGroup(selected, models))
    }

    val searchable =
        when (scope) {
            ModelSearchScope.AllProviders -> catalog
            ModelSearchScope.ActiveProvider -> listOfNotNull(selectedEntry)
        }
    return searchable
        .sortedBy { if (it.instance.instanceId == selected.instanceId) 0 else 1 }
        .mapNotNull { entry ->
            val matches = entry.models.filter { modelMatchesQuery(it, entry.instance.label, needle) }
            if (matches.isEmpty()) null else ModelGroup(entry.instance, matches)
        }
}
