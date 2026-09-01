package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.app.isComposerSubmitShortcut
import club.touchtech.s5code.kotlin.design.theme.S5Theme

/**
 * Bare composer text field: text, caret, placeholder, nothing else.
 *
 * Deliberately not a Material text field. `OutlinedTextField` and its filled
 * sibling bring their own container, label slot, and 56dp minimum, which is a
 * second box inside the composer's own surface and a control that cannot shrink
 * to a pill. The composer surface is the container; the field is only the text.
 *
 * Still state-backed rather than value/onValueChange: keyboard content commits
 * (Gboard clipboard, stickers, GIFs) only reach a `TextFieldState` field.
 *
 * [singleLine] is the IME contract, not a layout hint: it decides whether the
 * keyboard is told this field takes one line and gets a Done action instead of a
 * newline key. Android negotiates that once, when the input session opens, so a
 * field that flips it after focus keeps the keyboard it opened with. A composer
 * that grows on focus must therefore stay `singleLine = false` and shrink its
 * [maxLines] instead — see [S5ComposerField]'s caller in the thread composer.
 */
@Composable
fun S5ComposerField(
    state: TextFieldState,
    placeholder: String,
    modifier: Modifier = Modifier,
    singleLine: Boolean = false,
    maxLines: Int = 6,
    enabled: Boolean = true,
    textStyle: TextStyle? = null,
    interactionSource: MutableInteractionSource? = null,
    /** Vertically centers the text, as a one-line pill wants. */
    centerContent: Boolean = singleLine,
    /** Cmd/Ctrl+Enter submits; bare Enter remains a newline. */
    onSubmitShortcut: (() -> Unit)? = null,
) {
    val source = interactionSource ?: remember { MutableInteractionSource() }
    val resolved = textStyle ?: MaterialTheme.typography.bodyLarge
    val contentColor =
        if (enabled) MaterialTheme.colorScheme.onSurface
        else MaterialTheme.colorScheme.onSurface.copy(alpha = 0.38f)
    val resolvedStyle = resolved.copy(color = contentColor)
    BasicTextField(
        state = state,
        modifier =
            if (onSubmitShortcut == null) {
                modifier
            } else {
                modifier.onPreviewKeyEvent { event ->
                    val native = event.nativeKeyEvent
                    isComposerSubmitShortcut(
                        keyCode = native.keyCode,
                        action = native.action,
                        repeatCount = native.repeatCount,
                        ctrlPressed = native.isCtrlPressed,
                        metaPressed = native.isMetaPressed,
                        altPressed = native.isAltPressed,
                    ).also { submit -> if (submit) onSubmitShortcut() }
                }
            },
        enabled = enabled,
        textStyle = resolvedStyle,
        lineLimits =
            if (singleLine) {
                TextFieldLineLimits.SingleLine
            } else {
                TextFieldLineLimits.MultiLine(
                    minHeightInLines = 1,
                    maxHeightInLines = maxLines.coerceAtLeast(1),
                )
            },
        cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
        interactionSource = source,
        decorator = { inner ->
            Box(contentAlignment = if (centerContent) Alignment.CenterStart else Alignment.TopStart) {
                if (state.text.isEmpty()) {
                    Text(
                        placeholder,
                        style = resolvedStyle,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                inner()
            }
        },
    )
}

/**
 * Round icon action sized for the composer: the send/stop target in the pill and
 * the toolbar. Smaller than an M3 icon button on purpose, because it sits inside
 * a 48dp pill and a 40dp control would set the pill's height.
 */
@Composable
fun S5ComposerAction(
    icon: ImageVector,
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    containerColor: Color = MaterialTheme.colorScheme.primary,
    contentColor: Color = MaterialTheme.colorScheme.onPrimary,
    size: Dp = 36.dp,
) {
    val disabledContainer = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.12f)
    val disabledContent = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.38f)
    Surface(
        onClick = onClick,
        modifier = modifier.size(size),
        enabled = enabled,
        shape = CircleShape,
        color = if (enabled) containerColor else disabledContainer,
        contentColor = if (enabled) contentColor else disabledContent,
    ) {
        Box(contentAlignment = Alignment.Center) {
            Icon(icon, contentDescription = label, modifier = Modifier.size(size * 0.5f))
        }
    }
}

/**
 * Quiet inline control for the composer toolbar (model picker, attach). Reads as
 * part of the composer surface rather than a button inside a button, which is
 * what a tonal button in this row looks like.
 *
 * A null [onClick] makes it a static label with the same metrics: used where the
 * value exists but there is nothing to choose between, such as a single
 * environment. Disabling it instead would say "unavailable" about something that
 * is merely settled.
 */
@Composable
fun S5ComposerControl(
    label: String?,
    onClick: (() -> Unit)?,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    leading: @Composable (() -> Unit)? = null,
    trailingIcon: ImageVector? = null,
    enabled: Boolean = true,
    contentDescription: String? = null,
) {
    val contentColor =
        if (enabled) MaterialTheme.colorScheme.onSurfaceVariant
        else MaterialTheme.colorScheme.onSurface.copy(alpha = 0.38f)
    val content: @Composable () -> Unit = {
        Row(
            Modifier.padding(horizontal = S5Theme.spacing.small),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
        ) {
            when {
                leading != null -> leading()
                icon != null ->
                    Icon(
                        icon,
                        contentDescription = contentDescription ?: label,
                        modifier = Modifier.size(18.dp),
                    )
            }
            if (label != null) {
                Text(
                    label,
                    style = MaterialTheme.typography.labelLarge,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            if (trailingIcon != null) {
                Icon(trailingIcon, contentDescription = null, modifier = Modifier.size(14.dp))
            }
        }
    }

    if (onClick == null) {
        Surface(
            modifier.heightIn(min = 36.dp),
            shape = MaterialTheme.shapes.medium,
            color = Color.Transparent,
            contentColor = contentColor,
            content = content,
        )
    } else {
        Surface(
            onClick = onClick,
            modifier = modifier.heightIn(min = 36.dp),
            enabled = enabled,
            shape = MaterialTheme.shapes.medium,
            color = Color.Transparent,
            contentColor = contentColor,
            content = content,
        )
    }
}

/**
 * The composer's own container. One surface, one optional border, whatever shape
 * the current state wants: a pill when idle, a card when focused.
 *
 * [borderColor] is for the error state on form fields. Nothing draws a border by
 * default: the tonal step against the page is the containment, and an outline on
 * top of it is the doubled container this component exists to avoid.
 */
@Composable
fun S5ComposerSurface(
    cornerRadius: Dp,
    modifier: Modifier = Modifier,
    borderColor: Color? = null,
    content: @Composable () -> Unit,
) {
    Surface(
        modifier.fillMaxWidth(),
        shape = RoundedCornerShape(cornerRadius),
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        contentColor = MaterialTheme.colorScheme.onSurface,
        border = borderColor?.let { BorderStroke(1.dp, it) },
        content = content,
    )
}

/** Row wrapper that keeps the toolbar's content color consistent. */
@Composable
fun S5ComposerToolbarRow(modifier: Modifier = Modifier, content: @Composable RowScope.() -> Unit) {
    Row(
        modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
        content = content,
    )
}
