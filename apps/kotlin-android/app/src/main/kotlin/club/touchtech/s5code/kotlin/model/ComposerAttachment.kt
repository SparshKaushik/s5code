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
    /** `PROVIDER_SEND_TURN_MAX_FILE_BYTES` — the client's own cap for non-image uploads. */
    const val MAX_FILE_BYTES = 50L * 1024 * 1024
    val SUPPORTED_IMAGE_MIME_TYPES = setOf("image/gif", "image/jpeg", "image/png", "image/webp")
}

/**
 * One image accepted into a composer draft, or one attachment already sent on
 * a turn. [type] is the `ChatAttachment` discriminator — "image" or "file" —
 * which decides whether a transcript chip renders a thumbnail or opens the
 * attachment viewer.
 */
@Immutable
data class ComposerAttachment(
    val id: String,
    val name: String,
    val mimeType: String,
    val sizeBytes: Long,
    val uri: String,
    val type: String = "image",
)

/**
 * An attachment that already reached the server, as `attachmentsByQuestionId`
 * and sent-message attachments carry it: [id] is the server-issued attachment
 * id, so there are no local bytes here — only the fields needed to sign a
 * download URL and label the file.
 */
data class SentAttachment(
    val id: String,
    val name: String,
    val mimeType: String,
    val sizeBytes: Long,
    val type: String = "file",
)

/** "image" | "file" — the picker buckets `attachments.createUploadUrl` accepts. */
enum class QuestionAttachmentKind(val wireValue: String) {
    Image("image"),
    File("file"),
}

enum class QuestionAttachmentStatus {
    Uploading,
    Ready,
    Failed,
}

/**
 * One file staged on a pending user-input question. The local copy is what the
 * strip previews; [uploadedAttachmentId] appears once the bytes are on the
 * server and is what `attachmentsByQuestionId` references.
 */
@Immutable
data class QuestionAttachment(
    /** Client-local id, distinct from the upload id the server mints. */
    val localId: String,
    val name: String,
    val mimeType: String,
    val sizeBytes: Long,
    val kind: QuestionAttachmentKind,
    /** App-owned cache copy; survives the picker's lapsing URI grant. */
    val localPath: String,
    val status: QuestionAttachmentStatus,
    val uploadedAttachmentId: String? = null,
    val error: String? = null,
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
