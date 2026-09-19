package club.touchtech.s5code.kotlin.platform

import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import androidx.core.net.toUri
import club.touchtech.s5code.kotlin.data.IncomingShareAttachment
import club.touchtech.s5code.kotlin.data.IncomingShareAttachmentType
import club.touchtech.s5code.kotlin.data.IncomingShareDraft
import club.touchtech.s5code.kotlin.data.INCOMING_SHARE_MAX_FILE_BYTES
import club.touchtech.s5code.kotlin.data.fileTooLargeMessage
import club.touchtech.s5code.kotlin.data.incomingShareIdFor
import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Send-turn limits: eight attachments, 10 MB images, 50 MB files. */
private const val MAX_SHARED_ATTACHMENTS = 8

/**
 * Turns a share intent's payload into a durable inbox draft.
 *
 * Every `content:` payload is copied into the share's inbox directory before
 * the draft is written. The sender's grant lapses when the share activity
 * finishes — sometimes before the user even opens the draft — so an attachment
 * that kept the original URI would point at bytes that no longer resolve. The
 * copy also yields a measured size, which is the value the attachment records:
 * providers misreport `OpenableColumns.SIZE` often enough that trusting it is a
 * latent oversized-upload rejection.
 *
 * Returns the draft when it has anything worth drafting (text or an accepted
 * attachment), plus a failure message otherwise. Both can carry warnings for
 * the items that were refused.
 */
suspend fun buildIncomingShare(
    context: Context,
    text: String?,
    intentMimeType: String?,
    uris: List<String>,
    nowIso: String,
): IncomingShareDraft? =
    withContext(Dispatchers.IO) {
        val shareId = incomingShareIdFor(text, intentMimeType, uris)
        val directory = File(context.filesDir, "incoming-shares/$shareId").apply { mkdirs() }
        val resolver = context.contentResolver
        val attachments = mutableListOf<IncomingShareAttachment>()
        val warnings = mutableListOf<String>()
        var warnedAttachmentLimit = false

        uris.take(MAX_SHARED_ATTACHMENTS + 4).forEachIndexed { index, raw ->
            val source = raw.toUri()
            val mimeType =
                (resolver.getType(source) ?: intentMimeType ?: "application/octet-stream")
                    .lowercase()
            val name = attachmentDisplayName(resolver, source) ?: fallbackName(index, mimeType)
            val isImage = mimeType.startsWith("image/")

            if (attachments.size >= MAX_SHARED_ATTACHMENTS) {
                if (!warnedAttachmentLimit) {
                    warnings +=
                        "Only the first $MAX_SHARED_ATTACHMENTS shared " +
                            "${if (isImage) "images" else "files"} were attached."
                    warnedAttachmentLimit = true
                }
                return@forEachIndexed
            }

            if (isImage && mimeType !in ComposerAttachmentLimits.SUPPORTED_IMAGE_MIME_TYPES) {
                warnings += "'$name' is not a supported image type."
                return@forEachIndexed
            }
            val ceiling =
                if (isImage) ComposerAttachmentLimits.MAX_IMAGE_BYTES
                else INCOMING_SHARE_MAX_FILE_BYTES
            val target = File(directory, "payload-$index")
            val copied = copyWithCeiling(resolver, source, target, ceiling)
            when {
                copied == null || copied <= 0L -> {
                    target.delete()
                    warnings += "'$name' is empty or could not be read."
                }
                copied > ceiling -> {
                    target.delete()
                    warnings += fileTooLargeMessage(name, ceiling)
                }
                else ->
                    attachments +=
                        IncomingShareAttachment(
                            id = "$shareId:${if (isImage) "image" else "file"}:$index",
                            type =
                                if (isImage) IncomingShareAttachmentType.Image
                                else IncomingShareAttachmentType.File,
                            name = name,
                            mimeType = mimeType,
                            sizeBytes = copied,
                            uri = Uri.fromFile(target).toString(),
                        )
            }
        }

        IncomingShareDraft(
            id = shareId,
            createdAt = nowIso,
            text = text.orEmpty().trim(),
            attachments = attachments,
            warnings = warnings,
        )
    }

/**
 * Streams [source] into [target], stopping one byte past [ceiling] so an
 * oversized share cannot fill the inbox before the limit check refuses it.
 * Null when the provider cannot be opened.
 */
private fun copyWithCeiling(
    resolver: ContentResolver,
    source: Uri,
    target: File,
    ceiling: Long,
): Long? =
    runCatching {
            var copied = 0L
            resolver.openInputStream(source)?.use { input ->
                target.outputStream().use { output ->
                    val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
                    while (copied <= ceiling) {
                        val read = input.read(buffer)
                        if (read <= 0) break
                        output.write(buffer, 0, read)
                        copied += read
                    }
                }
            } ?: return@runCatching null
            copied
        }
        .getOrNull()

/** Deterministic name when the provider reports none, `fallbackName` in RN. */
private fun fallbackName(index: Int, mimeType: String): String {
    val family = mimeType.substringBefore('/').lowercase()
    val kind = if (family in setOf("image", "audio", "video")) family else "file"
    val extension =
        mimeType.substringAfter('/', "")
            .replace(Regex("[^a-z0-9.+\\-]"), "")
            .ifBlank { if (kind == "image") "png" else "bin" }
    return "shared-$kind-${index + 1}.$extension"
}
