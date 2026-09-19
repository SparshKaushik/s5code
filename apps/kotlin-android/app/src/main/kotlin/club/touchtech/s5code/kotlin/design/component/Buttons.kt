package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ButtonGroup
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.ButtonGroupScope
import androidx.compose.material3.ElevatedButton
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.PlainTooltip
import androidx.compose.material3.SplitButton
import androidx.compose.material3.SplitButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.ToggleButtonDefaults
import androidx.compose.material3.TooltipAnchorPosition
import androidx.compose.material3.TooltipBox
import androidx.compose.material3.TooltipDefaults
import androidx.compose.material3.rememberTooltipState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/**
 * Action emphasis levels. Sizing is derived from these rather than passed as raw
 * dp so no screen can accidentally ship a compact primary action.
 *
 * Heights are deliberately not the expressive extra-large tokens: those are
 * 96dp and 136dp, sized for a single full-bleed action on a marketing surface,
 * and they read as oversized in a working tool where an action sits next to a
 * transcript or a form. The scale below tops out at the medium token, the
 * largest size that still looks like a control rather than a banner.
 *
 * Four steps, far enough apart to be legible as a hierarchy: 56, 48, 40, 32.
 * Buttons that sit next to each other in a row should share an emphasis, or the
 * row gets a ragged top edge.
 *
 * - [Hero]: the single most important action on a screen.
 * - [Primary]: the main commit action inside a form or sheet.
 * - [Prominent]: important but not the hero. The default.
 * - [Secondary]: genuinely secondary, space-constrained rows.
 */
enum class S5ActionEmphasis {
    Hero,
    Primary,
    Prominent,
    Secondary,
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
val S5ActionEmphasis.containerHeight: Dp
    @Composable
    get() =
        when (this) {
            S5ActionEmphasis.Hero -> ButtonDefaults.MediumContainerHeight
            S5ActionEmphasis.Primary -> 48.dp
            S5ActionEmphasis.Prominent -> ButtonDefaults.MinHeight
            S5ActionEmphasis.Secondary -> ButtonDefaults.ExtraSmallContainerHeight
        }

/** Visual weight, independent of emphasis/sizing. */
enum class S5ButtonStyle {
    Filled,
    Tonal,
    Outlined,
    Elevated,
    Text,
}

/**
 * The one button feature code uses. Sizing, content padding, icon size, icon
 * spacing, text style, and interaction shape morphing all come from the matching
 * Compose Material 3 expressive token helpers.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5Button(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    emphasis: S5ActionEmphasis = S5ActionEmphasis.Prominent,
    style: S5ButtonStyle = S5ButtonStyle.Filled,
    icon: ImageVector? = null,
    trailingIcon: ImageVector? = null,
    enabled: Boolean = true,
) {
    val height = emphasis.containerHeight
    val shapes = ButtonDefaults.shapesFor(height)
    val contentPadding =
        ButtonDefaults.contentPaddingFor(
            buttonHeight = height,
            hasStartIcon = icon != null,
            hasEndIcon = trailingIcon != null,
        )
    val iconSize = ButtonDefaults.iconSizeFor(height)
    val iconSpacing = ButtonDefaults.iconSpacingFor(height)
    val textStyle = ButtonDefaults.textStyleFor(height)
    val sized = modifier.heightIn(min = height)

    val content: @Composable RowScope.() -> Unit = {
        if (icon != null) {
            Icon(icon, contentDescription = null, modifier = Modifier.size(iconSize))
            Spacer(Modifier.width(iconSpacing))
        }
        Text(text, style = textStyle)
        if (trailingIcon != null) {
            Spacer(Modifier.width(iconSpacing))
            Icon(trailingIcon, contentDescription = null, modifier = Modifier.size(iconSize))
        }
    }

    when (style) {
        S5ButtonStyle.Filled ->
            Button(
                onClick = onClick,
                shapes = shapes,
                modifier = sized,
                enabled = enabled,
                contentPadding = contentPadding,
                content = content,
            )
        S5ButtonStyle.Tonal ->
            FilledTonalButton(
                onClick = onClick,
                shapes = shapes,
                modifier = sized,
                enabled = enabled,
                contentPadding = contentPadding,
                content = content,
            )
        S5ButtonStyle.Outlined ->
            OutlinedButton(
                onClick = onClick,
                shapes = shapes,
                modifier = sized,
                enabled = enabled,
                contentPadding = contentPadding,
                content = content,
            )
        S5ButtonStyle.Elevated ->
            ElevatedButton(
                onClick = onClick,
                shapes = shapes,
                modifier = sized,
                enabled = enabled,
                contentPadding = contentPadding,
                content = content,
            )
        S5ButtonStyle.Text ->
            TextButton(
                onClick = onClick,
                shapes = shapes,
                modifier = sized,
                enabled = enabled,
                contentPadding = contentPadding,
                content = content,
            )
    }
}

/**
 * Icon-only action. Every caller supplies a label: it becomes both the content
 * description and the tooltip, so icon-only controls stay discoverable under
 * TalkBack and long-press.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5IconButton(
    icon: ImageVector,
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    filled: Boolean = false,
    enabled: Boolean = true,
    compact: Boolean = false,
) {
    TooltipBox(
        positionProvider =
            TooltipDefaults.rememberTooltipPositionProvider(TooltipAnchorPosition.Above),
        tooltip = { PlainTooltip { Text(label) } },
        state = rememberTooltipState(),
    ) {
        val shapes = IconButtonDefaults.shapes()
        // Compact is for icon buttons that sit on top of content, such as the
        // remove target on an attachment thumbnail, where the small container
        // would cover the thing it acts on.
        val size =
            if (compact) IconButtonDefaults.extraSmallContainerSize()
            else IconButtonDefaults.smallContainerSize()
        if (filled) {
            FilledTonalIconButton(
                onClick = onClick,
                shapes = shapes,
                modifier = modifier.size(size),
                enabled = enabled,
            ) {
                Icon(icon, contentDescription = label)
            }
        } else {
            IconButton(
                onClick = onClick,
                shapes = shapes,
                modifier = modifier.size(size),
                enabled = enabled,
            ) {
                Icon(icon, contentDescription = label)
            }
        }
    }
}

/** Icon-only toggle with the expressive round/square selected-shape morph. */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5IconToggleButton(
    icon: ImageVector,
    label: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    TooltipBox(
        positionProvider =
            TooltipDefaults.rememberTooltipPositionProvider(TooltipAnchorPosition.Above),
        tooltip = { PlainTooltip { Text(label) } },
        state = rememberTooltipState(),
    ) {
        IconToggleButton(
            checked = checked,
            onCheckedChange = onCheckedChange,
            shapes = IconButtonDefaults.toggleableShapes(),
            modifier = modifier.size(IconButtonDefaults.smallContainerSize()),
            enabled = enabled,
        ) {
            Icon(icon, contentDescription = label)
        }
    }
}

/** Expressive toggle button used for standalone on/off text controls. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5ToggleButton(
    text: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    enabled: Boolean = true,
) {
    ToggleButton(
        checked = checked,
        onCheckedChange = onCheckedChange,
        modifier = modifier,
        enabled = enabled,
        shapes = ToggleButtonDefaults.shapesFor(ButtonDefaults.MinHeight),
    ) {
        if (icon != null) {
            Icon(icon, contentDescription = null, modifier = Modifier.size(ToggleButtonDefaults.IconSize))
            Spacer(Modifier.width(ToggleButtonDefaults.IconSpacing))
        }
        Text(text)
    }
}

/**
 * Connected selection group. Replaces segmented controls: the pressed item grows
 * and its neighbours compress, which is the expressive selection behaviour.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun <T> S5ConnectedButtonGroup(
    options: List<T>,
    selected: T,
    onSelect: (T) -> Unit,
    label: (T) -> String,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween),
    ) {
        options.forEachIndexed { index, option ->
            ToggleButton(
                checked = option == selected,
                onCheckedChange = { onSelect(option) },
                shapes =
                    when {
                        options.size == 1 ->
                            ToggleButtonDefaults.shapesFor(ButtonDefaults.MinHeight)
                        index == 0 -> ButtonGroupDefaults.connectedLeadingButtonShapes()
                        index == options.lastIndex ->
                            ButtonGroupDefaults.connectedTrailingButtonShapes()
                        else -> ButtonGroupDefaults.connectedMiddleButtonShapes()
                    },
                modifier = Modifier.defaultMinSize(minHeight = 48.dp),
            ) {
                Text(label(option))
            }
        }
    }
}

/**
 * Standard button group with overflow. Use for a family of related actions where
 * the row may not fit; items beyond the fold move into the overflow menu.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5ButtonGroup(
    modifier: Modifier = Modifier,
    overflowIcon: ImageVector,
    overflowLabel: String,
    content: ButtonGroupScope.() -> Unit,
) {
    ButtonGroup(
        overflowIndicator = { menuState ->
            S5IconButton(
                icon = overflowIcon,
                label = overflowLabel,
                onClick = { if (menuState.isShowing) menuState.dismiss() else menuState.show() },
            )
        },
        modifier = modifier,
        content = content,
    )
}

/**
 * Primary action plus its closely related alternative (for example
 * "Send" + "choose model"). Never rebuild this from two adjacent buttons.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5SplitButton(
    text: String,
    onClick: () -> Unit,
    trailingIcon: ImageVector,
    trailingLabel: String,
    onTrailingClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    enabled: Boolean = true,
) {
    SplitButton(
        modifier = modifier,
        leadingButton = {
            SplitButtonDefaults.LeadingButton(onClick = onClick, enabled = enabled) {
                if (icon != null) {
                    Icon(
                        icon,
                        contentDescription = null,
                        modifier = Modifier.size(SplitButtonDefaults.LeadingIconSize),
                    )
                    Spacer(Modifier.width(ToggleButtonDefaults.IconSpacing))
                }
                Text(text)
            }
        },
        trailingButton = {
            SplitButtonDefaults.TrailingButton(onClick = onTrailingClick, enabled = enabled) {
                Icon(
                    trailingIcon,
                    contentDescription = trailingLabel,
                    modifier = Modifier.size(SplitButtonDefaults.TrailingIconSize),
                )
            }
        },
    )
}
