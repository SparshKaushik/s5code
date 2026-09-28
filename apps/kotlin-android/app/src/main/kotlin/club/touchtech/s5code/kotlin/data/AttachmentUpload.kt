package club.touchtech.s5code.kotlin.data

import android.content.Context
import android.util.Base64
import androidx.core.net.toUri
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * The `UploadChatImageAttachment` wire shape: inline `dataUrl`, no server id —
 * the server persists the bytes and mints one itself.
 *
 * The contract has no separate upload endpoint for chat images: the bytes travel
 * inside the `thread.turn.start` command. That is why the intake step already
 * bounded each image to 10 MB — this function would otherwise be where a large
 * photo becomes a 14-million-character JSON string on someone's cellular
 * connection.
 *
 * Attachments whose bytes cannot be read are dropped rather than sent empty. The
 * turn still goes out with its text, which is better than failing a message
 * because a cache file was evicted.
 */
 * The `UploadChatImageAttachment` wire shape: inline `dataUrl`, no server id —
 * the server persists the bytes and mints one itself.
 */
internal suspend fun attachmentDataUrl(
    context: Context,
    attachment: ComposerAttachment,
): String? =
    withContext(Dispatchers.IO) {
        if (attachment.sizeBytes > ComposerAttachmentLimits.MAX_IMAGE_BYTES) return@withContext null
        val bytes =
            runCatching {
                    context.contentResolver
                        .openInputStream(attachment.uri.toUri())
                        ?.use { it.readBytes() }
                }
                .getOrNull() ?: return@withContext null
        val encoded = Base64.encodeToString(bytes, Base64.NO_WRAP)
        "data:${attachment.mimeType};base64,$encoded"
    }
