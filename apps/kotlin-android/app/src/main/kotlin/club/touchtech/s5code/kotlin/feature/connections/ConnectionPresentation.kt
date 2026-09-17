package club.touchtech.s5code.kotlin.feature.connections

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Cloud
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.Computer
import androidx.compose.material.icons.rounded.DesktopWindows
import androidx.compose.material.icons.rounded.Dns
import androidx.compose.material.icons.rounded.Key
import androidx.compose.material.icons.rounded.Laptop
import androidx.compose.material.icons.rounded.PowerSettingsNew
import androidx.compose.material.icons.rounded.Sync
import androidx.compose.material.icons.rounded.SyncProblem
import androidx.compose.material.icons.rounded.Terminal
import androidx.compose.material.icons.rounded.Wifi
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.model.Environment

/**
 * The environment glyph, matching `EnvironmentMachineSymbol` in the RN client:
 * the machine kind the server publishes decides, and the cloud/direct fallback
 * only applies when the server sent no kind.
 */
fun environmentIcon(environment: Environment): ImageVector =
    when (environment.machineKind) {
        "cloud" -> Icons.Rounded.Cloud
        "linux" -> Icons.Rounded.Terminal
        "desktop" -> Icons.Rounded.DesktopWindows
        "laptop" -> Icons.Rounded.Laptop
        // No Material glyph distinguishes mini/studio form factors; the RN
        // labels ("Mini PC", "Workstation") both render as a desktop-class box.
        "mac-mini",
        "mac-studio" -> Icons.Rounded.Computer
        "server" -> Icons.Rounded.Dns
        else ->
            if (environment.kind ==
                club.touchtech.s5code.kotlin.model.EnvironmentKind.Cloud
            ) {
                Icons.Rounded.Cloud
            } else {
                Icons.Rounded.Computer
            }
    }

/**
 * Label, color pair, and icon for one environment's health. [content] is the
 * on-container color, so the pair is readable when drawn as a badge.
 */
@Immutable
data class S5ConnectionPresentation(
    val label: String,
    val container: Color,
    val content: Color,
    val icon: ImageVector,
    val offline: Boolean,
)

@Composable
fun connectionPresentation(state: ConnectionState): S5ConnectionPresentation {
    val colors = S5Theme.status
    return when (state) {
        ConnectionState.Connected ->
            S5ConnectionPresentation(
                "Online",
                colors.settledContainer,
                colors.onSettledContainer,
                Icons.Rounded.Wifi,
                offline = false,
            )
        ConnectionState.Connecting ->
            S5ConnectionPresentation(
                "Connecting",
                colors.workingContainer,
                colors.onWorkingContainer,
                Icons.Rounded.Sync,
                offline = false,
            )
        ConnectionState.Recovering ->
            S5ConnectionPresentation(
                "Recovering",
                colors.approvalContainer,
                colors.onApprovalContainer,
                Icons.Rounded.SyncProblem,
                offline = false,
            )
        ConnectionState.Offline ->
            S5ConnectionPresentation(
                "Offline",
                colors.failedContainer,
                colors.onFailedContainer,
                Icons.Rounded.CloudOff,
                offline = true,
            )
        ConnectionState.AuthRequired ->
            S5ConnectionPresentation(
                "Sign-in needed",
                colors.inputContainer,
                colors.onInputContainer,
                Icons.Rounded.Key,
                offline = true,
            )
        // Deliberately not an error tone: off is a choice the user made, so the
        // row recedes to neutral surface colors rather than alarming.
        ConnectionState.Disabled ->
            S5ConnectionPresentation(
                "Off",
                MaterialTheme.colorScheme.surfaceContainerHigh,
                MaterialTheme.colorScheme.onSurfaceVariant,
                Icons.Rounded.PowerSettingsNew,
                offline = true,
            )
    }
}
