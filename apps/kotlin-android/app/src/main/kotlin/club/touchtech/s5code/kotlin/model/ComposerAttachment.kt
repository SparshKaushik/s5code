package club.touchtech.s5code.kotlin.model

import androidx.compose.runtime.Immutable

/**
 * Send-turn attachment limits. These mirror `PROVIDER_SEND_TURN_*` in
 * `packages/contracts`: the server rejects anything outside them, so a client
 * that validates first never builds a turn the server will refuse.
 */
object ComposerAttachmentLimits {
    const val MAX_ATTACHMENTS = 8
    const val MAX_IMAGE_BYTES = 10L * 1024 * 1024
    val SUPPORTED_IMAGE_MIME_TYPES = setOf("image/gif", "image/jpeg", "image/png", "image/webp")
}

/** One image accepted into a composer draft or already sent on a turn. */
@Immutable
data class ComposerAttachment(
    val id: String,
    val name: String,
    val mimeType: String,
    val sizeBytes: Long,
    val uri: String,
)

/**
 * An image offered to a composer by the clipboard, the keyboard, a drop, or the
 * photo picker, before validation. [sizeBytes] is null when the provider does
 * not report a size.
 */
data class ComposerImageCandidate(
    val uri: String,
    val mimeType: String?,
    val name: String? = null,
    val sizeBytes: Long? = null,
)
