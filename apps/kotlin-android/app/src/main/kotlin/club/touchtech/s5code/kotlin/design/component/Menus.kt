package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.layout.Box
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuGroup
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MenuDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp

/** One entry in an S5 overflow/selection menu. */
data class S5MenuOption(
    val id: String,
    val label: String,
    val icon: ImageVector? = null,
    val supporting: String? = null,
    val selected: Boolean = false,
    val enabled: Boolean = true,
    val destructive: Boolean = false,
)

/**
 * Icon-anchored overflow menu.
 *
 * One visible container, not two. `DropdownMenu` paints its own elevated surface
 * and the expressive grouped items paint another rounded fill inside it, one
 * tonal step apart and inset by the popup's padding: a box in a box with a seam
 * around it. The *inner* box is the one that stays, because it is the shape the
 * items actually sit in and the one whose corners follow the group. The popup
 * container is turned off entirely — transparent, no elevation, no border — so
 * the group carries the shape and the shadow.
 *
 * The selected item still gets its own fill, since that is a state marker rather
 * than decoration, and it is the only thing in the menu that should read as a
 * separate shape.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5OverflowMenu(
    icon: ImageVector,
    label: String,
    options: List<S5MenuOption>,
    onSelect: (String) -> Unit,
    expanded: Boolean,
    onExpandedChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
) {
    Box(modifier) {
        S5IconButton(icon = icon, label = label, onClick = { onExpandedChange(!expanded) })
        DropdownMenu(
            expanded = expanded,
            onDismissRequest = { onExpandedChange(false) },
            containerColor = Color.Transparent,
            tonalElevation = 0.dp,
            shadowElevation = 0.dp,
            border = null,
        ) {
            MenuGroupSurface {
                options.forEachIndexed { index, option ->
                    val itemColors =
                        if (option.destructive) {
                            // Destructive entries were previously indistinguishable
                            // from the rest, so the flag did nothing.
                            MenuDefaults.selectableItemColors(
                                containerColor = Color.Transparent,
                                textColor = MaterialTheme.colorScheme.error,
                                leadingIconColor = MaterialTheme.colorScheme.error,
                            )
                        } else {
                            MenuDefaults.selectableItemColors(containerColor = Color.Transparent)
                        }
                    DropdownMenuItem(
                        selected = option.selected,
                        enabled = option.enabled,
                        onClick = {
                            onExpandedChange(false)
                            onSelect(option.id)
                        },
                        text = { Text(option.label) },
                        shapes = MenuDefaults.itemShape(index = index, count = options.size),
                        colors = itemColors,
                        leadingIcon = option.icon?.let { { Icon(it, contentDescription = null) } },
                        selectedLeadingIcon = {
                            Icon(Icons.Rounded.Check, contentDescription = null)
                        },
                        supportingText = option.supporting?.let { { Text(it) } },
                    )
                }
            }
        }
    }
}

/**
 * The menu's single container: the expressive grouped item box, now carrying the
 * shape, tone, and shadow the popup used to own.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun MenuGroupSurface(content: @Composable () -> Unit) {
    DropdownMenuGroup(
        shapes = MenuDefaults.groupShapes(),
        shadowElevation = MENU_SHADOW_ELEVATION,
    ) {
        content()
    }
}

/** Lift for the one remaining container. Matches the M3 menu shadow token. */
private val MENU_SHADOW_ELEVATION = 3.dp
