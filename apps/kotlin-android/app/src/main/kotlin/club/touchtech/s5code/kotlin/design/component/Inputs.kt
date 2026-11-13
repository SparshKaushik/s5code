package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.theme.S5Theme

/**
 * The one text input feature code uses, shaped like the composer.
 *
 * Deliberately not a Material text field. The filled and outlined fields bring a
 * floating label, a 56dp minimum, an indicator line, and their own container
 * tone, which is three competing input styles in an app whose primary input is
 * the composer pill. Every field in the app is now the same object: one rounded
 * surface, the caption above it, the text inside it, and the supporting line
 * below it.
 *
 * The label sits above the field rather than floating into its border. A
 * floating label is what forces the tall container, and it animates on every
 * focus change, which is motion for no information.
 */
@Composable
fun S5TextField(
    value: String,
    onValueChange: (String) -> Unit,
    modifier: Modifier = Modifier,
    label: String? = null,
    placeholder: String = "",
    supporting: String? = null,
    isError: Boolean = false,
    enabled: Boolean = true,
    singleLine: Boolean = false,
    maxLines: Int = 6,
    minHeight: Dp = FIELD_MIN_HEIGHT,
    leadingIcon: ImageVector? = null,
    textStyle: TextStyle? = null,
    trailing: @Composable (RowScope.() -> Unit)? = null,
) {
    // Keyed on nothing: this field owns its text, and the caller's value is the
    // source of truth for external changes (a cleared search, a restored draft).
    val state = rememberDraftTextFieldState(Unit, value, onValueChange)
    S5TextField(
        state = state,
        modifier = modifier,
        label = label,
        placeholder = placeholder,
        supporting = supporting,
        isError = isError,
        enabled = enabled,
        singleLine = singleLine,
        maxLines = maxLines,
        minHeight = minHeight,
        leadingIcon = leadingIcon,
        textStyle = textStyle,
        trailing = trailing,
    )
}

/**
 * State-backed variant, for fields that need keyboard content commits (image
 * paste, stickers, GIFs) or that already hold a [TextFieldState].
 */
@Composable
fun S5TextField(
    state: TextFieldState,
    modifier: Modifier = Modifier,
    label: String? = null,
    placeholder: String = "",
    supporting: String? = null,
    isError: Boolean = false,
    enabled: Boolean = true,
    singleLine: Boolean = false,
    maxLines: Int = 6,
    minHeight: Dp = FIELD_MIN_HEIGHT,
    leadingIcon: ImageVector? = null,
    textStyle: TextStyle? = null,
    interactionSource: MutableInteractionSource? = null,
    fieldModifier: Modifier = Modifier,
    trailing: @Composable (RowScope.() -> Unit)? = null,
) {
    val captionColor =
        if (isError) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
        if (label != null) {
            Text(
                label,
                style = MaterialTheme.typography.labelLarge,
                color = captionColor,
                modifier = Modifier.padding(start = S5Theme.spacing.tiny),
            )
        }
        S5ComposerSurface(
            cornerRadius = if (singleLine) FIELD_PILL_RADIUS else FIELD_CARD_RADIUS,
            borderColor = if (isError) MaterialTheme.colorScheme.error else null,
        ) {
            Row(
                Modifier.padding(
                    horizontal = S5Theme.spacing.medium,
                    vertical = S5Theme.spacing.small,
                ),
                verticalAlignment = if (singleLine) Alignment.CenterVertically else Alignment.Top,
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                if (leadingIcon != null) {
                    Icon(
                        leadingIcon,
                        contentDescription = null,
                        modifier = Modifier.size(20.dp),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                S5ComposerField(
                    state = state,
                    placeholder = placeholder,
                    singleLine = singleLine,
                    maxLines = maxLines,
                    enabled = enabled,
                    textStyle = textStyle,
                    interactionSource = interactionSource,
                    modifier = fieldModifier.weight(1f).heightIn(min = minHeight),
                )
                trailing?.invoke(this)
            }
        }
        if (supporting != null) {
            Text(
                supporting,
                style = MaterialTheme.typography.bodySmall,
                color = captionColor,
                modifier = Modifier.padding(start = S5Theme.spacing.tiny),
            )
        }
    }
}

/**
 * Search field: the same surface as a pill, plus a clear target once there is
 * something to clear. Separate from [S5TextField] only because every search in
 * the app wants the identical icon, placeholder shape, and clear affordance, and
 * repeating that at six call sites is how they drift apart.
 */
@Composable
fun S5SearchField(
    value: String,
    onValueChange: (String) -> Unit,
    placeholder: String,
    modifier: Modifier = Modifier,
    focusRequester: FocusRequester? = null,
) {
    // State-backed so a focus requester can target the BasicTextField rather
    // than its outer layout node.
    val state = rememberDraftTextFieldState(Unit, value, onValueChange)
    S5TextField(
        state = state,
        placeholder = placeholder,
        singleLine = true,
        leadingIcon = Icons.Rounded.Search,
        modifier = modifier,
        fieldModifier =
            if (focusRequester == null) Modifier else Modifier.focusRequester(focusRequester),
        trailing = {
            if (value.isNotEmpty()) {
                S5IconButton(
                    icon = Icons.Rounded.Close,
                    label = "Clear search",
                    onClick = { onValueChange("") },
                    compact = true,
                )
            } else {
                // Holds the clear button's width so the text does not reflow the
                // moment you type the first character.
                Box(Modifier.size(CLEAR_SLOT_SIZE))
            }
        },
    )
}

/** Text height floor: one line plus the caret, matching the composer's field. */
private val FIELD_MIN_HEIGHT = 28.dp

/** Fully round for single-line fields, so they read as the composer's pill. */
private val FIELD_PILL_RADIUS = 22.dp

/** Multi-line fields get the composer's card radius instead. */
private val FIELD_CARD_RADIUS = 22.dp

private val CLEAR_SLOT_SIZE = 32.dp
