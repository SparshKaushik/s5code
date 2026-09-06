package club.touchtech.s5code.kotlin

import android.app.Application
import coil3.ImageLoader
import coil3.PlatformContext
import coil3.SingletonImageLoader
import coil3.disk.DiskCache
import coil3.memory.MemoryCache
import coil3.request.crossfade
import club.touchtech.s5code.kotlin.data.IMAGE_CACHE_DIRECTORY
import club.touchtech.s5code.kotlin.design.text.TextMateHighlighter
import club.touchtech.s5code.kotlin.platform.notifications.PushRuntime
import club.touchtech.s5code.kotlin.platform.notifications.S5Firebase
import club.touchtech.s5code.kotlin.platform.notifications.createNotificationChannels
import okio.Path.Companion.toOkioPath

/**
 * Process entry point.
 *
 * State is owned by `AppStore`, which is a `ViewModel` so it survives configuration
 * changes without a singleton here, and Clerk is initialized by `CloudAuth` when
 * that store is created.
 *
 * The one thing that does belong here is Coil's image loader, because its disk cache
 * has to land somewhere the Client storage screen can measure and clear. Coil's
 * default is a directory under the system temp path with a library-internal name;
 * pinning it to `cacheDir/image-previews` is what makes the cache inventory honest
 * rather than a guess at another library's internals.
 */
class S5CodeApplication : Application(), SingletonImageLoader.Factory {

    override fun onCreate() {
        super.onCreate()
        TextMateHighlighter.install { path ->
            try {
                assets.open(path)
            } catch (_: Exception) {
                javaClass.classLoader?.getResourceAsStream("assets/$path")
                    ?: javaClass.classLoader?.getResourceAsStream(path)
                    ?: throw java.io.FileNotFoundException("Asset not found: $path")
            }
        }
        createNotificationChannels(this)
        val firebaseConfigured = S5Firebase.initialize(this)
        PushRuntime.initialize(this, firebaseConfigured)
    }

    override fun newImageLoader(context: PlatformContext): ImageLoader =
        ImageLoader.Builder(context)
            .memoryCache {
                MemoryCache.Builder()
                    // Image previews can be several decoded megapixels. Bound the
                    // strong cache explicitly rather than inheriting a
                    // device-dependent fraction with an unknown ceiling.
                    .maxSizeBytes(IMAGE_MEMORY_CACHE_MAX_BYTES)
                    .build()
            }
            .diskCache {
                DiskCache.Builder()
                    .directory(cacheDir.resolve(IMAGE_CACHE_DIRECTORY).toOkioPath())
                    // Bounded, as `Cache bounds` in the parity tracker requires: a
                    // workspace with a thousand screenshots must not grow this
                    // without limit on a phone.
                    .maxSizeBytes(IMAGE_CACHE_MAX_BYTES)
                    .build()
            }
            // Icons and previews replace a placeholder that is already on screen, so a
            // fade is the whole animation budget here. No continuous repaint.
            .crossfade(true)
            .build()

    private companion object {
        /** 24 MB of decoded previews, enough for a small nearby working set. */
        const val IMAGE_MEMORY_CACHE_MAX_BYTES = 24L * 1024 * 1024

        /** 64 MB, about a few hundred compressed workspace screenshots. */
        const val IMAGE_CACHE_MAX_BYTES = 64L * 1024 * 1024
    }
}
