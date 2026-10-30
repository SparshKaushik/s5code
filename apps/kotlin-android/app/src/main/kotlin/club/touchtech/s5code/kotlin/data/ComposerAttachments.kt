package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.ComposerAttachmentLimits
import club.touchtech.s5code.kotlin.model.ComposerImageCandidate

/**
 * Accepted images plus the message to show for whatever was refused. Callers
 * append the attachments and surface the error; both can be non-empty for one
 * paste, so a bad image never silently drops the good ones beside it.
 */
data class ComposerAttachmentResult(
    val attachments: List<ComposerAttachment>,
    val error: String? = null,
)

/**
 * Validates pasted, dropped, or picked images against the send-turn limits and
 * skips URIs already in the draft. Rejections keep the newest reason, because a
 * single notice is all the composer shows.
 */
fun acceptComposerImages(
    existing: List<ComposerAttachment>,
    candidates: List<ComposerImageCandidate>,
): ComposerAttachmentResult {
    if (candidates.isEmpty()) return ComposerAttachmentResult(emptyList())
    if (existing.size >= ComposerAttachmentLimits.MAX_ATTACHMENTS) {
        return ComposerAttachmentResult(
            emptyList(),
            "You can attach up to ${ComposerAttachmentLimits.MAX_ATTACHMENTS} images per message.",
        )
    }

    val knownUris = existing.mapTo(mutableSetOf()) { it.uri }
    val accepted = mutableListOf<ComposerAttachment>()
    var error: String? = null

    for (candidate in candidates) {
        if (!knownUris.add(candidate.uri)) continue

        val name = candidate.name?.takeIf { it.isNotBlank() } ?: displayNameOf(candidate)
        val mimeType = candidate.mimeType?.lowercase()

        if (mimeType == null || mimeType !in ComposerAttachmentLimits.SUPPORTED_IMAGE_MIME_TYPES) {
            error = "'$name' is not a supported image type. Attach GIF, JPEG, PNG, or WebP images."
            continue
        }
        if (candidate.sizeBytes != null &&
            (candidate.sizeBytes <= 0L ||
                candidate.sizeBytes > ComposerAttachmentLimits.MAX_IMAGE_BYTES)
        ) {
            error = "'$name' exceeds the 10 MB attachment limit."
            continue
        }
        if (existing.size + accepted.size >= ComposerAttachmentLimits.MAX_ATTACHMENTS) {
            error =
                "You can attach up to ${ComposerAttachmentLimits.MAX_ATTACHMENTS} images per message."
            continue
        }

        accepted +=
            ComposerAttachment(
                id = candidate.uri,
                name = name,
                mimeType = mimeType,
                sizeBytes = candidate.sizeBytes ?: 0L,
                uri = candidate.uri,
            )
    }

    return ComposerAttachmentResult(accepted, error)
}

/** Falls back to the URI's file name, then to a generic pasted-image label. */
private fun displayNameOf(candidate: ComposerImageCandidate): String {
    val segment = candidate.uri.substringBefore('?').substringAfterLast('/')
    if (segment.contains('.') && segment.length in 2..255) return segment
    val extension =
        when (candidate.mimeType?.lowercase()) {
            "image/gif" -> "gif"
            "image/jpeg" -> "jpg"
            "image/webp" -> "webp"
            else -> "png"
        }
    return "pasted-image.$extension"
}
