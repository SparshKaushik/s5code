package club.touchtech.s5code.kotlin.feature.settings

import androidx.compose.foundation.interaction.DragInteraction
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Star
import androidx.compose.material.icons.rounded.StarOutline
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.component.S5BottomSheet
import club.touchtech.s5code.kotlin.design.component.S5ConnectedButtonGroup
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5ProviderAvatar
import club.touchtech.s5code.kotlin.design.component.S5SearchField
import club.touchtech.s5code.kotlin.design.component.S5SectionHeader
import club.touchtech.s5code.kotlin.design.component.S5SelectableRow
import club.touchtech.s5code.kotlin.design.component.S5SwitchRow
import club.touchtech.s5code.kotlin.design.component.rowPosition
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.ApprovalPolicy
import club.touchtech.s5code.kotlin.model.ModelFavorite
import club.touchtech.s5code.kotlin.model.ProviderCatalogEntry
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ProviderOptionDescriptor
import club.touchtech.s5code.kotlin.model.ProviderOptionValue
import club.touchtech.s5code.kotlin.model.RuntimeMode
import club.touchtech.s5code.kotlin.model.ThreadSettings
import kotlinx.coroutines.launch

/**
 * Provider, model, mode, effort, and permissions in a modal sheet.
 *
 * This is a sheet rather than a destination because it is always opened from
 * something you are in the middle of: a draft you are writing, or a thread you
 * are reading. Pushing a page to change a model loses that place and costs two
 * transitions to get back to it.
 *
 * The sheet is stateless about *where* the settings live. The new-task draft and
 * a live thread both pass their effective [ThreadSettings] and an update callback.
 * Existing threads stage the change in their composer draft, matching RN, and
 * synchronize it when the next turn is sent.
 *
 * [catalog] is the server's own list of usable provider instances, not a
 * hardcoded set of drivers. That is the difference between offering the agents a
 * user actually has and offering five names, four of which fail on send. The rows
 * below the model come from the same place: each model advertises its own option
 * descriptors, so the sheet renders what this model has rather than a fixed
 * effort row.
 *
 * [searchScope] widens or narrows the *search*, never the agent list. See
 * [ModelSearchScope].
 */
@Composable
fun TaskSettingsSheet(
    settings: ThreadSettings,
    catalog: List<ProviderCatalogEntry>,
    modelsFor: (ProviderInstance) -> List<String>,
    onSettingsChange: (ThreadSettings) -> Unit,
    onDismiss: () -> Unit,
    title: String = "Model and settings",
    searchScope: ModelSearchScope = ModelSearchScope.ActiveProvider,
    favorites: List<ModelFavorite> = emptyList(),
    onToggleFavorite: (ModelFavorite) -> Unit = {},
    catalogRefreshing: Boolean = false,
    onRefreshCatalog: () -> Unit = {},
) {
    // The thread's own instance always has a row, even when no connected server
    // lists it: a thread bound to an instance the user removed still has to show
    // what it is running, and hiding the selection would read as a blank picker.
    val instances =
        remember(catalog, settings.provider, searchScope) {
            val listed = catalog.map { it.instance }
            val visible =
                when (searchScope) {
                    ModelSearchScope.AllProviders -> listed
                    // RN binds an existing thread to its current provider
                    // instance; sessions cannot move between harnesses.
                    ModelSearchScope.ActiveProvider ->
                        listed.filter { it.instanceId == settings.provider.instanceId }
                }
            if (visible.any { it.instanceId == settings.provider.instanceId }) {
                visible
            } else if (searchScope == ModelSearchScope.ActiveProvider || visible.isEmpty()) {
                visible + settings.provider
            } else {
                visible
            }
        }

    val activeProvider =
        remember(instances, settings.provider) {
            if (instances.any { it.instanceId == settings.provider.instanceId }) {
                settings.provider
            } else {
                instances.firstOrNull() ?: settings.provider
            }
        }

    LaunchedEffect(activeProvider) {
        if (activeProvider.instanceId != settings.provider.instanceId) {
            val models = modelsFor(activeProvider)
            onSettingsChange(
                settings.copy(
                    provider = activeProvider,
                    model = models.firstOrNull() ?: settings.model,
                    options = emptyList(),
                )
            )
        }
    }

    var query by remember { mutableStateOf("") }
    // Rows and searches both resolve display names through the catalog, so the
    // picker never shows a raw slug while the wire key stays the slug.
    val catalogById =
        remember(catalog) { catalog.associateBy { it.instance.instanceId } }
    val groups =
        remember(catalog, settings.provider, query, searchScope) {
            modelSearchResults(catalog, settings.provider, query, searchScope)
        }
    // Every knob below the model is the server's to describe. See [ProviderOptions].
    val descriptors =
        remember(catalog, settings.provider, settings.model, settings.options) {
            providerOptionDescriptors(catalog, settings.provider, settings.model, settings.options)
        }

    // Starred rows resolve against the catalog so a favorite on an instance this
    // environment no longer offers does not render a dead row. Under
    // ActiveProvider only the bound instance's favorites show — picking another
    // agent mid-thread is not a move the turn start would accept anyway.
    val favoriteRows =
        remember(favorites, catalog, searchScope, settings.provider, query) {
            favorites
                .asSequence()
                .mapNotNull { favorite ->
                    val entry =
                        catalog.firstOrNull { it.instance.instanceId == favorite.instanceId }
                            ?: return@mapNotNull null
                    if (favorite.model !in entry.models) return@mapNotNull null
                    if (searchScope == ModelSearchScope.ActiveProvider &&
                        entry.instance.instanceId != settings.provider.instanceId
                    ) {
                        return@mapNotNull null
                    }
                    if (
                        !modelMatchesQuery(
                            favorite.model,
                            entry.instance.label,
                            query,
                            label = entry.modelLabel(favorite.model),
                        )
                    ) {
                        return@mapNotNull null
                    }
                    entry.instance to favorite.model
                }
                .toList()
        }
    val favoriteKeys =
        remember(favorites) { favorites.mapTo(mutableSetOf()) { it.instanceId to it.model } }

    fun change(id: String, value: ProviderOptionValue) {
        // Null means the catalog moved under the sheet; dropping the tap is better
        // than persisting a value the provider would refuse on the next turn.
        applyProviderOption(descriptors, id, value)?.let {
            onSettingsChange(settings.copy(options = it))
        }
    }

    fun select(provider: ProviderInstance, model: String) {
        onSettingsChange(
            settings.copy(
                provider = provider,
                model = model,
                options = if (model == settings.model) settings.options else emptyList(),
            )
        )
    }

    val listState = rememberLazyListState()
    val coroutineScope = rememberCoroutineScope()
    val keyboard = LocalSoftwareKeyboardController.current

    // Same contract as the RN list (`keyboardDismissMode="on-drag"`): a real
    // pull on the list puts the keyboard away; the programmatic scroll after a
    // selection is not a drag, so it never trips this.
    LaunchedEffect(listState) {
        listState.interactionSource.interactions.collect { interaction ->
            if (interaction is DragInteraction.Start) keyboard?.hide()
        }
    }

    // The rows below the model list (mode, reasoning, permissions) sit past the
    // fold. After picking a model the sheet scrolls them into view instead of
    // leaving the user to discover them; the count bookkeeping matches the
    // LazyColumn content below exactly.
    val hasNoMatchRow = groups.isEmpty() && query.isNotBlank()
    var modelRowCount = 0
    groups.forEach { group ->
        modelRowCount += group.models.size + if (groups.size > 1) 1 else 0
    }
    val favoritesItemCount = if (favoriteRows.isEmpty()) 0 else favoriteRows.size + 1
    val modeHeaderIndex =
        favoritesItemCount +
            1 + // header_agent
            instances.size +
            1 + // header_model
            1 + // search_field
            (if (hasNoMatchRow) 1 else 0) +
            modelRowCount

    fun scrollToModeSection() {
        coroutineScope.launch {
            val visible = listState.layoutInfo.visibleItemsInfo
            if (visible.any { it.key == "header_mode" }) return@launch
            if (modeHeaderIndex >= listState.layoutInfo.totalItemsCount) return@launch
            listState.animateScrollToItem(modeHeaderIndex)
        }
    }

    fun selectAndReveal(provider: ProviderInstance, model: String) {
        keyboard?.hide()
        select(provider, model)
        scrollToModeSection()
    }

    S5BottomSheet(
        onDismiss = onDismiss,
        title = title,
        // The subtitle carries what is in effect, so the values a knob is set to are
        // visible before scrolling to its row.
        subtitle =
            (listOf(settings.provider.label) + providerOptionSummaryLabels(descriptors))
                .joinToString(" · "),
    ) {
        // weight(fill = false) keeps the list shrinkable: without it the fixed
        // cap plus the sections below can overflow the space the keyboard leaves,
        // and the overflow is what made model rows unreachable while typing.
        LazyColumn(
            state = listState,
            modifier =
                Modifier.fillMaxWidth()
                    .heightIn(max = 560.dp)
                    .weight(1f, fill = false),
            contentPadding = PaddingValues(bottom = S5Theme.spacing.large),
            verticalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            if (favoriteRows.isNotEmpty()) {
                item(key = "header_favorites", contentType = "header") {
                    S5SectionHeader("Favorites")
                }
                itemsIndexed(
                    items = favoriteRows,
                    key = { _, (instance, model) -> "fav_${instance.instanceId}_$model" },
                    contentType = { _, _ -> "model_row" },
                ) { index, (instance, model) ->
                    Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                        S5SelectableRow(
                            label =
                                catalogById[instance.instanceId]?.modelLabel(model) ?: model,
                            supporting = instance.label,
                            selected =
                                model == settings.model &&
                                    instance.instanceId == settings.provider.instanceId,
                            onClick = { selectAndReveal(instance, model) },
                            leading = { S5ProviderAvatar(instance, size = 28.dp) },
                            trailing = {
                                FavoriteToggle(
                                    favorited = true,
                                    onClick = {
                                        onToggleFavorite(ModelFavorite(instance.instanceId, model))
                                    },
                                )
                            },
                            position = rowPosition(index, favoriteRows.size),
                        )
                    }
                }
            }

            item(key = "header_agent", contentType = "header") {
                S5SectionHeader("Agent")
            }
            itemsIndexed(
                items = instances,
                key = { _, provider -> "agent_${provider.instanceId}" },
                contentType = { _, _ -> "agent_row" },
            ) { index, provider ->
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5SelectableRow(
                        label = provider.label,
                        selected = provider.instanceId == settings.provider.instanceId,
                        onClick = {
                            keyboard?.hide()
                            val models = modelsFor(provider)
                            select(
                                provider,
                                models.firstOrNull() ?: settings.model,
                            )
                            scrollToModeSection()
                        },
                        leading = { S5ProviderAvatar(provider, size = 28.dp) },
                        position = rowPosition(index, instances.size),
                    )
                }
            }

            item(key = "header_model", contentType = "header") {
                S5SectionHeader(
                    "Model",
                    trailing = {
                        S5IconButton(
                            icon = Icons.Rounded.Refresh,
                            label = "Refresh model list",
                            onClick = onRefreshCatalog,
                            enabled = !catalogRefreshing,
                            compact = true,
                        )
                    },
                )
            }
            item(key = "search_field", contentType = "search") {
                Box(
                    Modifier.padding(
                        horizontal = S5Theme.spacing.gutter,
                        vertical = S5Theme.spacing.tiny,
                    )
                ) {
                    S5SearchField(
                        value = query,
                        onValueChange = { query = it },
                        placeholder =
                            when (searchScope) {
                                ModelSearchScope.AllProviders -> "Find a model"
                                ModelSearchScope.ActiveProvider ->
                                    "Find a ${settings.provider.label} model"
                            },
                    )
                }
            }
            if (hasNoMatchRow) {
                item(key = "no_matching_models", contentType = "empty") {
                    Box(Modifier.padding(S5Theme.spacing.gutter)) {
                        Text(
                            "No matching models",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
            groups.forEach { group ->
                // Only labelled once the search spans more than the selected agent:
                // a single group's heading would repeat the agent row above it.
                if (groups.size > 1) {
                    item(key = "header_group_${group.instance.instanceId}", contentType = "header") {
                        S5SectionHeader(group.instance.label)
                    }
                }
                itemsIndexed(
                    items = group.models,
                    key = { _, model -> "model_${group.instance.instanceId}_$model" },
                    contentType = { _, _ -> "model_row" },
                ) { index, model ->
                    Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                        S5SelectableRow(
                            label =
                                catalogById[group.instance.instanceId]?.modelLabel(model)
                                    ?: model,
                            supporting =
                                if (groups.size > 1) group.instance.label else null,
                            selected =
                                model == settings.model &&
                                    group.instance.instanceId == settings.provider.instanceId,
                            onClick = { selectAndReveal(group.instance, model) },
                            trailing = {
                                FavoriteToggle(
                                    favorited =
                                        (group.instance.instanceId to model) in favoriteKeys,
                                    onClick = {
                                        onToggleFavorite(
                                            ModelFavorite(group.instance.instanceId, model)
                                        )
                                    },
                                )
                            },
                            position = rowPosition(index, group.models.size),
                        )
                    }
                }
            }

            item(key = "header_mode", contentType = "header") {
                S5SectionHeader("Mode")
            }
            item(key = "mode_picker", contentType = "mode") {
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5ConnectedButtonGroup(
                        options = RuntimeMode.entries,
                        selected = settings.runtimeMode,
                        onSelect = { onSettingsChange(settings.copy(runtimeMode = it)) },
                        label = { it.label },
                    )
                }
            }

            // No "Reasoning effort" heading of our own: the descriptor names itself,
            // and the set differs per model — Codex has reasoning and a service tier,
            // Claude has effort, fast mode and a context window, OpenCode has none.
            descriptors.forEach { descriptor ->
                when (descriptor) {
                    is ProviderOptionDescriptor.Select -> {
                        item(key = "header_desc_${descriptor.id}", contentType = "header") {
                            S5SectionHeader(descriptor.label)
                        }
                        // A short set stays a button group, which is one tap. A long
                        // one (Claude ships seven efforts) would shrink to unreadable
                        // slivers, so it becomes rows.
                        if (descriptor.options.size <= MAX_INLINE_OPTION_CHOICES) {
                            item(key = "desc_btn_${descriptor.id}", contentType = "desc_btn") {
                                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                                    S5ConnectedButtonGroup(
                                        options = descriptor.options,
                                        selected =
                                            descriptor.options.firstOrNull {
                                                it.id == descriptor.effectiveValue
                                            } ?: descriptor.options.first(),
                                        onSelect = {
                                            change(descriptor.id, ProviderOptionValue.Text(it.id))
                                        },
                                        label = { it.label },
                                    )
                                }
                            }
                        } else {
                            itemsIndexed(
                                items = descriptor.options,
                                key = { _, choice -> "desc_choice_${descriptor.id}_${choice.id}" },
                                contentType = { _, _ -> "desc_choice" },
                            ) { index, choice ->
                                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                                    S5SelectableRow(
                                        label = choice.label,
                                        supporting = choice.description,
                                        selected = choice.id == descriptor.effectiveValue,
                                        onClick = {
                                            change(
                                                descriptor.id,
                                                ProviderOptionValue.Text(choice.id),
                                            )
                                        },
                                        position = rowPosition(index, descriptor.options.size),
                                    )
                                }
                            }
                        }
                    }
                    is ProviderOptionDescriptor.Toggle -> {
                        item(key = "desc_toggle_${descriptor.id}", contentType = "desc_toggle") {
                            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                                S5SwitchRow(
                                    icon = null,
                                    label = descriptor.label,
                                    supporting = descriptor.description,
                                    checked = descriptor.currentValue,
                                    onCheckedChange = {
                                        change(descriptor.id, ProviderOptionValue.Flag(it))
                                    },
                                )
                            }
                        }
                    }
                }
            }

            item(key = "header_permissions", contentType = "header") {
                S5SectionHeader("Permissions")
            }
            itemsIndexed(
                items = ApprovalPolicy.entries,
                key = { _, policy -> "policy_${policy.name}" },
                contentType = { _, _ -> "policy_row" },
            ) { index, policy ->
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5SelectableRow(
                        label = policy.label,
                        supporting =
                            when (policy) {
                                ApprovalPolicy.Ask -> "Every command and write needs approval"
                                ApprovalPolicy.AutoEdit -> "File edits run; commands still ask"
                                ApprovalPolicy.Full -> "No approval prompts in this thread"
                            },
                        selected = policy == settings.approvalPolicy,
                        onClick = { onSettingsChange(settings.copy(approvalPolicy = policy)) },
                        position = rowPosition(index, ApprovalPolicy.entries.size),
                    )
                }
            }
        }
    }
}

/**
 * The star toggle on a model row. Drawn directly rather than through
 * [S5IconButton] because a tooltip on every row in a picker is noise, and the
 * compact size keeps it inside the row's own tap target instead of growing the
 * row to fit a button.
 */
@Composable
private fun FavoriteToggle(favorited: Boolean, onClick: () -> Unit) {
    IconButton(onClick = onClick) {
        Icon(
            if (favorited) Icons.Rounded.Star else Icons.Rounded.StarOutline,
            contentDescription = if (favorited) "Remove from favorites" else "Add to favorites",
            tint = if (favorited) FavoriteStarColor else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/** The same yellow the web picker fills its star with. */
private val FavoriteStarColor = Color(0xFFEAB308)

/**
 * Above this many choices a select becomes a list of rows rather than a connected
 * button group. Three fits a phone's width with readable labels; Claude's seven
 * efforts do not.
 */
private const val MAX_INLINE_OPTION_CHOICES = 3
