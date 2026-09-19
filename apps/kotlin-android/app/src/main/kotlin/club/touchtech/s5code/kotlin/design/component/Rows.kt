package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.theme.S5Theme

/**
 * Position of a row inside a grouped block. Drives the expressive segmented
 * shape so a group reads as one container without drawing manual dividers.
 */
enum class S5RowPosition {
    Only,
    First,
    Middle,
    Last,
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun rowShapes(position: S5RowPosition) =
    when (position) {
        S5RowPosition.Only -> ListItemDefaults.shapes()
        S5RowPosition.First -> ListItemDefaults.segmentedShapes(index = 0, count = 3)
        S5RowPosition.Middle -> ListItemDefaults.segmentedShapes(index = 1, count = 3)
        S5RowPosition.Last -> ListItemDefaults.segmentedShapes(index = 2, count = 3)
    }

/**
 * Settings row: icon, label, optional value, chevron.
 *
 * [onClick] is nullable so a row can be a fact rather than a destination while
 * keeping the same shape as its neighbours — an empty cache category, for instance.
 * A null click drops the chevron too, since a chevron that leads nowhere is the
 * promise this exists to avoid.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5SettingsRow(
    icon: ImageVector,
    label: String,
    onClick: (() -> Unit)?,
    modifier: Modifier = Modifier,
    value: String? = null,
    supporting: String? = null,
    position: S5RowPosition = S5RowPosition.Only,
) {
    val trailing: @Composable () -> Unit = {
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (value != null) {
                Text(
                    value,
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (onClick != null) {
                Icon(
                    Icons.AutoMirrored.Rounded.KeyboardArrowRight,
                    contentDescription = null,
                    modifier = Modifier.padding(start = 4.dp).size(20.dp),
                )
            }
        }
    }
    val supportingContent: (@Composable () -> Unit)? =
        supporting?.let { { Text(it, style = MaterialTheme.typography.bodySmall) } }
    if (onClick == null) {
        ListItem(
            modifier = modifier.fillMaxWidth(),
            shapes = rowShapes(position),
            colors = ListItemDefaults.segmentedColors(),
            leadingContent = { Icon(icon, contentDescription = null) },
            supportingContent = supportingContent,
            trailingContent = trailing,
            content = { Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        )
        return
    }
    ListItem(
        onClick = onClick,
        modifier = modifier.fillMaxWidth(),
        shapes = rowShapes(position),
        colors = ListItemDefaults.segmentedColors(),
        leadingContent = { Icon(icon, contentDescription = null) },
        supportingContent = supportingContent,
        trailingContent = trailing,
        content = { Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
    )
}

/**
 * Read-only grouped row: an icon, a label, supporting text, and an optional
 * trailing meta string. No click, because some rows are facts rather than
 * destinations, and giving them ripple would promise a screen that does not
 * exist.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5ListRow(
    icon: ImageVector,
    label: String,
    modifier: Modifier = Modifier,
    supporting: String? = null,
    meta: String? = null,
    position: S5RowPosition = S5RowPosition.Only,
) {
    ListItem(
        modifier = modifier.fillMaxWidth(),
        shapes = rowShapes(position),
        colors = ListItemDefaults.segmentedColors(),
        leadingContent = { Icon(icon, contentDescription = null) },
        supportingContent =
            supporting?.let { { Text(it, style = MaterialTheme.typography.bodySmall) } },
        trailingContent =
            meta?.let {
                {
                    Text(
                        it,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            },
        content = { Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
    )
}

/** Settings row with an inline switch. Icon-less inside a sheet, where a column of glyphs is noise. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5SwitchRow(
    icon: ImageVector?,
    label: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    supporting: String? = null,
    enabled: Boolean = true,
    position: S5RowPosition = S5RowPosition.Only,
) {
    ListItem(
        checked = checked,
        onCheckedChange = onCheckedChange,
        enabled = enabled,
        modifier = modifier.fillMaxWidth(),
        shapes = rowShapes(position),
        colors = ListItemDefaults.segmentedColors(),
        leadingContent = icon?.let { { Icon(it, contentDescription = null) } },
        supportingContent = supporting?.let { { Text(it, style = MaterialTheme.typography.bodySmall) } },
        trailingContent = { Switch(checked = checked, onCheckedChange = null, enabled = enabled) },
        content = { Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
    )
}

/** Single-choice row inside a grouped selection block. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5SelectableRow(
    label: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    supporting: String? = null,
    leading: @Composable (() -> Unit)? = null,
    trailing: @Composable (() -> Unit)? = null,
    position: S5RowPosition = S5RowPosition.Only,
) {
    ListItem(
        selected = selected,
        onClick = onClick,
        modifier = modifier.fillMaxWidth(),
        shapes = rowShapes(position),
        colors = ListItemDefaults.segmentedColors(),
        leadingContent = leading,
        supportingContent = supporting?.let { { Text(it, style = MaterialTheme.typography.bodySmall) } },
        trailingContent = trailing,
        content = { Text(label, maxLines = 2, overflow = TextOverflow.Ellipsis) },
    )
}

/** Grouped block container. Rows inside receive their own segmented shape. */
@Composable
fun S5RowGroup(
    modifier: Modifier = Modifier,
    title: String? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(modifier.fillMaxWidth()) {
        if (title != null) S5SectionHeader(title)
        Column(
            Modifier.padding(horizontal = S5Theme.spacing.gutter),
            verticalArrangement = Arrangement.spacedBy(ListItemDefaults.SegmentedGap),
            content = content,
        )
    }
}

/** Position helper for building grouped rows from an index. */
fun rowPosition(index: Int, count: Int): S5RowPosition =
    when {
        count <= 1 -> S5RowPosition.Only
        index == 0 -> S5RowPosition.First
        index == count - 1 -> S5RowPosition.Last
        else -> S5RowPosition.Middle
    }
