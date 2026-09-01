package club.touchtech.s5code.kotlin.feature.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.component.S5BottomSheet
import club.touchtech.s5code.kotlin.design.component.S5ConnectedButtonGroup
import club.touchtech.s5code.kotlin.design.component.S5ProviderAvatar
import club.touchtech.s5code.kotlin.design.component.S5SearchField
import club.touchtech.s5code.kotlin.design.component.S5SectionHeader
import club.touchtech.s5code.kotlin.design.component.S5SelectableRow
import club.touchtech.s5code.kotlin.design.component.S5SwitchRow
import club.touchtech.s5code.kotlin.design.component.rowPosition
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.ApprovalPolicy
import club.touchtech.s5code.kotlin.model.ProviderCatalogEntry
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ProviderOptionDescriptor
import club.touchtech.s5code.kotlin.model.ProviderOptionValue
import club.touchtech.s5code.kotlin.model.RuntimeMode
import club.touchtech.s5code.kotlin.model.ThreadSettings

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
            if (visible.any { it.instanceId == settings.provider.instanceId }) visible
            else visible + settings.provider
        }
    var query by remember { mutableStateOf("") }
    val groups =
        remember(catalog, settings.provider, query, searchScope) {
            modelSearchResults(catalog, settings.provider, query, searchScope)
        }
    // Every knob below the model is the server's to describe. See [ProviderOptions].
    val descriptors =
        remember(catalog, settings.provider, settings.model, settings.options) {
            providerOptionDescriptors(catalog, settings.provider, settings.model, settings.options)
        }
    fun change(id: String, value: ProviderOptionValue) {
        // Null means the catalog moved under the sheet; dropping the tap is better
        // than persisting a value the provider would refuse on the next turn.
        applyProviderOption(descriptors, id, value)?.let {
            onSettingsChange(settings.copy(options = it))
        }
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
        // Bounded so the sheet stops short of the top: the list is long and a
        // full-height sheet reads as a page, which is what this replaced.
        Column(
            Modifier.fillMaxWidth()
                .heightIn(max = 560.dp)
                .verticalScroll(rememberScrollState())
                .padding(bottom = S5Theme.spacing.large),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
        ) {
            S5SectionHeader("Agent")
            SheetGroup {
                instances.forEachIndexed { index, provider ->
                    S5SelectableRow(
                        label = provider.label,
                        selected = provider.instanceId == settings.provider.instanceId,
                        onClick = {
                            val models = modelsFor(provider)
                            onSettingsChange(
                                settings.copy(
                                    provider = provider,
                                    model = models.firstOrNull() ?: settings.model,
                                    // Options belong to a model, so they do not
                                    // survive the move. Carrying them over would
                                    // send the new provider an id it never advertised.
                                    options = emptyList(),
                                )
                            )
                        },
                        leading = { S5ProviderAvatar(provider, size = 28.dp) },
                        position = rowPosition(index, instances.size),
                    )
                }
            }

            S5SectionHeader("Model")
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
            if (groups.isEmpty() && query.isNotBlank()) {
                Box(Modifier.padding(S5Theme.spacing.gutter)) {
                    Text(
                        "No matching models",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            groups.forEach { group ->
                // Only labelled once the search spans more than the selected agent:
                // a single group's heading would repeat the agent row above it.
                if (groups.size > 1) S5SectionHeader(group.instance.label)
                SheetGroup {
                    group.models.forEachIndexed { index, model ->
                        S5SelectableRow(
                            label = model,
                            selected =
                                model == settings.model &&
                                    group.instance.instanceId == settings.provider.instanceId,
                            onClick = {
                                onSettingsChange(
                                    settings.copy(
                                        provider = group.instance,
                                        model = model,
                                        options =
                                            if (model == settings.model) settings.options
                                            else emptyList(),
                                    )
                                )
                            },
                            position = rowPosition(index, group.models.size),
                        )
                    }
                }
            }

            S5SectionHeader("Mode")
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                S5ConnectedButtonGroup(
                    options = RuntimeMode.entries,
                    selected = settings.runtimeMode,
                    onSelect = { onSettingsChange(settings.copy(runtimeMode = it)) },
                    label = { it.label },
                )
            }

            // No "Reasoning effort" heading of our own: the descriptor names itself,
            // and the set differs per model — Codex has reasoning and a service tier,
            // Claude has effort, fast mode and a context window, OpenCode has none.
            descriptors.forEach { descriptor ->
                when (descriptor) {
                    is ProviderOptionDescriptor.Select -> {
                        S5SectionHeader(descriptor.label)
                        // A short set stays a button group, which is one tap. A long
                        // one (Claude ships seven efforts) would shrink to unreadable
                        // slivers, so it becomes rows.
                        if (descriptor.options.size <= MAX_INLINE_OPTION_CHOICES) {
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
                        } else {
                            SheetGroup {
                                descriptor.options.forEachIndexed { index, choice ->
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
                    is ProviderOptionDescriptor.Toggle ->
                        SheetGroup {
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

            S5SectionHeader("Permissions")
            SheetGroup {
                ApprovalPolicy.entries.forEachIndexed { index, policy ->
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

@Composable
private fun SheetGroup(content: @Composable () -> Unit) {
    Column(
        Modifier.padding(horizontal = S5Theme.spacing.gutter),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        content()
    }
}

/**
 * Above this many choices a select becomes a list of rows rather than a connected
 * button group. Three fits a phone's width with readable labels; Claude's seven
 * efforts do not.
 */
private const val MAX_INLINE_OPTION_CHOICES = 3
