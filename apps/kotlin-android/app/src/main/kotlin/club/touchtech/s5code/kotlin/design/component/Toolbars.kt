package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.layout.RowScope
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FloatingToolbarDefaults
import androidx.compose.material3.HorizontalFloatingToolbar
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector

/**
 * Contextual page actions live in a floating toolbar rather than crowding the
 * app bar. Optionally pairs with a vibrant FAB for the screen's hero action.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5FloatingToolbar(
    modifier: Modifier = Modifier,
    expanded: Boolean = true,
    vibrant: Boolean = false,
    fabIcon: ImageVector? = null,
    fabLabel: String? = null,
    onFabClick: (() -> Unit)? = null,
    content: @Composable RowScope.() -> Unit,
) {
    val colors =
        if (vibrant) FloatingToolbarDefaults.vibrantFloatingToolbarColors()
        else FloatingToolbarDefaults.standardFloatingToolbarColors()

    if (fabIcon != null && onFabClick != null) {
        HorizontalFloatingToolbar(
            expanded = expanded,
            modifier = modifier,
            colors = colors,
            floatingActionButton = {
                if (vibrant) {
                    FloatingToolbarDefaults.VibrantFloatingActionButton(onClick = onFabClick) {
                        Icon(fabIcon, contentDescription = fabLabel)
                    }
                } else {
                    FloatingToolbarDefaults.StandardFloatingActionButton(onClick = onFabClick) {
                        Icon(fabIcon, contentDescription = fabLabel)
                    }
                }
            },
            content = content,
        )
    } else {
        HorizontalFloatingToolbar(
            expanded = expanded,
            modifier = modifier,
            colors = colors,
            content = content,
        )
    }
}
