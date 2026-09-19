package club.touchtech.s5code.kotlin.design.component

import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.ui.graphics.Color
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue

/** One imperative confirmation request owned by the app root. */
data class S5ConfirmDialogRequest(
    val title: String,
    val message: String? = null,
    val confirmText: String,
    val cancelText: String = "Cancel",
    val destructive: Boolean = false,
    val onConfirm: () -> Unit,
    val onCancel: (() -> Unit)? = null,
)

/**
 * Central dialog presenter. Screens can request confirmation without carrying an
 * `AlertDialog` branch and destructive styling of their own.
 */
@Stable
class S5ConfirmDialogController internal constructor() {
    var request by mutableStateOf<S5ConfirmDialogRequest?>(null)
        private set

    fun show(request: S5ConfirmDialogRequest) {
        this.request = request
    }

    fun cancel() {
        val active = request ?: return
        request = null
        active.onCancel?.invoke()
    }

    fun confirm() {
        val active = request ?: return
        request = null
        active.onConfirm()
    }
}

@Composable
fun rememberS5ConfirmDialogController(): S5ConfirmDialogController =
    remember { S5ConfirmDialogController() }

@Composable
fun S5ConfirmDialogHost(controller: S5ConfirmDialogController) {
    val request = controller.request ?: return
    AlertDialog(
        onDismissRequest = controller::cancel,
        title = { Text(request.title) },
        text = request.message?.let { message -> { Text(message) } },
        confirmButton = {
            TextButton(onClick = controller::confirm) {
                Text(
                    request.confirmText,
                    color =
                        if (request.destructive) MaterialTheme.colorScheme.error
                        else Color.Unspecified,
                )
            }
        },
        dismissButton = {
            TextButton(onClick = controller::cancel) { Text(request.cancelText) }
        },
    )
}
