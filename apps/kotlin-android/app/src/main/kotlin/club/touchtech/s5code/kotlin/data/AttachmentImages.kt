package club.touchtech.s5code.kotlin.data

import android.content.Context
import coil3.request.ImageRequest
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive

/**
 * Coil cache key for a stored attachment, `assetImageSource` in RN. Every mint
 * signs a new URL and a reconnect mints again, so a URL-keyed cache misses each
 * time; attachments never change once stored, so they cache under their id.
 */
fun attachmentImageCacheKey(environmentId: String, attachmentId: String): String =
    "t3-attachment:" + JsonArray(listOf(JsonPrimitive(environmentId), JsonPrimitive(attachmentId)))

/** A signed attachment [url] as a Coil model keyed by [attachmentImageCacheKey]. */
fun attachmentImageRequest(
    context: Context,
    environmentId: String,
    attachmentId: String,
    url: String,
): ImageRequest {
    val key = attachmentImageCacheKey(environmentId, attachmentId)
    return ImageRequest.Builder(context).data(url).memoryCacheKey(key).diskCacheKey(key).build()
}
