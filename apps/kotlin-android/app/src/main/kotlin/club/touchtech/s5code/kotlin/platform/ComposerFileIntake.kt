package club.touchtech.s5code.kotlin.platform

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.platform.LocalContext
import androidx.core.net.toUri
import club.touchtech.s5code.kotlin.data.COMPOSER_ATTACHMENT_CACHE_DIRECTORY
import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import club.touchtech.s5code.kotlin.model.QuestionAttachmentKind
import java.io.File
import java.util.UUID
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * One picker result copied into the app-owned attachment cache, before the
 * upload that makes it a `QuestionAttachment`. [localPath] is a real file path
 * rather than a `content:` URI so the copy outlives the picker's read grant —
 * the same reason composer images are materialized at intake.
 */
data class StagedQuestionFile(
    val name: String,
    val mimeType: String,
    val sizeBytes: Long,
    val localPath: String,
    val kind: QuestionAttachmentKind,
)

/**
 * The materialized files plus the reason anything was refused, mirroring
 * `pickComposerFiles`/`pickComposerMedia`: partial success is normal, so the
 * caller appends what worked and surfaces [error] once.
 */
data class StagedQuestionFiles(
    val files: List<StagedQuestionFile>,
    val error: String? = null,
)

/**
 * Document picker for question attachments of any type (`OpenMultipleDocuments`,
 * the `pickComposerFiles` counterpart). Returned URIs still need
 * [materializeQuestionFiles] before they can be staged. [onPicked] fires on
 * cancellation too, with an empty list, so callers can always close their
 * "pick in progress" bookkeeping.
 */
@Composable
fun rememberQuestionFilePicker(onPicked: (List<Uri>) -> Unit): () -> Unit {
    val launcher =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
            onPicked(uris)
        }
    return { launcher.launch(arrayOf("*/*")) }
}

/**
 * Media picker for question attachments, the `pickComposerMedia` counterpart.
 * Videos are only offered when the server accepts file uploads — they upload
 * under type "file", so an image-only server must not even see them
 * (`mediaTypes: ["images"]` when `maxVideoBytes` is absent).
 */
@Composable
fun rememberQuestionMediaPicker(
    remaining: Int,
    allowVideos: Boolean,
    onPicked: (List<Uri>) -> Unit,
): () -> Unit {
    val slots = remaining.coerceAtLeast(0)
    // PickMultipleVisualMedia rejects a limit below 2, so a single free slot
    // falls back to the single-item contract.
    val multiple =
        rememberLauncherForActivityResult(
            ActivityResultContracts.PickMultipleVisualMedia(slots.coerceAtLeast(2))
        ) { uris ->
            // Always called, even on cancel, so the card can drop its
            // "pick in progress" mark for the question.
            onPicked(uris)
        }
    val single =
        rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
            onPicked(listOfNotNull(uri))
        }
    return {
        val request =
            PickVisualMediaRequest(
                if (allowVideos) {
                    ActivityResultContracts.PickVisualMedia.ImageAndVideo
                } else {
                    ActivityResultContracts.PickVisualMedia.ImageOnly
                }
            )
        // A full draft still opens the picker: validation then names the limit,
        // which reads better than a button that silently does nothing.
        if (slots >= 2) multiple.launch(request) else single.launch(request)
    }
}

/**
 * Copies picker URIs into the attachment cache and classifies each as an image
 * or a file upload.
 *
 * Classification follows the attachment itself, not which picker produced it —
 * `isComposerImageAttachment`: a PNG chosen through the file picker still
 * uploads as `type: "image"` under the 10 MB image cap. Everything else is a
 * file upload bounded by [maxFileBytes] (`clampFileAttachmentUploadBytes` —
 * the server cap clamped to `PROVIDER_SEND_TURN_MAX_FILE_BYTES`), or an error
 * when the server does not accept file uploads at all.
 *
 * Copying stops one byte past the limit so an oversized pick cannot fill the
 * cache before validation refuses it.
 */
@Composable
fun rememberQuestionFileIntake(
    maxFileBytes: Long?,
    onFiles: (StagedQuestionFiles) -> Unit,
): (List<Uri>) -> Unit {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    return remember(context, scope, maxFileBytes, onFiles) {
        { uris ->
            if (uris.isEmpty()) {
                onFiles(StagedQuestionFiles(emptyList()))
            } else {
                scope.launch {
                    val staged =
                        withContext(Dispatchers.IO) {
                            materializeQuestionFiles(context, uris, maxFileBytes)
                        }
                    onFiles(staged)
                }
            }
        }
    }
}

internal fun materializeQuestionFiles(
    context: Context,
    uris: List<Uri>,
    maxFileBytes: Long?,
): StagedQuestionFiles {
    val directory = File(context.cacheDir, COMPOSER_ATTACHMENT_CACHE_DIRECTORY).apply { mkdirs() }
    pruneStaleAttachments(directory)
    val resolver = context.contentResolver
    val files = mutableListOf<StagedQuestionFile>()
    var error: String? = null
    for (uri in uris) {
        val name = attachmentDisplayName(resolver, uri)?.takeIf { it.isNotBlank() } ?: "file"
        val mimeType = resolver.getType(uri)?.lowercase() ?: "application/octet-stream"
        val isImage = mimeType in ComposerAttachmentLimits.SUPPORTED_IMAGE_MIME_TYPES
        if (!isImage && maxFileBytes == null) {
            error = "'$name' is not a supported image type. Attach GIF, JPEG, PNG, or WebP images."
            continue
        }
        val maxBytes =
            if (isImage) ComposerAttachmentLimits.MAX_IMAGE_BYTES
            else checkNotNull(maxFileBytes)
        val target =
            File(
                directory,
                "${UUID.randomUUID()}-${name.replace(Regex("[/\\\\\\u0000-\\u001F]"), "-")}",
            )
        val copied = copyBounded(resolver, uri, target, maxBytes)
        when {
            copied == null || copied <= 0L -> {
                target.delete()
                error = "'$name' is empty or could not be read."
            }
            copied > maxBytes -> {
                target.delete()
                error = attachmentTooLargeMessage(name, maxBytes)
            }
            else ->
                files +=
                    StagedQuestionFile(
                        name = name,
                        mimeType = mimeType,
                        sizeBytes = copied,
                        localPath = target.absolutePath,
                        kind =
                            if (isImage) QuestionAttachmentKind.Image
                            else QuestionAttachmentKind.File,
                    )
        }
    }
    return StagedQuestionFiles(files, error)
}

/** Streams at most `maxBytes + 1` bytes into [target]; the extra byte is how "too large" is detected. */
private fun copyBounded(
    resolver: android.content.ContentResolver,
    uri: Uri,
    target: File,
    maxBytes: Long,
): Long? =
    runCatching {
            var copied = 0L
            val ceiling = maxBytes + 1
            resolver.openInputStream(uri)?.use { input ->
                target.outputStream().use { output ->
                    val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
                    while (copied < ceiling) {
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

/** `fileAttachmentTooLargeMessage`: the effective cap spelled out as MB, KB, or bytes. */
private fun attachmentTooLargeMessage(name: String, maxBytes: Long): String {
    val size =
        when {
            maxBytes >= 1024 * 1024 && maxBytes % (1024 * 1024) == 0L ->
                "${maxBytes / (1024 * 1024)} MB"
            maxBytes >= 1024 && maxBytes % 1024 == 0L -> "${maxBytes / 1024} KB"
            else -> "$maxBytes ${if (maxBytes == 1L) "byte" else "bytes"}"
        }
    return "'$name' exceeds the $size attachment limit."
}
