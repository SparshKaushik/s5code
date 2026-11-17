package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FloatingActionButtonDefaults
import androidx.compose.material3.FloatingActionButtonMenu
import androidx.compose.material3.FloatingActionButtonMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MediumExtendedFloatingActionButton
import androidx.compose.material3.MediumFloatingActionButton
import androidx.compose.material3.SmallFloatingActionButton
import androidx.compose.material3.Text
import androidx.compose.material3.ToggleFloatingActionButton
import androidx.compose.material3.ToggleFloatingActionButtonDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp

/** One hero action per screen. Extended when a label adds clarity. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5HeroFab(
    icon: ImageVector,
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    extended: Boolean = true,
) {
    if (extended) {
        MediumExtendedFloatingActionButton(onClick = onClick, modifier = modifier) {
            Icon(icon, contentDescription = null, modifier = Modifier.size(FloatingActionButtonDefaults.MediumIconSize))
            Spacer(Modifier.width(12.dp))
            Text(label)
        }
    } else {
        MediumFloatingActionButton(onClick = onClick, modifier = modifier) {
            Icon(
                icon,
                contentDescription = label,
                modifier = Modifier.size(FloatingActionButtonDefaults.MediumIconSize),
            )
        }
    }
}

/**
 * Small circular action that floats over content, for a secondary move the content
 * itself suggests rather than a screen's hero action (jumping a transcript back to
 * its live edge).
 *
 * Deliberately smaller and tonal: it sits above a composer that is already the
 * primary target on the screen, and a medium primary FAB there would outweigh it.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5FloatingAction(
    icon: ImageVector,
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    SmallFloatingActionButton(
        onClick = onClick,
        modifier = modifier,
        containerColor = MaterialTheme.colorScheme.secondaryContainer,
        contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
    ) {
        Icon(icon, contentDescription = label, modifier = Modifier.size(20.dp))
    }
}

/** A short family of related actions behind one hero toggle. */
data class S5FabMenuItem(val icon: ImageVector, val label: String, val onClick: () -> Unit)

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5FabMenu(
    expanded: Boolean,
    onExpandedChange: (Boolean) -> Unit,
    icon: ImageVector,
    expandedIcon: ImageVector,
    label: String,
    items: List<S5FabMenuItem>,
    modifier: Modifier = Modifier,
) {
    FloatingActionButtonMenu(
        expanded = expanded,
        modifier = modifier,
        button = {
            ToggleFloatingActionButton(
                checked = expanded,
                onCheckedChange = onExpandedChange,
                containerSize = ToggleFloatingActionButtonDefaults.containerSizeMedium(),
                containerCornerRadius = ToggleFloatingActionButtonDefaults.containerCornerRadiusMedium(),
                modifier =
                    Modifier.semantics {
                        stateDescription = if (expanded) "Expanded" else "Collapsed"
                        contentDescription = label
                    },
            ) {
                with(ToggleFloatingActionButtonDefaults) {
                    Icon(
                        imageVector = if (expanded) expandedIcon else icon,
                        contentDescription = null,
                        modifier = Modifier.animateIcon({ checkedProgress }),
                    )
                }
            }
        },
    ) {
        items.forEach { item ->
            FloatingActionButtonMenuItem(
                onClick = {
                    onExpandedChange(false)
                    item.onClick()
                },
                icon = { Icon(item.icon, contentDescription = null) },
                text = { Text(item.label) },
            )
        }
    }
}
