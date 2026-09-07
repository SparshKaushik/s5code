package club.touchtech.s5code.kotlin.data

import android.content.Context
import coil3.SingletonImageLoader
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Measures and clears the client's on-device caches.
 *
 * Attachment and image categories live in `cacheDir`; durable workspace snapshots
 * live in `filesDir` because Android may evict cache storage while the device is
 * offline. All three are measured through the same filesystem walk.
 *
 * Everything here runs on IO. Walking a cache directory on the main thread is a
 * dropped frame on the settings screen for no benefit.
 */
class ClientCacheStore(private val context: Context) {

    suspend fun inventory(): ClientCacheInventory =
        withContext(Dispatchers.IO) {
            ClientCacheInventory(
                entries =
                    ClientCacheKind.entries.map { kind ->
                        ClientCacheEntry(kind = kind, bytes = directoryFor(kind).sizeOnDisk())
                    }
            )
        }

    /** Clears one category and returns the inventory as it now stands. */
    suspend fun clear(kind: ClientCacheKind): ClientCacheInventory {
        withContext(Dispatchers.IO) {
            when (kind) {
                // Through Coil rather than by deleting the directory: the loader keeps
                // an open journal, and pulling the files out from under it leaves it
                // writing into a cache that no longer exists.
                ClientCacheKind.Images -> SingletonImageLoader.get(context).diskCache?.clear()
                ClientCacheKind.Attachments,
                ClientCacheKind.Workspace -> directoryFor(kind).deleteRecursively()
            }
        }
        return inventory()
    }

    /** Clears every category. */
    suspend fun clearAll(): ClientCacheInventory {
        ClientCacheKind.entries.forEach { clear(it) }
        return inventory()
    }

    private fun directoryFor(kind: ClientCacheKind): File =
        when (kind) {
            ClientCacheKind.Attachments -> File(context.cacheDir, COMPOSER_ATTACHMENT_CACHE_DIRECTORY)
            ClientCacheKind.Images -> File(context.cacheDir, IMAGE_CACHE_DIRECTORY)
            ClientCacheKind.Workspace -> File(context.filesDir, WORKSPACE_SNAPSHOT_DIRECTORY)
        }

    private fun File.sizeOnDisk(): Long =
        if (!exists()) 0L else walkBottomUp().filter { it.isFile }.sumOf { it.length() }
}

/**
 * Where Coil's disk cache lives.
 *
 * Set explicitly rather than left to Coil's default (`coil3_disk_cache` under the
 * system temp directory) so the Client storage screen can name a directory it is
 * certain about instead of guessing at the library's internals.
 */
const val IMAGE_CACHE_DIRECTORY = "image-previews"

/** Where composer attachment copies live. See `platform/ComposerImageIntake.kt`. */
const val COMPOSER_ATTACHMENT_CACHE_DIRECTORY = "composer-attachments"
