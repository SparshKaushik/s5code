package club.touchtech.s5code.kotlin.design.component

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Bedtime
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.automirrored.rounded.HelpOutline
import androidx.compose.material.icons.rounded.HourglassEmpty
import androidx.compose.material.icons.rounded.PendingActions
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Schedule
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.vector.ImageVector
import club.touchtech.s5code.kotlin.design.theme.S5MaterialShapes
import club.touchtech.s5code.kotlin.design.theme.S5PillShape
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.ThreadStatus

/**
 * One place that turns a thread status into its label, color pairs, icon, and
 * iconic shape. Every surface that shows status reads this, so a thread looks
 * the same in the home list, the thread header, and (later) notifications.
 *
 * Two pairs, and they must not be crossed: [content] is the readable label color
 * on [container], while [onAccent] is what goes on top of the saturated
 * [accent]. Putting `onAccent` on `container` is how a badge ends up as white
 * text on a pastel fill at 1.3:1.
 */
@Immutable
data class S5StatusPresentation(
    val label: String,
    val container: Color,
    val content: Color,
    val accent: Color,
    val onAccent: Color,
    val icon: ImageVector,
    val shape: Shape,
)

@Composable
fun statusPresentation(status: ThreadStatus): S5StatusPresentation {
    val colors = S5Theme.status
    return when (status) {
        ThreadStatus.Working ->
            S5StatusPresentation(
                "Working",
                colors.workingContainer,
                colors.onWorkingContainer,
                colors.working,
                colors.onWorking,
                Icons.Rounded.PlayArrow,
                S5MaterialShapes.working(),
            )
        ThreadStatus.AwaitingApproval ->
            S5StatusPresentation(
                "Approval",
                colors.approvalContainer,
                colors.onApprovalContainer,
                colors.approval,
                colors.onApproval,
                Icons.Rounded.PendingActions,
                S5MaterialShapes.approval(),
            )
        ThreadStatus.AwaitingInput ->
            S5StatusPresentation(
                "Input",
                colors.inputContainer,
                colors.onInputContainer,
                colors.input,
                colors.onInput,
                Icons.AutoMirrored.Rounded.HelpOutline,
                S5MaterialShapes.input(),
            )
        ThreadStatus.Failed ->
            S5StatusPresentation(
                "Failed",
                colors.failedContainer,
                colors.onFailedContainer,
                colors.failed,
                colors.onFailed,
                Icons.Rounded.ErrorOutline,
                S5MaterialShapes.failed(),
            )
        ThreadStatus.Settled ->
            S5StatusPresentation(
                "Settled",
                colors.settledContainer,
                colors.onSettledContainer,
                colors.settled,
                colors.onSettled,
                Icons.Rounded.CheckCircle,
                S5MaterialShapes.settled(),
            )
        ThreadStatus.Snoozed ->
            S5StatusPresentation(
                "Snoozed",
                colors.inputContainer,
                colors.onInputContainer,
                colors.input,
                colors.onInput,
                Icons.Rounded.Bedtime,
                S5PillShape,
            )
        ThreadStatus.Queued ->
            S5StatusPresentation(
                "Queued",
                colors.workingContainer,
                colors.onWorkingContainer,
                colors.working,
                colors.onWorking,
                Icons.Rounded.Schedule,
                S5PillShape,
            )
        ThreadStatus.Idle ->
            S5StatusPresentation(
                "Idle",
                colors.settledContainer,
                colors.onSettledContainer,
                colors.settled,
                colors.onSettled,
                Icons.Rounded.HourglassEmpty,
                S5PillShape,
            )
    }
}
