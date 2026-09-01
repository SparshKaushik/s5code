package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.theme.S5Theme

/**
 * Container prominence, expressed through the M3 surface-container hierarchy and
 * the expanded shape scale rather than elevation stacking.
 */
enum class S5CardTone {
    /** Default grouped content. */
    Standard,

    /** A container that should read quieter than its neighbours. */
    Receded,

    /** A hero container: the one thing on the screen that should be noticed first. */
    Hero,
}

@Composable
fun S5Card(
    modifier: Modifier = Modifier,
    tone: S5CardTone = S5CardTone.Standard,
    onClick: (() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    val shape =
        when (tone) {
            S5CardTone.Standard -> MaterialTheme.shapes.large
            S5CardTone.Receded -> MaterialTheme.shapes.medium
            S5CardTone.Hero -> MaterialTheme.shapes.extraLargeIncreased
        }
    val colors =
        when (tone) {
            S5CardTone.Standard ->
                CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow)
            S5CardTone.Receded ->
                CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLowest)
            S5CardTone.Hero ->
                CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                )
        }
    if (onClick != null) {
        Card(onClick = onClick, modifier = modifier, shape = shape, colors = colors) {
            Column(content = content)
        }
    } else {
        Card(modifier = modifier, shape = shape, colors = colors) { Column(content = content) }
    }
}

/**
 * Section label plus rule. The only structure in otherwise flat technical lists,
 * matching the RN client's section dividers.
 */
@Composable
fun S5SectionHeader(
    label: String,
    modifier: Modifier = Modifier,
    trailing: @Composable (RowScope.() -> Unit)? = null,
) {
    Row(
        modifier
            .fillMaxWidth()
            .padding(
                start = S5Theme.spacing.gutter,
                end = S5Theme.spacing.gutter,
                top = S5Theme.spacing.large,
                bottom = S5Theme.spacing.small,
            ),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        Text(
            label,
            style = MaterialTheme.typography.labelLargeEmphasized,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        HorizontalDivider(Modifier.weight(1f))
        if (trailing != null) trailing()
    }
}

/**
 * Iconic-shape badge. Used for avatars, statuses, and hero art — the intentional
 * places for `MaterialShapes`, not scattered through dense rows.
 */
@Composable
fun S5ShapeBadge(
    icon: ImageVector,
    contentDescription: String?,
    shape: androidx.compose.ui.graphics.Shape,
    containerColor: Color,
    contentColor: Color,
    modifier: Modifier = Modifier,
    size: androidx.compose.ui.unit.Dp = 40.dp,
    iconSize: androidx.compose.ui.unit.Dp = 20.dp,
) {
    Surface(shape = shape, color = containerColor, contentColor = contentColor, modifier = modifier.size(size)) {
        Box(contentAlignment = Alignment.Center) {
            Icon(icon, contentDescription = contentDescription, modifier = Modifier.size(iconSize))
        }
    }
}
