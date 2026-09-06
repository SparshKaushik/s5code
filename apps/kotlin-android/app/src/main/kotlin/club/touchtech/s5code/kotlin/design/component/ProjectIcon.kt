package club.touchtech.s5code.kotlin.design.component

import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Folder
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.collection.LruCache
import club.touchtech.s5code.kotlin.model.Project
import coil3.compose.AsyncImage

/**
 * Project favicon with signed-URL resolution, decode state, and a bounded memory
 * cache. Coil owns disk/network caching; this cache only prevents a recycled row
 * from flashing its fallback after a favicon has already decoded once.
 */
@Composable
fun S5ProjectIcon(
    project: Project?,
    resolveUrl: suspend (Project) -> String?,
    modifier: Modifier = Modifier,
    size: Dp = 16.dp,
) {
    val identity = project?.let { "${it.environmentId.value}:${it.workspaceRoot}:${it.faviconPath}" }
    val url by
        produceState<String?>(initialValue = ProjectFaviconMemoryCache.url(identity), identity) {
            value = project?.let { runCatching { resolveUrl(it) }.getOrNull() }
            ProjectFaviconMemoryCache.rememberUrl(identity, value)
        }
    var failed by remember(identity, url) { mutableStateOf(false) }
    val cachedBitmap = ProjectFaviconMemoryCache.bitmap(identity)

    Box(modifier.size(size), contentAlignment = Alignment.Center) {
        when {
            cachedBitmap != null ->
                Image(
                    bitmap = cachedBitmap.asImageBitmap(),
                    contentDescription = project?.let { "${it.title} favicon" },
                    contentScale = ContentScale.Fit,
                    modifier = Modifier.size(size).clip(MaterialTheme.shapes.extraSmall),
                )
            url == null || failed ->
                Icon(
                    Icons.Rounded.Folder,
                    contentDescription = null,
                    modifier = Modifier.size(size),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            else ->
                AsyncImage(
                    model = url,
                    contentDescription = project?.let { "${it.title} favicon" },
                    contentScale = ContentScale.Fit,
                    onSuccess = { state ->
                        ProjectFaviconMemoryCache.rememberDrawable(identity, state.result.image)
                    },
                    onError = { failed = true },
                    modifier = Modifier.size(size).clip(MaterialTheme.shapes.extraSmall),
                )
        }
    }
}

/** Small bounded LRU. URL keys include the server's content revision. */
private object ProjectFaviconMemoryCache {
    private const val MAX_ENTRIES = 96
    private val urls = BoundedLruCache<String, String>(MAX_ENTRIES)
    private val bitmaps = BoundedLruCache<String, Bitmap>(MAX_ENTRIES)

    @Synchronized fun url(key: String?): String? = key?.let(urls::get)

    @Synchronized fun rememberUrl(key: String?, value: String?) {
        if (key != null) {
            if (value != null) urls.put(key, value) else urls.remove(key)
        }
    }

    @Synchronized fun bitmap(key: String?): Bitmap? = key?.let(bitmaps::get)

    /**
     * Coil 3's Image abstraction is platform-neutral. Its Android bitmap bridge is
     * intentionally optional: unsupported vector drawables remain in Coil's own
     * memory/disk cache and still render without being copied here.
     */
    @Synchronized
    fun rememberDrawable(key: String?, image: coil3.Image) {
        if (key == null) return
        (image as? coil3.BitmapImage)?.bitmap?.let { bitmaps.put(key, it) }
    }
}

/** Access-ordered and independently testable so the UI cache can never grow with history. */
internal class BoundedLruCache<K : Any, V : Any>(maximumSize: Int) : LruCache<K, V>(maximumSize) {
    operator fun contains(key: K): Boolean = get(key) != null
    operator fun set(key: K, value: V) { put(key, value) }
    val size: Int get() = size()
}
