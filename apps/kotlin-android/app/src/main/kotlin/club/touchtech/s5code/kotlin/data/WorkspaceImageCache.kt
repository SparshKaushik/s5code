package club.touchtech.s5code.kotlin.data

import android.content.Context
import coil3.ImageLoader
import coil3.request.ImageRequest
import coil3.request.SuccessResult
import coil3.request.crossfade
import coil3.SingletonImageLoader
import coil3.size.Size
import java.util.LinkedHashMap
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Metadata measured from the downloaded bytes and decoded image. */
data class WorkspaceImageMetadata(
    val width: Int,
    val height: Int,
    val byteLength: Long?,
)

/**
 * Bounded process cache for workspace image prewarming and metadata.
 *
 * Coil owns decoded-memory and 64 MB disk eviction. This layer only retains a
 * small LRU of metadata and deduplicates work through Coil's cache keys.
 */
object WorkspaceImageCache {
    private const val MAX_METADATA_ENTRIES = 48
    private val metadata = LinkedHashMap<String, WorkspaceImageMetadata>(16, 0.75f, true)

    suspend fun preload(context: Context, url: String): WorkspaceImageMetadata =
        withContext(Dispatchers.IO) {
            synchronized(metadata) { metadata[url] }?.let { return@withContext it }
            val loader = SingletonImageLoader.get(context)
            val request =
                ImageRequest.Builder(context)
                    .data(url)
                    .size(Size.ORIGINAL)
                    .crossfade(false)
                    .build()
            val result = loader.execute(request)
            if (result !is SuccessResult) error("The image could not be decoded.")
            val measured =
                WorkspaceImageMetadata(
                    width = result.image.width,
                    height = result.image.height,
                    byteLength = cachedByteLength(loader, result),
                )
            synchronized(metadata) {
                metadata[url] = measured
                while (metadata.size > MAX_METADATA_ENTRIES) metadata.remove(metadata.entries.first().key)
            }
            measured
        }

    private fun cachedByteLength(loader: ImageLoader, result: SuccessResult): Long? {
        val key = result.diskCacheKey ?: return null
        return runCatching {
                loader.diskCache?.openSnapshot(key)?.use { snapshot ->
                    snapshot.data.toFile().length()
                }
            }
            .getOrNull()
    }
}
