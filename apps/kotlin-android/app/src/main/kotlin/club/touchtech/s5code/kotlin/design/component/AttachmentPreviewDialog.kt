package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import coil3.compose.AsyncImage
import club.touchtech.s5code.kotlin.model.ComposerAttachment

/**
 * Full-resolution composer attachment preview with pinch and double-tap zoom.
 * The draft URI points at the app-owned intake copy, so this does not depend on
 * an expired picker/clipboard grant.
 */
@Composable
fun S5AttachmentPreviewDialog(
    attachment: ComposerAttachment?,
    onDismiss: () -> Unit,
) {
    S5ImageLightbox(
        model = attachment?.uri,
        contentDescription = attachment?.name,
        imageKey = attachment?.id,
        onDismiss = onDismiss,
    )
}

/**
 * Shared fullscreen image lightbox for draft, sent, and Markdown images.
 * Pinch zooms around the current image, drags pan while zoomed, and double tap
 * toggles 1×/2×. The dialog owns the scrim and close affordance everywhere.
 */
@Composable
fun S5ImageLightbox(
    model: Any?,
    contentDescription: String?,
    onDismiss: () -> Unit,
    imageKey: Any? = model,
) {
    if (model == null) return
    var scale by remember(imageKey) { mutableFloatStateOf(1f) }
    var offset by remember(imageKey) { mutableStateOf(Offset.Zero) }
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Box(
            Modifier.fillMaxSize()
                .background(MaterialTheme.colorScheme.scrim.copy(alpha = 0.92f))
                .pointerInput(imageKey) {
                    detectTransformGestures { _, pan, zoom, _ ->
                        scale = (scale * zoom).coerceIn(1f, 8f)
                        offset = if (scale > 1f) offset + pan else Offset.Zero
                    }
                }
                .pointerInput(imageKey) {
                    detectTapGestures(
                        onDoubleTap = {
                            scale = if (scale > 1f) 1f else 2f
                            offset = Offset.Zero
                        }
                    )
                },
            contentAlignment = Alignment.Center,
        ) {
            AsyncImage(
                model = model,
                contentDescription = contentDescription,
                contentScale = ContentScale.Fit,
                modifier =
                    Modifier.fillMaxSize()
                        .graphicsLayer(
                            scaleX = scale,
                            scaleY = scale,
                            translationX = offset.x,
                            translationY = offset.y,
                        ),
            )
            S5IconButton(
                icon = Icons.Rounded.Close,
                label = "Close image preview",
                onClick = onDismiss,
                filled = true,
                modifier = Modifier.align(Alignment.TopEnd),
            )
        }
    }
}
