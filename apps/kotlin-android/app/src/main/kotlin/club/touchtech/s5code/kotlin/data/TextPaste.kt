package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import java.io.File
import java.util.UUID
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * `PASTED_TEXT_ATTACHMENT_THRESHOLD_BYTES` — a paste at or past this size folds
 * into a file attachment rather than filling the draft. Byte-based, matching
 * the shared module: character counts understate the context cost of
 * Unicode-heavy clipboard contents.
 */
const val PASTED_TEXT_ATTACHMENT_THRESHOLD_BYTES = 32 * 1024

/**
 * `pastedTextDisposition`: whether a paste should fold into an attachment.
 * [canAttach] rolls the caller's capability and slot checks together — the
 * receiver cannot suspend to ask the server, so the caller answers up front.
 */
fun pastedTextShouldFold(text: String, canAttach: Boolean): Boolean {
    if (!canAttach || text.isEmpty()) return false
    return text.length >= PASTED_TEXT_ATTACHMENT_THRESHOLD_BYTES ||
        text.toByteArray(Charsets.UTF_8).size >= PASTED_TEXT_ATTACHMENT_THRESHOLD_BYTES
}

/** `clampFileAttachmentUploadBytes`: the client never exceeds its own cap. */
fun clampFileAttachmentUploadBytes(advertisedMax: Long): Long =
    minOf(advertisedMax, ComposerAttachmentLimits.MAX_FILE_BYTES)

/**
 * `fileAttachmentTooLargeMessage`: the user-facing rejection for an oversized
 * folded paste or picked file.
 */
fun fileAttachmentTooLargeMessage(name: String, maxUploadBytes: Long): String {
    val maxUploadSize =
        when {
            maxUploadBytes >= 1024 * 1024 && maxUploadBytes % (1024 * 1024) == 0L ->
                "${maxUploadBytes / (1024 * 1024)} MB"
            maxUploadBytes >= 1024 && maxUploadBytes % 1024 == 0L ->
                "${maxUploadBytes / 1024} KB"
            else -> "$maxUploadBytes ${if (maxUploadBytes == 1L) "byte" else "bytes"}"
        }
    return "'$name' exceeds the $maxUploadSize attachment limit."
}

/** `nextPastedTextFileName`: stable names when a draft holds several folds. */
fun nextPastedTextFileName(existingNames: List<String>): String {
    val names = existingNames.mapTo(HashSet()) { it.lowercase() }
    if ("pasted-text.txt" !in names) return "pasted-text.txt"
    var sequence = 2
    while (true) {
        val candidate = "pasted-text-$sequence.txt"
        if (candidate !in names) return candidate
        sequence += 1
    }
}

/**
 * `createPastedTextComposerAttachment`: writes the clipboard text into an
 * app-private cache file and returns its attachment. The cache copy lives
 * beside the outbox-promoted copy the send uses — losing it only blanks the
 * local preview, never the queued bytes.
 */
suspend fun persistPastedTextAttachment(
    cacheDir: File,
    text: String,
    existingNames: List<String>,
): ComposerAttachment? =
    withContext(Dispatchers.IO) {
        val bytes = text.toByteArray(Charsets.UTF_8)
        if (bytes.isEmpty()) return@withContext null
        val name = nextPastedTextFileName(existingNames)
        val directory = File(cacheDir, "pasted-text").apply { mkdirs() }
        // The disk file gets a random prefix so two drafts never share a path;
        // the attachment name stays the friendly one the transcript renders.
        val file = File(directory, "${UUID.randomUUID()}-$name")
        runCatching { file.writeBytes(bytes) }.getOrNull() ?: return@withContext null
        ComposerAttachment(
            id = file.toURI().toString(),
            name = name,
            mimeType = "text/plain;charset=utf-8",
            sizeBytes = bytes.size.toLong(),
            uri = file.toURI().toString(),
            type = "file",
            pastedText = true,
        )
    }
