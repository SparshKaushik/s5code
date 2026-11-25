package club.touchtech.s5code.kotlin.design.component

import androidx.compose.material3.Icon
import androidx.compose.material3.ShortNavigationBar
import androidx.compose.material3.ShortNavigationBarItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector

/** One destination in [S5BottomTabs]. */
data class S5Tab<T>(val value: T, val label: String, val icon: ImageVector)

/**
 * Top-level destinations within one screen, as an M3 short navigation bar.
 * Goes in an [S5Screen]'s `bottomBar`; switching tabs swaps that screen's
 * content rather than pushing a route.
 */
@Composable
fun <T> S5BottomTabs(
    tabs: List<S5Tab<T>>,
    selected: T,
    onSelect: (T) -> Unit,
    modifier: Modifier = Modifier,
) {
    ShortNavigationBar(modifier) {
        tabs.forEach { tab ->
            ShortNavigationBarItem(
                selected = tab.value == selected,
                onClick = { onSelect(tab.value) },
                icon = { Icon(tab.icon, contentDescription = null) },
                label = { Text(tab.label) },
            )
        }
    }
}
