package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.layout.Box
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.rounded.ArrowDropDown
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuGroup
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MenuDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.foundation.layout.size
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
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
    val children: List<S5MenuOption> = emptyList(),
    /** A toggle in a multi-select menu: picking it leaves the menu open for the next pick. */
    val keepOpen: Boolean = false,
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
    /** What the menu drops from; a long-press target replaces the icon button. */
    anchor: @Composable () -> Unit = {
        S5IconButton(icon = icon, label = label, onClick = { onExpandedChange(!expanded) })
    },
) {
    var currentSubmenu by remember(expanded) { mutableStateOf<S5MenuOption?>(null) }

    Box(modifier) {
        anchor()
        DropdownMenu(
            expanded = expanded,
            onDismissRequest = {
                currentSubmenu = null
                onExpandedChange(false)
            },
            containerColor = Color.Transparent,
            tonalElevation = 0.dp,
            shadowElevation = 0.dp,
            border = null,
        ) {
            MenuGroupSurface {
                val activeMenu = currentSubmenu
                if (activeMenu != null) {
                    val count = activeMenu.children.size + 1
                    DropdownMenuItem(
                        selected = false,
                        onClick = { currentSubmenu = null },
                        text = { Text(activeMenu.label, fontWeight = FontWeight.SemiBold) },
                        shapes = MenuDefaults.itemShape(index = 0, count = count),
                        colors = MenuDefaults.selectableItemColors(containerColor = Color.Transparent),
                        leadingIcon = {
                            Icon(Icons.AutoMirrored.Rounded.ArrowBack, contentDescription = "Back")
                        },
                    )
                    activeMenu.children.forEachIndexed { index, child ->
                        val childColors =
                            if (child.destructive) {
                                MenuDefaults.selectableItemColors(
                                    containerColor = Color.Transparent,
                                    textColor = MaterialTheme.colorScheme.error,
                                    leadingIconColor = MaterialTheme.colorScheme.error,
                                )
                            } else {
                                MenuDefaults.selectableItemColors(containerColor = Color.Transparent)
                            }
                        DropdownMenuItem(
                            selected = child.selected,
                            enabled = child.enabled,
                            onClick = {
                                if (!child.keepOpen) {
                                    currentSubmenu = null
                                    onExpandedChange(false)
                                }
                                onSelect(child.id)
                            },
                            text = { Text(child.label) },
                            shapes = MenuDefaults.itemShape(index = index + 1, count = count),
                            colors = childColors,
                            leadingIcon = child.icon?.let { { Icon(it, contentDescription = null) } },
                            selectedLeadingIcon = {
                                Icon(Icons.Rounded.Check, contentDescription = null)
                            },
                            supportingText = child.supporting?.let { { Text(it) } },
                        )
                    }
                } else {
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
                        val hasChildren = option.children.isNotEmpty()
                        val trailingContent: (@Composable () -> Unit)? =
                            if (hasChildren) {
                                {
                                    Icon(
                                        Icons.AutoMirrored.Rounded.KeyboardArrowRight,
                                        contentDescription = null,
                                    )
                                }
                            } else {
                                null
                            }
                        DropdownMenuItem(
                            selected = option.selected,
                            enabled = option.enabled,
                            onClick = {
                                if (hasChildren) {
                                    currentSubmenu = option
                                } else {
                                    if (!option.keepOpen) onExpandedChange(false)
                                    onSelect(option.id)
                                }
                            },
                            text = { Text(option.label) },
                            shapes = MenuDefaults.itemShape(index = index, count = options.size),
                            colors = itemColors,
                            leadingIcon = option.icon?.let { { Icon(it, contentDescription = null) } },
                            selectedLeadingIcon = {
                                Icon(Icons.Rounded.Check, contentDescription = null)
                            },
                            trailingContent = trailingContent,
                            supportingText = option.supporting?.let { { Text(it) } },
                        )
                    }
                }
            }
        }
    }
}

/**
 * A filter-row chip that drops an [S5OverflowMenu]. [active] fills the chip
 * while its filter narrows the list, so a row of chips reads at a glance.
 */
@Composable
fun S5FilterMenu(
    label: String,
    active: Boolean,
    options: List<S5MenuOption>,
    onSelect: (String) -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
) {
    var expanded by remember { mutableStateOf(false) }
    S5OverflowMenu(
        icon = Icons.Rounded.ArrowDropDown,
        label = label,
        options = options,
        onSelect = onSelect,
        expanded = expanded,
        onExpandedChange = { expanded = it },
        modifier = modifier,
        anchor = {
            FilterChip(
                selected = active,
                onClick = { expanded = !expanded },
                label = { Text(label, maxLines = 1) },
                leadingIcon =
                    icon?.let {
                        { Icon(it, contentDescription = null, modifier = Modifier.size(FilterChipDefaults.IconSize)) }
                    },
                trailingIcon = {
                    Icon(
                        Icons.Rounded.ArrowDropDown,
                        contentDescription = null,
                        modifier = Modifier.size(FilterChipDefaults.IconSize),
                    )
                },
            )
        },
    )
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
