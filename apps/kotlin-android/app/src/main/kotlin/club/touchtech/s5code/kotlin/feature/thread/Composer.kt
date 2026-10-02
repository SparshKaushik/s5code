package club.touchtech.s5code.kotlin.feature.thread

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.CallSplit
import androidx.compose.material.icons.automirrored.rounded.Send
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.AlternateEmail
import androidx.compose.material.icons.rounded.AutoAwesome
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.ExpandMore
import androidx.compose.material.icons.rounded.Smartphone
import androidx.compose.material.icons.rounded.Sync
import androidx.compose.material.icons.rounded.Stop
import androidx.compose.material.icons.rounded.Terminal
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.component.S5AttachmentPreviewDialog
import club.touchtech.s5code.kotlin.design.component.S5AttachmentStrip
import club.touchtech.s5code.kotlin.design.component.S5ComposerAction
import club.touchtech.s5code.kotlin.design.component.S5ComposerControl
import club.touchtech.s5code.kotlin.design.component.S5ComposerField
import club.touchtech.s5code.kotlin.design.component.S5ComposerSurface
import club.touchtech.s5code.kotlin.design.component.S5ComposerToolbarRow
import club.touchtech.s5code.kotlin.design.component.S5ProviderAvatar
import club.touchtech.s5code.kotlin.design.component.rememberDraftTextFieldState
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.ComposerImageCandidate
import club.touchtech.s5code.kotlin.model.ComposerPullRequestCandidate
import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ProviderSkill
import club.touchtech.s5code.kotlin.model.RuntimeMode
import club.touchtech.s5code.kotlin.model.SlashCommand
import club.touchtech.s5code.kotlin.model.ThreadSyncPhase
import kotlinx.coroutines.delay
import club.touchtech.s5code.kotlin.data.pastedTextShouldFold
import club.touchtech.s5code.kotlin.platform.active
import club.touchtech.s5code.kotlin.platform.composerImageReceiver
import club.touchtech.s5code.kotlin.platform.composerPastedTextReceiver

/** Radius of the collapsed pill. Half the collapsed height, so it is fully round. */
private val COLLAPSED_RADIUS = 24.dp

/** Radius of the expanded card. */
private val EXPANDED_RADIUS = 26.dp

/**
 * Line cap for the expanded field, matching the RN composer's 160px scroll cap:
 * past this the field scrolls rather than pushing the transcript off screen.
 */
private const val EXPANDED_MAX_LINES = 6

/**
 * Thread composer, matching the RN client's two-state shape: a single-line pill
 * with just a send target while idle, expanding into a card with the attach
 * control, model picker, and send/stop along its bottom edge once focused.
 *
 * There is exactly one [S5ComposerField] call site, and it keeps the same parent
 * in both states. That is load-bearing rather than tidiness: branching the layout
 * would put the field at a different position in the composition, so taking focus
 * would destroy the node that had just been focused, drop the keyboard, and
 * collapse the composer again. Only the surrounding chrome and the padding
 * change between states.
 *
 * Images arrive through [onAddImages]: the keyboard's own paste/insert gesture and
 * drops (via [composerImageReceiver]), and the picker. There is deliberately no
 * paste button — every Android keyboard already offers paste, and a second one on
 * the toolbar spent a permanent slot on a gesture the user has.
 */
@Composable
fun ThreadComposer(
    value: String,
    /** Slash commands the thread's provider advertises. */
    commands: List<SlashCommand>,
    /**
     * Skills the provider resolved for the thread's cwd (`$` trigger plus the
     * `skill:` rows inside the slash menu), already filtered to the ones a user
     * may invoke and de-duplicated by name.
     */
    skills: List<ProviderSkill> = emptyList(),
    /** Resolves `@` mentions to workspace paths. Suspend: it hits the file index. */
    onSearchPaths: suspend (String) -> List<String>,
    /**
     * Resolves a `#` token to pull requests in the thread's repository. Null
     * when the server has no `pullRequests` capability or the project has no
     * repository — the trigger then behaves like plain text.
     */
    onSearchPullRequests: (suspend (String) -> List<ComposerPullRequestCandidate>)? = null,
    /**
     * A `#` row was tapped: splice `formatComposerContextReference` over the
     * `[rangeStart, rangeEnd)` token and store the record with the draft.
     */
    onPickPullRequest: (ComposerPullRequestCandidate, Int, Int) -> Unit = { _, _, _ -> },
    /**
     * Kicks `server.refreshProviders` for the thread's cwd when the provider has
     * no snapshot for it — the per-workspace skills list arrives a beat after
     * the composer first asks.
     */
    onRefreshWorkspaceSnapshot: () -> Unit = {},
    /**
     * `pastedTextDisposition`: whether a text paste may become an attachment —
     * the caller knows the draft's remaining slots and the server's upload
     * capability, neither of which the receiver can suspend to ask.
     */
    canAttachPastedText: () -> Boolean = { false },
    /** Receives clipboard text that was folded out of the field. */
    onPastedText: (String) -> Unit = {},
    /**
     * Whether the thread holds anything `/compact` could act on — RN hides the
     * row on an empty conversation.
     */
    hasCompactableConversation: Boolean = true,
    /**
     * The provider's `compatibilityAdvisory` line when it says the installed
     * agent is unsupported or broken — shown above the field, matching RN's
     * inline notice.
     */
    providerAdvisory: String? = null,
    /**
     * Limits has data for this provider (`hasProviderUsageLimits`), so T3 owns
     * the `/usage-limits` command: picking it — or sending the bare command
     * with no attachments — opens the limits card instead of a turn.
     */
    offersUsageLimits: Boolean = false,
    onUsageLimits: () -> Unit = {},
    /**
     * `devicePreviews`: how many devices this thread holds open. Nonzero shows
     * the preview button; the count is the accessibility label's plural.
     */
    devicePreviewCount: Int = 0,
    onOpenDevicePreview: () -> Unit = {},
    onValueChange: (String) -> Unit,
    onSend: () -> Unit,
    onCancel: () -> Unit,
    working: Boolean,
    attachments: List<ComposerAttachment>,
    onAddAttachment: () -> Unit,
    onAddImages: (List<ComposerImageCandidate>) -> Unit,
    onRemoveAttachment: (ComposerAttachment) -> Unit,
    queuedMessages: Int,
    connectionState: ConnectionState,
    modifier: Modifier = Modifier,
    connectionError: String? = null,
    environmentLabel: String,
    syncPhase: ThreadSyncPhase,
    onReconnect: () -> Unit,
    provider: ProviderInstance,
    /** Display name for the chip, resolved from the catalog — never the slug. */
    modelLabel: String,
    onOpenSettings: () -> Unit,
    /**
     * Whether the provider allows the plan/default mode switch, from
     * `showInteractionModeToggle`. Gates the built-in `/plan` and `/default`
     * suggestions, which swap the mode rather than sending text.
     */
    interactionModeAllowed: Boolean = true,
    /** Applies a `/plan` or `/default` pick to the staged thread settings. */
    onInteractionMode: ((RuntimeMode) -> Unit)? = null,
    /** Resets the field's own text state, e.g. per thread. */
    draftKey: Any?,
) {
    // `detectComposerTrigger`: the run of non-whitespace under the caret (end
    // of text here) classifies as a path, pull-request, skill, or slash token.
    val trigger = remember(value) { detectComposerTrigger(value) }
    // A stale live token — providers expand their own commands only at the very
    // start of a message, while T3's own act anywhere.
    val atMessageStart = trigger?.rangeStart == 0

    val commandSuggestions =
        remember(trigger, commands, skills, interactionModeAllowed, hasCompactableConversation) {
            if (trigger?.kind != ComposerTriggerKind.SlashCommand) {
                return@remember emptyList<ComposerSuggestion>()
            }
            val query = trigger.query.lowercase()
            buildList<ComposerSuggestion> {
                // T3's own commands act locally, ahead of provider ones.
                if ("model".contains(query)) {
                    // `/model` inserts literal text, matching mobile: the slash-model
                    // picker that follows is a web-only affordance.
                    add(ComposerSuggestion("/model", "Switch model", null, "/model "))
                }
                if (interactionModeAllowed) {
                    if ("plan".contains(query)) {
                        add(
                            ComposerSuggestion(
                                "/plan", "Switch to plan mode", null, "",
                                interactionMode = RuntimeMode.Plan,
                            )
                        )
                    }
                    if ("default".contains(query)) {
                        add(
                            ComposerSuggestion(
                                "/default", "Switch to default mode", null, "",
                                interactionMode = RuntimeMode.Default,
                            )
                        )
                    }
                }
                // Providers expand commands only at the start of a message.
                if (atMessageStart) {
                    // A skill of the same name shadows the provider's command —
                    // the skill wins its own dispatch form.
                    val skillNames = skills.mapTo(HashSet()) { it.name.trim().lowercase() }
                    val visible = commands.filter {
                        val name = it.name.removePrefix("/").lowercase()
                        name !in skillNames &&
                            !(name == "compact" && !hasCompactableConversation)
                    }
                    rankSlashCommands(visible, trigger.query).forEach { command ->
                        add(
                            ComposerSuggestion(
                                command.name, command.description,
                                Icons.Rounded.Terminal, "${command.name} ",
                                // The server injects its usage-limits row only
                                // for clients answering it; intercepting it here
                                // is what makes that answer local.
                                opensUsageLimits =
                                    offersUsageLimits &&
                                        command.name.removePrefix("/") == "usage-limits",
                            )
                        )
                    }
                }
                // `matchesSlashSkillQuery`: enabled skills whose name or
                // descriptions carry the query, listed as `skill:<name>`.
                val skillQuery =
                    when {
                        query == "skill" -> ""
                        query.startsWith("skill:") -> query.removePrefix("skill:")
                        else -> query
                    }
                skills
                    .filter { skill ->
                        skill.enabled &&
                            (
                                skillQuery.isEmpty() ||
                                    listOfNotNull(
                                        skill.name, skill.displayName,
                                        skill.shortDescription, skill.description,
                                    ).any { it.lowercase().contains(skillQuery) }
                            )
                    }
                    .take(20)
                    .forEach { skill ->
                        add(
                            ComposerSuggestion(
                                "skill:${skill.name}",
                                skill.shortDescription ?: skill.description.orEmpty(),
                                Icons.Rounded.AutoAwesome,
                                "\$${skill.name} ",
                            )
                        )
                    }
            }
        }

    val skillSuggestions =
        remember(trigger, skills) {
            if (trigger?.kind != ComposerTriggerKind.Skill) {
                return@remember emptyList<ComposerSuggestion>()
            }
            val query =
                trigger.query.trim()
                    .replace(Regex("""^[\p{Sc}]+"""), "")
                    .lowercase()
            if (query.isEmpty()) {
                return@remember skills.take(20).map { skill -> skill.suggestion() }
            }
            skills
                .mapNotNull { skill -> rankProviderSkill(skill, query) }
                .sortedWith(compareBy({ it.score }, { it.tieBreaker }))
                .take(20)
                .map { it.skill.suggestion() }
        }

    // Server round trips are debounced and cancel on the next keystroke rather
    // than firing per character.
    var pathSuggestions by remember { mutableStateOf(emptyList<String>()) }
    var pullRequestSuggestions by
        remember { mutableStateOf(emptyList<ComposerPullRequestCandidate>()) }
    var previewAttachment by remember { mutableStateOf<ComposerAttachment?>(null) }
    LaunchedEffect(trigger) {
        if (trigger?.kind != ComposerTriggerKind.Path) {
            pathSuggestions = emptyList()
            return@LaunchedEffect
        }
        delay(160)
        pathSuggestions =
            runCatching { onSearchPaths(trigger.query) }
                .map { rankComposerPaths(it, trigger.query) }
                .getOrDefault(emptyList())
    }
    LaunchedEffect(trigger) {
        val search = onSearchPullRequests
        if (trigger?.kind != ComposerTriggerKind.PullRequest || search == null) {
            pullRequestSuggestions = emptyList()
            return@LaunchedEffect
        }
        delay(180)
        pullRequestSuggestions =
            runCatching { search(trigger.query) }.getOrDefault(emptyList())
    }
    // The first composer visit for a workspace usually precedes its per-cwd
    // provider snapshot; one refresh per draft fills the skills list, with the
    // gateway's own cooldown covering a failure.
    val refreshSnapshot by rememberUpdatedState(onRefreshWorkspaceSnapshot)
    LaunchedEffect(draftKey) {
        refreshSnapshot()
    }
    // The field owns its text so keyboard content commits (Gboard clipboard,
    // stickers, GIFs) reach it; the draft outside still wins on external change.
    val draftState = rememberDraftTextFieldState(draftKey, value, onValueChange)
    // Dictation binds to the field, not the prop, so a transcript commits at the
    // caret. Null on devices with no speech service, and the mic stays hidden.
    val dictation = rememberDictation(draftKey, draftState)
    val voicePresented = dictation?.presentation?.statusLabel != null

    val interactionSource = remember { MutableInteractionSource() }
    val focused by interactionSource.collectIsFocusedAsState()
    // Attachments force the card open: thumbnails have nowhere to go in a pill,
    // and a paste that appeared to do nothing is worse than an expanded composer.
    // An active (or errored) dictation session does the same — its toolbar row
    // lives in the expanded chrome.
    val expanded = focused || attachments.isNotEmpty() || voicePresented
    val canSend = value.isNotBlank() || attachments.isNotEmpty()

    val spatial = MaterialTheme.motionScheme.fastSpatialSpec<Dp>()
    val spatialFloat = MaterialTheme.motionScheme.fastSpatialSpec<IntSize>()
    val radius by
        animateDpAsState(
            targetValue = if (expanded) EXPANDED_RADIUS else COLLAPSED_RADIUS,
            animationSpec = spatial,
            label = "composerRadius",
        )
    val surfacePadding by
        animateDpAsState(
            targetValue = if (expanded) S5Theme.spacing.medium else S5Theme.spacing.tiny,
            animationSpec = spatial,
            label = "composerPadding",
        )
    val fieldStartPadding by
        animateDpAsState(
            targetValue = if (expanded) S5Theme.spacing.tiny else S5Theme.spacing.medium,
            animationSpec = spatial,
            label = "composerFieldPadding",
        )

    Column(
        modifier
            .fillMaxWidth()
            .imePadding()
            .navigationBarsPadding()
            .padding(horizontal = S5Theme.spacing.medium, vertical = S5Theme.spacing.small),
        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        ComposerStatusPill(
            connectionState = connectionState,
            connectionError = connectionError,
            environmentLabel = environmentLabel,
            syncPhase = syncPhase,
            onClick = onReconnect,
        )

        SuggestionPopover(
            commands = commandSuggestions + skillSuggestions,
            paths = pathSuggestions,
            pullRequests = pullRequestSuggestions,
            groupLabel =
                when (trigger?.kind) {
                    ComposerTriggerKind.PullRequest -> "Pull requests"
                    ComposerTriggerKind.Skill -> "Skills"
                    ComposerTriggerKind.Path -> "Files"
                    else -> "Commands"
                },
            onPick = onPickRow@{ suggestion ->
                val range = trigger ?: return@onPickRow
                if (suggestion.opensUsageLimits && attachments.isEmpty()) {
                    // The row belongs to T3: the token leaves the draft and the
                    // card opens without a turn. With attachments the prompt is
                    // real input, so the row inserts like any other command.
                    onValueChange(
                        replaceComposerTextRange(value, range.rangeStart, range.rangeEnd, ""),
                    )
                    onUsageLimits()
                    return@onPickRow
                }
                if (suggestion.interactionMode != null) {
                    // A mode command consumes the token rather than inserting text.
                    onValueChange(
                        replaceComposerTextRange(value, range.rangeStart, range.rangeEnd, ""),
                    )
                    onInteractionMode?.invoke(suggestion.interactionMode)
                } else {
                    onValueChange(
                        replaceComposerTextRange(
                            value, range.rangeStart, range.rangeEnd,
                            suggestion.replacement,
                        ),
                    )
                }
            },
            onPickPullRequest = onPick@{ pullRequest ->
                val range = trigger ?: return@onPick
                if (range.kind != ComposerTriggerKind.PullRequest) return@onPick
                onPickPullRequest(pullRequest, range.rangeStart, range.rangeEnd)
            },
        )

        if (providerAdvisory != null) {
            Text(
                providerAdvisory,
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurface,
                modifier =
                    Modifier.padding(
                        horizontal = S5Theme.spacing.medium,
                        vertical = S5Theme.spacing.tiny,
                    ),
            )
        }

        S5ComposerSurface(cornerRadius = radius) {
            // The toolbar appearing changes the surface's height, and growing the
            // composer in one frame shoves the transcript. This is the one place
            // an animated size is worth it, and it is bounded by the field's own
            // max line count.
            Column(Modifier.animateContentSize(spatialFloat).padding(surfacePadding)) {
                AnimatedVisibility(
                    visible = expanded && attachments.isNotEmpty(),
                    enter = fadeIn(),
                    exit = fadeOut(),
                ) {
                    S5AttachmentStrip(
                        attachments = attachments,
                        onRemove = onRemoveAttachment,
                        onPreview = { previewAttachment = it },
                        thumbnailSize = 56.dp,
                        modifier = Modifier.fillMaxWidth().padding(bottom = S5Theme.spacing.small),
                    )
                }

                Row(
                    Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    S5ComposerField(
                        state = draftState,
                        // RN's composer placeholder, verbatim.
                        placeholder = "Ask the repo agent, or run a command…",
                        // Never single-line, even collapsed. `singleLine` is what
                        // tells the IME to replace the newline key with Done, and
                        // Android fixes that when the input session opens: a field
                        // that flips it on focus keeps the single-line keyboard it
                        // was focused with, so the return key never becomes Enter
                        // and the composer cannot take a second line. Collapsed is
                        // a one-line window onto a multiline field instead.
                        singleLine = false,
                        maxLines = if (expanded) EXPANDED_MAX_LINES else 1,
                        centerContent = !expanded,
                        interactionSource = interactionSource,
                        // The editor freezes while a session is active
                        // (`voiceInputFreezesEditor` in RN): a keystroke cannot
                        // invalidate a transcript mid-flight.
                        enabled = dictation?.state?.active != true,
                        onSubmitShortcut = {
                            if (canSend && dictation?.state?.active != true) onSend()
                        },
                        modifier =
                            Modifier.weight(1f)
                                .heightIn(min = 40.dp)
                                .padding(start = fieldStartPadding)
                                .composerPastedTextReceiver(
                                    shouldFold = { text ->
                                        pastedTextShouldFold(text, canAttachPastedText())
                                    },
                                    onFolded = onPastedText,
                                )
                                .composerImageReceiver(onAddImages),
                    )
                    // Collapsed keeps the mic and one action in the pill. If a
                    // turn is active that action remains Stop; focusing expands
                    // the composer and exposes a separate Send action beside it.
                    if (!expanded) {
                        DictationMicControl(dictation)
                        SendOrStop(
                            working = working,
                            canSend = canSend,
                            onSend = onSend,
                            onCancel = onCancel,
                        )
                    }
                }

                if (expanded) {
                    val activeDictation = dictation?.takeIf { voicePresented }
                    if (activeDictation != null) {
                        // The whole toolbar flips to the dictation row while a
                        // session is live, matching the RN composer.
                        DictationToolbar(
                            dictation = activeDictation,
                            modifier = Modifier.padding(top = S5Theme.spacing.small),
                        )
                    } else {
                        S5ComposerToolbarRow(Modifier.padding(top = S5Theme.spacing.small)) {
                            S5ComposerControl(
                                label = null,
                                icon = Icons.Rounded.Add,
                                contentDescription = "Attach image",
                                onClick = onAddAttachment,
                            )
                            S5ComposerControl(
                                label = modelLabel,
                                leading = { S5ProviderAvatar(provider, size = 20.dp) },
                                trailingIcon = Icons.Rounded.ExpandMore,
                                onClick = onOpenSettings,
                                contentDescription = "Model and settings",
                                modifier = Modifier.widthIn(max = 180.dp),
                            )
                            Box(Modifier.weight(1f))
                            if (devicePreviewCount > 0) {
                                // The device stream opens from the toolbar,
                                // badged like RN when several are open.
                                BadgedBox(
                                    badge = {
                                        if (devicePreviewCount > 1) {
                                            Badge { Text("$devicePreviewCount") }
                                        }
                                    },
                                ) {
                                    S5ComposerControl(
                                        label = null,
                                        icon = Icons.Rounded.Smartphone,
                                        contentDescription =
                                            if (devicePreviewCount == 1) "View device"
                                            else "View $devicePreviewCount devices",
                                        onClick = onOpenDevicePreview,
                                    )
                                }
                            }
                            DictationMicControl(dictation)
                            if (working) {
                                S5ComposerAction(
                                    icon = Icons.Rounded.Stop,
                                    label = "Stop agent",
                                    onClick = onCancel,
                                    containerColor = MaterialTheme.colorScheme.errorContainer,
                                    contentColor = MaterialTheme.colorScheme.onErrorContainer,
                                )
                            }
                            S5ComposerAction(
                                icon = Icons.AutoMirrored.Rounded.Send,
                                label = if (queuedMessages > 0 || connectionState != ConnectionState.Connected) "Queue" else "Send",
                                onClick = onSend,
                                enabled = canSend,
                            )
                        }
                    }
                }
            }
        }

        if (queuedMessages > 0) {
            Text(
                "$queuedMessages queued message${if (queuedMessages == 1) "" else "s"} will send automatically.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = S5Theme.spacing.small),
            )
        }
    }

    S5AttachmentPreviewDialog(
        attachment = previewAttachment,
        onDismiss = { previewAttachment = null },
    )
}

private data class ComposerStatus(val label: String, val unavailable: Boolean)

@Composable
private fun ComposerStatusPill(
    connectionState: ConnectionState,
    connectionError: String?,
    environmentLabel: String,
    syncPhase: ThreadSyncPhase,
    onClick: () -> Unit,
) {
    val status =
        remember(connectionState, connectionError, environmentLabel, syncPhase) {
            when (connectionState) {
                ConnectionState.Connecting,
                ConnectionState.Recovering ->
                    ComposerStatus(
                        if (connectionError.isNullOrBlank()) {
                            "Reconnecting to $environmentLabel…"
                        } else {
                            "Failed to connect. Retrying $environmentLabel…"
                        },
                        unavailable = false,
                    )
                ConnectionState.Offline -> ComposerStatus("You are offline", unavailable = true)
                ConnectionState.Disabled ->
                    ComposerStatus("$environmentLabel is switched off", unavailable = true)
                ConnectionState.AuthRequired ->
                    ComposerStatus(
                        connectionError?.takeIf { it.isNotBlank() }
                            ?.let { "Failed to connect to $environmentLabel: $it" }
                            ?: "Failed to connect to $environmentLabel",
                        unavailable = true,
                    )
                ConnectionState.Connected ->
                    when (syncPhase) {
                        ThreadSyncPhase.Loading -> ComposerStatus("Loading messages…", false)
                        ThreadSyncPhase.Syncing -> ComposerStatus("Syncing messages…", false)
                        ThreadSyncPhase.Live -> null
                    }
            }
        }
    AnimatedVisibility(status != null, enter = fadeIn(), exit = fadeOut()) {
        status?.let { presentation ->
            Surface(
                onClick = onClick,
                shape = MaterialTheme.shapes.extraLarge,
                color = MaterialTheme.colorScheme.surfaceContainerHigh,
                tonalElevation = 2.dp,
            ) {
                Row(
                    Modifier.padding(horizontal = S5Theme.spacing.medium, vertical = S5Theme.spacing.small),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    Icon(
                        if (presentation.unavailable) Icons.Rounded.CloudOff else Icons.Rounded.Sync,
                        contentDescription = null,
                        modifier = Modifier.size(16.dp),
                        tint =
                            if (presentation.unavailable) MaterialTheme.colorScheme.error
                            else MaterialTheme.colorScheme.primary,
                    )
                    Text(
                        presentation.label,
                        style = MaterialTheme.typography.labelMedium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
    }
}

@Composable
private fun SendOrStop(
    working: Boolean,
    canSend: Boolean,
    onSend: () -> Unit,
    onCancel: () -> Unit,
) {
    if (working) {
        S5ComposerAction(
            icon = Icons.Rounded.Stop,
            label = "Stop the agent",
            onClick = onCancel,
            containerColor = MaterialTheme.colorScheme.errorContainer,
            contentColor = MaterialTheme.colorScheme.onErrorContainer,
        )
    } else {
        S5ComposerAction(
            icon = Icons.AutoMirrored.Rounded.Send,
            label = "Send message",
            onClick = onSend,
            enabled = canSend,
        )
    }
}

/**
 * One suggestion row. [replacement] is the text that replaces the trigger token
 * — already serialized, so a path pick carries its Markdown link. When
 * [interactionMode] is set the pick is a mode switch, not an insertion.
 */
internal data class ComposerSuggestion(
    val label: String,
    val description: String,
    val icon: ImageVector?,
    val replacement: String,
    val interactionMode: RuntimeMode? = null,
    /** `/usage-limits` answers locally instead of inserting text. */
    val opensUsageLimits: Boolean = false,
)

/** One ranked `skill` trigger candidate. */
internal data class RankedSkill(
    val skill: ProviderSkill,
    val score: Int,
    val tieBreaker: String,
)

internal fun ProviderSkill.suggestion(): ComposerSuggestion =
    ComposerSuggestion(
        label = displayName ?: name,
        description = shortDescription ?: description.orEmpty(),
        icon = Icons.Rounded.AutoAwesome,
        replacement = "\$${name} ",
    )

/**
 * The `skill` trigger's ranking, ported from `use-composer-command-menu`: the
 * name wins tiers over the display label, then short and long descriptions —
 * fuzzy subsequence only on the two name fields, never in prose.
 */
internal fun rankProviderSkill(skill: ProviderSkill, query: String): RankedSkill? {
    val displayLabel = (skill.displayName ?: skill.name).lowercase()
    val score =
        listOfNotNull(
            scoreQueryMatch(
                value = skill.name.lowercase(),
                query = query,
                exactBase = 0, prefixBase = 2, boundaryBase = 4,
                includesBase = 6, fuzzyBase = 100,
                boundaryMarkers = listOf("-", "_", "/"),
            ),
            scoreQueryMatch(
                value = displayLabel,
                query = query,
                exactBase = 1, prefixBase = 3, boundaryBase = 5,
                includesBase = 7, fuzzyBase = 110,
            ),
            scoreQueryMatch(
                value = skill.shortDescription?.lowercase().orEmpty(),
                query = query,
                exactBase = 20, prefixBase = 22, boundaryBase = 24, includesBase = 26,
            ),
            scoreQueryMatch(
                value = skill.description?.lowercase().orEmpty(),
                query = query,
                exactBase = 30, prefixBase = 32, boundaryBase = 34, includesBase = 36,
            ),
        ).minOrNull() ?: return null
    return RankedSkill(skill, score, "$displayLabel\u0000${skill.name}")
}

/**
 * Slash-command and path-mention discovery. Sits above the composer so the field
 * stays put while the list changes under the token.
 *
 * Rows are plain pressables separated by a hairline, not row-group items: a
 * segmented row draws its own container, which inside this surface reads as a
 * box in a box. The popover is one container, like the RN client's.
 */
@Composable
internal fun SuggestionPopover(
    commands: List<ComposerSuggestion>,
    paths: List<String>,
    pullRequests: List<ComposerPullRequestCandidate>,
    groupLabel: String,
    onPick: (ComposerSuggestion) -> Unit,
    onPickPullRequest: (ComposerPullRequestCandidate) -> Unit,
) {
    val pathSuggestions =
        paths.map { path ->
            ComposerSuggestion(
                label = path.substringAfterLast('/'),
                description = path.substringBeforeLast('/', missingDelimiterValue = ""),
                icon = Icons.Rounded.AlternateEmail,
                // The draft stores the serialized link, not a bare `@path`: the
                // mention only means something to the provider once it is a link.
                replacement = serializeComposerFileLink(path) + " ",
            )
        }
    AnimatedVisibility(
        commands.isNotEmpty() || pathSuggestions.isNotEmpty() || pullRequests.isNotEmpty()
    ) {
        Surface(
            Modifier.fillMaxWidth(),
            shape = MaterialTheme.shapes.large,
            color = MaterialTheme.colorScheme.surfaceContainerHigh,
        ) {
            Column {
                Text(
                    groupLabel,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier =
                        Modifier.padding(
                            start = S5Theme.spacing.medium,
                            end = S5Theme.spacing.medium,
                            top = S5Theme.spacing.small,
                            bottom = S5Theme.spacing.hair,
                        ),
                )
                // Bounded so a broad token cannot push the composer off screen.
                Column(
                    Modifier.heightIn(max = SUGGESTION_LIST_MAX_HEIGHT)
                        .verticalScroll(rememberScrollState())
                ) {
                    val rows = commands + pathSuggestions
                    rows.forEachIndexed { index, suggestion ->
                        SuggestionRow(
                            label = suggestion.label,
                            description = suggestion.description,
                            icon = suggestion.icon ?: Icons.Rounded.Terminal,
                            onClick = { onPick(suggestion) },
                            divided = index != rows.lastIndex + pullRequests.size,
                        )
                    }
                    pullRequests.forEachIndexed { index, pullRequest ->
                        SuggestionRow(
                            label = "#${pullRequest.number}",
                            description =
                                "${if (pullRequest.isDraft) "Draft" else pullRequest.state} · ${pullRequest.title}",
                            icon = Icons.AutoMirrored.Rounded.CallSplit,
                            onClick = { onPickPullRequest(pullRequest) },
                            divided = index != pullRequests.lastIndex,
                        )
                    }
                }
            }
        }
    }
}

@Composable
internal fun SuggestionRow(
    label: String,
    description: String,
    icon: ImageVector,
    onClick: () -> Unit,
    divided: Boolean,
) {
    Column {
        Row(
            Modifier.fillMaxWidth()
                .clickable(onClick = onClick)
                .padding(
                    horizontal = S5Theme.spacing.medium,
                    vertical = S5Theme.spacing.small,
                ),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Icon(
                icon,
                contentDescription = null,
                modifier = Modifier.size(16.dp),
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                label,
                style = MaterialTheme.typography.bodyMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (description.isNotEmpty()) {
                Text(
                    description,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
            }
        }
        if (divided) {
            HorizontalDivider(
                Modifier.padding(horizontal = S5Theme.spacing.medium),
                color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.4f),
            )
        }
    }
}

/** Roughly four rows, matching the RN popover's 180dp scroll cap. */
private val SUGGESTION_LIST_MAX_HEIGHT = 180.dp
