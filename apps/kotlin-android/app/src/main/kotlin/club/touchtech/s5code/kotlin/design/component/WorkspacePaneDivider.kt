package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt

/**
 * Forgiving touch, mouse, and accessibility target between chat and the
 * trailing workspace inspector. Only the center line paints; the full 16dp
 * width can be dragged.
 */
@Composable
fun S5WorkspacePaneDivider(
    currentWidthDp: Float,
    minimumWidthDp: Float,
    maximumWidthDp: Float,
    onResizeBy: (Float) -> Unit,
    modifier: Modifier = Modifier,
) {
    var dragging by remember { mutableStateOf(false) }
    val inactive = MaterialTheme.colorScheme.outlineVariant
    val active = MaterialTheme.colorScheme.primary
    Box(
        modifier
            .width(WORKSPACE_DIVIDER_TOUCH_WIDTH)
            .fillMaxHeight()
            .semantics {
                stateDescription = "${currentWidthDp.roundToInt()} dp wide"
                progressBarRangeInfo =
                    androidx.compose.ui.semantics.ProgressBarRangeInfo(
                        current = currentWidthDp,
                        range = minimumWidthDp..maximumWidthDp.coerceAtLeast(minimumWidthDp),
                    )
                customActions =
                    listOf(
                        CustomAccessibilityAction("Make inspector wider") {
                            onResizeBy(ACCESSIBILITY_RESIZE_STEP_DP)
                            true
                        },
                        CustomAccessibilityAction("Make inspector narrower") {
                            onResizeBy(-ACCESSIBILITY_RESIZE_STEP_DP)
                            true
                        },
                    )
            }
            .pointerInput(onResizeBy) {
                detectHorizontalDragGestures(
                    onDragStart = { dragging = true },
                    onDragCancel = { dragging = false },
                    onDragEnd = { dragging = false },
                ) { change, dragAmount ->
                    change.consume()
                    // The inspector trails the divider, so dragging left grows it.
                    onResizeBy(-dragAmount / density)
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier.width(if (dragging) 2.dp else 1.dp)
                .fillMaxHeight()
                .background(if (dragging) active else inactive)
        )
    }
}

val WORKSPACE_DIVIDER_TOUCH_WIDTH = 16.dp
private const val ACCESSIBILITY_RESIZE_STEP_DP = 24f
