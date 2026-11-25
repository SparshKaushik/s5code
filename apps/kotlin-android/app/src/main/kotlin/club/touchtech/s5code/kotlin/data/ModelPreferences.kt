package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ModelFavorite
import club.touchtech.s5code.kotlin.transport.wire.ProviderModelPreferencesDto
import club.touchtech.s5code.kotlin.transport.wire.ServerConfigSettingsDto

internal fun ServerConfigSettingsDto.modelFavorites(legacy: List<ModelFavorite>): List<ModelFavorite> =
    favorites?.map { ModelFavorite(it.provider, it.model) } ?: legacy

internal fun orderedProviderModels(models: List<String>, preferences: ProviderModelPreferencesDto?): List<String> {
    val order = preferences?.modelOrder.orEmpty().withIndex().associate { it.value to it.index }
    return if (order.isEmpty()) models else models.sortedBy { order[it] ?: Int.MAX_VALUE }
}
