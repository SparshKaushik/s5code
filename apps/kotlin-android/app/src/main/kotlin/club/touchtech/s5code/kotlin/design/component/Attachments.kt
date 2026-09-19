package club.touchtech.s5code.kotlin.design.component

import android.content.ContentResolver
import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.BrokenImage
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Draft attachment strip: a thumbnail per image with its own remove target.
 * Thumbnails are decoded downsampled and off the main thread, so a 10 MB paste
 * never lands a full-size bitmap in the composer's recomposition.
 */
@Composable
fun S5AttachmentStrip(
    attachments: List<ComposerAttachment>,
    onRemove: (ComposerAttachment) -> Unit,
    modifier: Modifier = Modifier,
    onPreview: (ComposerAttachment) -> Unit = {},
    thumbnailSize: Dp = 64.dp,
) {
    if (attachments.isEmpty()) return
    Row(modifier, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
        attachments.forEach { attachment ->
            key(attachment.id) {
                AttachmentThumbnail(
                    attachment = attachment,
                    size = thumbnailSize,
                    onPreview = { onPreview(attachment) },
                    onRemove = { onRemove(attachment) },
                )
            }
        }
    }
}

@Composable
private fun AttachmentThumbnail(
    attachment: ComposerAttachment,
    size: Dp,
    onPreview: () -> Unit,
    onRemove: () -> Unit,
) {
    val sizePx = with(LocalDensity.current) { size.roundToPx() }
    val thumbnail = rememberImageThumbnail(attachment.uri, sizePx)
    Box(contentAlignment = Alignment.TopEnd) {
        Surface(
            onClick = onPreview,
            modifier = Modifier.size(size),
            shape = RoundedCornerShape(12.dp),
            color = MaterialTheme.colorScheme.surfaceContainerHighest,
        ) {
            if (thumbnail != null) {
                Image(
                    bitmap = thumbnail,
                    contentDescription = attachment.name,
                    contentScale = ContentScale.Crop,
                    modifier = Modifier.size(size),
                )
            } else {
                Box(Modifier.size(size), contentAlignment = Alignment.Center) {
                    Icon(
                        Icons.Rounded.BrokenImage,
                        contentDescription = attachment.name,
                        modifier = Modifier.size(18.dp),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        S5IconButton(
            icon = Icons.Rounded.Close,
            label = "Remove ${attachment.name}",
            onClick = onRemove,
            filled = true,
            compact = true,
        )
    }
}

/**
 * Decodes a bounded thumbnail for a content or file URI. Returns null while the
 * decode is in flight and when the URI is unreadable, which is the normal
 * outcome for a clipboard grant that has already expired.
 */
@Composable
fun rememberImageThumbnail(uri: String, sizePx: Int): ImageBitmap? {
    if (uri.isBlank()) return null
    val resolver = LocalContext.current.contentResolver
    var bitmap by remember(uri, sizePx) { mutableStateOf<ImageBitmap?>(null) }
    LaunchedEffect(uri, sizePx, resolver) {
        bitmap = withContext(Dispatchers.IO) { decodeThumbnail(resolver, uri, sizePx) }
    }
    return bitmap
}

private fun decodeThumbnail(
    resolver: ContentResolver,
    uri: String,
    sizePx: Int,
): ImageBitmap? =
    runCatching {
            val parsed = uri.toUri()
            val bounds =
                BitmapFactory.Options().apply { inJustDecodeBounds = true }.also { options ->
                    resolver.openInputStream(parsed)?.use { BitmapFactory.decodeStream(it, null, options) }
                }
            val shortestEdge = minOf(bounds.outWidth, bounds.outHeight)
            if (shortestEdge <= 0) return@runCatching null
            var sample = 1
            while (sample * 2 <= 8 && shortestEdge / (sample * 2) >= sizePx) sample *= 2
            val options = BitmapFactory.Options().apply { inSampleSize = sample }
            resolver
                .openInputStream(parsed)
                ?.use { BitmapFactory.decodeStream(it, null, options) }
                ?.asImageBitmap()
        }
        .getOrNull()
