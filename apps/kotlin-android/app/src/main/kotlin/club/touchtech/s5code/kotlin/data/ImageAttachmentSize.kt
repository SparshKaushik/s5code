package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.sqrt

/**
 * Anthropic rejects an image over 2000px on a side once a request holds more
 * than 20 images, and Claude Code forwards images sent into a running turn
 * unchanged, so one large paste breaks every later request in that thread.
 * Ported from `packages/client-runtime/src/imageAttachmentSize.ts`.
 */
const val IMAGE_ATTACHMENT_MAX_EDGE = 2000

/** The formats intake re-encodes; GIF and WebP pass through untouched. */
fun downscalableImageMimeType(mimeType: String): String? =
    when (mimeType.lowercase()) {
        "image/png" -> "image/png"
        "image/jpeg", "image/jpg" -> "image/jpeg"
        else -> null
    }

/** The size an image must shrink to, or null when its longest edge already fits. */
fun fitImageAttachmentSize(width: Int, height: Int): Pair<Int, Int>? {
    val longestEdge = max(width, height)
    if (longestEdge <= IMAGE_ATTACHMENT_MAX_EDGE) return null
    val scale = IMAGE_ATTACHMENT_MAX_EDGE.toDouble() / longestEdge
    // JS Math.round: half rounds up.
    fun fit(edge: Int) = floor(edge * scale + 0.5).toInt().coerceIn(1, IMAGE_ATTACHMENT_MAX_EDGE)
    return fit(width) to fit(height)
}

/** How many times intake shrinks a re-encode that is still over the byte limit. */
const val MAX_BYTE_LIMIT_SHRINKS = 3

/**
 * `shrinkToByteLimit` in mobile `composerImageFit.ts`: the smaller size to try
 * when an encode is over the image byte limit, or null when it fits or cannot
 * shrink. PNG ignores quality, so a resampled PNG can outweigh its original;
 * encoded size tracks pixel count, so each edge scales by the square root.
 */
fun shrinkImageToByteLimit(
    width: Int,
    height: Int,
    bytes: Long,
    limit: Long = ComposerAttachmentLimits.MAX_IMAGE_BYTES,
): Pair<Int, Int>? {
    if (bytes <= limit) return null
    val scale = sqrt(limit.toDouble() / bytes) * 0.9
    val next = max(1, floor(width * scale).toInt()) to max(1, floor(height * scale).toInt())
    return if (next.first < width || next.second < height) next else null
}
