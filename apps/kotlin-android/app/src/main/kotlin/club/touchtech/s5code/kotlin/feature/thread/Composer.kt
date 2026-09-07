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
import androidx.compose.material.icons.automirrored.rounded.Send
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.AlternateEmail
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.ExpandMore
import androidx.compose.material.icons.rounded.Sync
import androidx.compose.material.icons.rounded.Stop
import androidx.compose.material.icons.rounded.Terminal
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
import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.SlashCommand
import club.touchtech.s5code.kotlin.model.ThreadSyncPhase
import kotlinx.coroutines.delay
import club.touchtech.s5code.kotlin.platform.composerImageReceiver

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
    /** Resolves `@` mentions to workspace paths. Suspend: it hits the file index. */
    onSearchPaths: suspend (String) -> List<String>,
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
    model: String,
    onOpenSettings: () -> Unit,
    /** Resets the field's own text state, e.g. per thread. */
    draftKey: Any?,
) {
    val activeToken = remember(value) { value.substringAfterLast(' ', missingDelimiterValue = value) }
    val commandSuggestions =
        remember(activeToken, commands) {
            if (activeToken.startsWith("/")) rankSlashCommands(commands, activeToken)
            else emptyList()
        }
    // Path lookup is a server round trip, so it is debounced and cancels itself
    // on the next keystroke rather than firing per character.
    var pathSuggestions by remember { mutableStateOf(emptyList<String>()) }
    var previewAttachment by remember { mutableStateOf<ComposerAttachment?>(null) }
    LaunchedEffect(activeToken) {
        if (!activeToken.startsWith("@")) {
            pathSuggestions = emptyList()
            return@LaunchedEffect
        }
        delay(160)
        pathSuggestions =
            runCatching { onSearchPaths(activeToken.drop(1)) }
                .map { rankComposerPaths(it, activeToken) }
                .getOrDefault(emptyList())
    }
    // The field owns its text so keyboard content commits (Gboard clipboard,
    // stickers, GIFs) reach it; the draft outside still wins on external change.
    val draftState = rememberDraftTextFieldState(draftKey, value, onValueChange)

    val interactionSource = remember { MutableInteractionSource() }
    val focused by interactionSource.collectIsFocusedAsState()
    // Attachments force the card open: thumbnails have nowhere to go in a pill,
    // and a paste that appeared to do nothing is worse than an expanded composer.
    val expanded = focused || attachments.isNotEmpty()
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
            commands = commandSuggestions,
            paths = pathSuggestions,
            onPick = { replacement ->
                onValueChange(value.dropLast(activeToken.length) + replacement + " ")
            },
        )

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
                        placeholder =
                            if (expanded) "Message the agent. / for commands, @ for paths"
                            else "Message the agent",
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
                        onSubmitShortcut = { if (canSend) onSend() },
                        modifier =
                            Modifier.weight(1f)
                                .heightIn(min = 40.dp)
                                .padding(start = fieldStartPadding)
                                .composerImageReceiver(onAddImages),
                    )
                    // Collapsed keeps one action in the pill. If a turn is active
                    // that action remains Stop; focusing expands the composer and
                    // exposes a separate Queue action beside it.
                    if (!expanded) {
                        SendOrStop(
                            working = working,
                            canSend = canSend,
                            onSend = onSend,
                            onCancel = onCancel,
                        )
                    }
                }

                if (expanded) {
                    S5ComposerToolbarRow(Modifier.padding(top = S5Theme.spacing.small)) {
                        S5ComposerControl(
                            label = null,
                            icon = Icons.Rounded.Add,
                            contentDescription = "Attach image",
                            onClick = onAddAttachment,
                        )
                        S5ComposerControl(
                            label = model,
                            leading = { S5ProviderAvatar(provider, size = 20.dp) },
                            trailingIcon = Icons.Rounded.ExpandMore,
                            onClick = onOpenSettings,
                            contentDescription = "Model and settings",
                            modifier = Modifier.widthIn(max = 180.dp),
                        )
                        Box(Modifier.weight(1f))
                        if (working) {
                            S5ComposerAction(
                                icon = Icons.Rounded.Stop,
                                label = "Stop the agent",
                                onClick = onCancel,
                                containerColor = MaterialTheme.colorScheme.errorContainer,
                                contentColor = MaterialTheme.colorScheme.onErrorContainer,
                            )
                        }
                        S5ComposerAction(
                            icon = Icons.AutoMirrored.Rounded.Send,
                            label = if (working || queuedMessages > 0 || connectionState != ConnectionState.Connected) "Queue message" else "Send message",
                            onClick = onSend,
                            enabled = canSend,
                        )
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
 * Slash-command and path-mention discovery. Sits above the composer so the field
 * stays put while the list changes under the token.
 *
 * Rows are plain pressables separated by a hairline, not row-group items: a
 * segmented row draws its own container, which inside this surface reads as a
 * box in a box. The popover is one container, like the RN client's.
 */
@Composable
private fun SuggestionPopover(
    commands: List<SlashCommand>,
    paths: List<String>,
    onPick: (String) -> Unit,
) {
    AnimatedVisibility(commands.isNotEmpty() || paths.isNotEmpty()) {
        Surface(
            Modifier.fillMaxWidth(),
            shape = MaterialTheme.shapes.large,
            color = MaterialTheme.colorScheme.surfaceContainerHigh,
        ) {
            Column {
                Text(
                    if (commands.isNotEmpty()) "Commands" else "Files",
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
                    val rows =
                        commands.map { command ->
                            Triple(command.name, command.description, Icons.Rounded.Terminal)
                        } + paths.map { Triple("@$it", "", Icons.Rounded.AlternateEmail) }
                    rows.forEachIndexed { index, (label, description, icon) ->
                        SuggestionRow(
                            label = label,
                            description = description,
                            icon = icon,
                            onClick = { onPick(label) },
                            divided = index != rows.lastIndex,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun SuggestionRow(
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
