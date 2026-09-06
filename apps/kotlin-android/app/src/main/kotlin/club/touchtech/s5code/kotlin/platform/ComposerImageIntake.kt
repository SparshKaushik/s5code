package club.touchtech.s5code.kotlin.platform

import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.platform.LocalContext
import androidx.core.net.toUri
import club.touchtech.s5code.kotlin.data.COMPOSER_ATTACHMENT_CACHE_DIRECTORY
import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import club.touchtech.s5code.kotlin.model.ComposerImageCandidate
import java.io.File
import java.util.UUID
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private const val STALE_ATTACHMENT_AGE_MILLIS = 24L * 60 * 60 * 1000

/**
 * Copies incoming images into app cache and forwards them for validation.
 *
 * A clipboard or keyboard grant covers one read, so an attachment that keeps the
 * original content URI stops resolving as soon as the grant lapses — the draft
 * would still list the image while its thumbnail and its bytes are gone. Copying
 * at intake makes the attachment durable and yields a real byte count, which is
 * a better size check than provider-reported metadata.
 */
@Composable
fun rememberComposerImageIntake(
    onImages: (List<ComposerImageCandidate>) -> Unit,
): (List<ComposerImageCandidate>) -> Unit {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    return remember(context, scope, onImages) {
        { candidates ->
            if (candidates.isNotEmpty()) {
                scope.launch {
                    val materialized =
                        withContext(Dispatchers.IO) { materializeComposerImages(context, candidates) }
                    onImages(materialized)
                }
            }
        }
    }
}

/**
 * Cached copies for the images worth keeping. Candidates whose type is already
 * unsupported pass through untouched so validation can name them in its error
 * rather than this layer silently dropping them.
 */
internal fun materializeComposerImages(
    context: Context,
    candidates: List<ComposerImageCandidate>,
): List<ComposerImageCandidate> {
    val directory = File(context.cacheDir, COMPOSER_ATTACHMENT_CACHE_DIRECTORY).apply { mkdirs() }
    pruneStaleAttachments(directory)
    return candidates.map { candidate ->
        val mimeType = candidate.mimeType?.lowercase()
        if (mimeType !in ComposerAttachmentLimits.SUPPORTED_IMAGE_MIME_TYPES) return@map candidate
        copyToCache(context.contentResolver, directory, candidate, mimeType!!) ?: candidate
    }
}

/**
 * Streams the image into cache, stopping one byte past the limit so an
 * oversized pick cannot fill the cache before validation refuses it.
 */
private fun copyToCache(
    resolver: ContentResolver,
    directory: File,
    candidate: ComposerImageCandidate,
    mimeType: String,
): ComposerImageCandidate? =
    runCatching {
            val source = candidate.uri.toUri()
            val target = File(directory, "${UUID.randomUUID()}.${extensionFor(mimeType)}")
            var copied = 0L
            val ceiling = ComposerAttachmentLimits.MAX_IMAGE_BYTES + 1
            resolver.openInputStream(source)?.use { input ->
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

            if (copied <= 0) {
                target.delete()
                return@runCatching null
            }
            candidate.copy(
                uri = Uri.fromFile(target).toString(),
                name = candidate.name ?: displayName(resolver, source),
                sizeBytes = copied,
            )
        }
        .getOrNull()

/** The provider's display name, so the UI and errors can name the image. */
private fun displayName(resolver: ContentResolver, uri: Uri): String? =
    runCatching {
            resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
                val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (index >= 0 && cursor.moveToFirst() && !cursor.isNull(index)) {
                    cursor.getString(index)?.takeIf { it.isNotBlank() }
                } else {
                    null
                }
            }
        }
        .getOrNull()

private fun extensionFor(mimeType: String): String =
    when (mimeType) {
        "image/gif" -> "gif"
        "image/jpeg" -> "jpg"
        "image/webp" -> "webp"
        else -> "png"
    }

/** Drops yesterday's copies; drafts that still reference them are long gone. */
private fun pruneStaleAttachments(directory: File) {
    val cutoff = System.currentTimeMillis() - STALE_ATTACHMENT_AGE_MILLIS
    directory.listFiles()?.forEach { file -> if (file.lastModified() < cutoff) file.delete() }
}
