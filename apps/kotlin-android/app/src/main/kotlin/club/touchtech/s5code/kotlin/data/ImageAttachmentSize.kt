package club.touchtech.s5code.kotlin.data

import kotlin.math.floor
import kotlin.math.max

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
