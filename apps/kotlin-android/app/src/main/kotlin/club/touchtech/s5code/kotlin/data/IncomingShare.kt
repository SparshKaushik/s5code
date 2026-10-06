package club.touchtech.s5code.kotlin.data

import java.security.MessageDigest
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

/**
 * One item handed to the app through the system sharesheet, waiting to become a
 * new-task draft attachment.
 *
 * Mirrors `IncomingShareDraft` in
 * `apps/mobile/src/features/sharing/incoming-share-model.ts`. Attachments keep a
 * `type` even though this build only sends images: the RN draft can carry
 * `file`/`audio`/`video` payloads gated on the destination server's
 * `fileAttachments` capability, and the Kotlin draft needs the same field to
 * answer "is this share sendable here" rather than dropping the record.
 */
@Serializable
data class IncomingShareDraft(
    val schemaVersion: Int = 1,
    val id: String,
    val createdAt: String,
    val destination: IncomingShareDestination? = null,
    val text: String = "",
    val attachments: List<IncomingShareAttachment> = emptyList(),
    val warnings: List<String> = emptyList(),
)

/** The project a share is reserved for, once the user picked one. */
@Serializable
data class IncomingShareDestination(val environmentId: String, val projectId: String)

enum class IncomingShareAttachmentType {
    Image,
    File,
}

@Serializable
data class IncomingShareAttachment(
    val id: String,
    val type: IncomingShareAttachmentType,
    val name: String,
    val mimeType: String,
    val sizeBytes: Long,
    /**
     * A `file:` URI inside the share's inbox directory. Payloads are copied at
     * ingest because the sender's `content:` grant can lapse before the user
     * finishes the draft.
     */
    val uri: String,
)

/** `PROVIDER_SEND_TURN_MAX_FILE_BYTES` in `packages/contracts`: the file ceiling. */
const val INCOMING_SHARE_MAX_FILE_BYTES: Long = 50L * 1024 * 1024

fun hasIncomingShareContent(draft: IncomingShareDraft): Boolean =
    draft.text.isNotBlank() || draft.attachments.isNotEmpty()

/**
 * Content-derived share id, matching RN's `incomingShareIdForPayloads`: hashing
 * the normalized payload means a re-delivery of the same share (an intent
 * replayed after process death, or a double tap) lands on the existing inbox
 * item instead of importing twice.
 */
fun incomingShareIdFor(text: String?, mimeType: String?, uris: List<String>): String {
    val fingerprint =
        buildJsonObject {
                put("text", text.orEmpty().trim())
                put("mimeType", mimeType)
                putJsonArray("uris") {
                    uris.forEach { add(JsonPrimitive(it)) }
                }
            }
            .toString()
    val digest = MessageDigest.getInstance("SHA-256").digest(fingerprint.toByteArray())
    return "share-" + digest.joinToString("") { "%02x".format(it) }
}

/**
 * Which of a share's attachments the chosen destination can take, and what to
 * say about the ones it cannot — `selectIncomingShareAttachments` in the RN
 * client. [maxFileBytes] is the server's `fileAttachments.maxUploadBytes`; null
 * means the server predates file attachments and only accepts images.
 */
fun selectIncomingShareAttachments(
    attachments: List<IncomingShareAttachment>,
    maxFileBytes: Long?,
): Pair<List<IncomingShareAttachment>, List<String>> {
    val kept = mutableListOf<IncomingShareAttachment>()
    val warnings = mutableListOf<String>()
    for (attachment in attachments) {
        if (attachment.type == IncomingShareAttachmentType.Image) {
            kept += attachment
            continue
        }
        if (maxFileBytes == null) {
            warnings += "'${attachment.name}' was skipped because this server does not support files."
            continue
        }
        val ceiling = maxFileBytes.coerceAtMost(INCOMING_SHARE_MAX_FILE_BYTES)
        if (attachment.sizeBytes > ceiling) {
            warnings += fileTooLargeMessage(attachment.name, ceiling)
            continue
        }
        kept += attachment
    }
    return kept to warnings
}

fun fileTooLargeMessage(name: String, maxBytes: Long): String =
    "'$name' exceeds the ${maxBytes / (1024 * 1024)} MB attachment limit."

/** Newest first, one row per id — `sortAndDedupeIncomingShares` in the RN client. */
fun sortAndDedupeIncomingShares(
    drafts: List<IncomingShareDraft>,
): List<IncomingShareDraft> =
    drafts.sortedByDescending { it.createdAt }.distinctBy { it.id }

/* ── Presentation ────────────────────────────────────────────────────── */

/**
 * Whether the pending-share navigation has already shown (or been dismissed
 * from) the new-task flow, ported from `incoming-share-presentation.ts`. The
 * latch is what stops a back-out from re-presenting the picker forever.
 */
data class IncomingSharePresentationState(
    val presentedShareId: String? = null,
    val dismissedShareId: String? = null,
)

data class IncomingSharePresentationTransition(
    val state: IncomingSharePresentationState,
    val shareIdToPresent: String?,
)

fun transitionIncomingSharePresentation(
    state: IncomingSharePresentationState,
    isInShareFlow: Boolean,
    isOnProjectPicker: Boolean,
    pendingShareId: String?,
): IncomingSharePresentationTransition {
    if (isInShareFlow) {
        if (state.presentedShareId != null && pendingShareId != state.presentedShareId) {
            // The share was consumed while the flow stayed open; forget the
            // presentation so a later handoff may reuse its slot.
            return IncomingSharePresentationTransition(IncomingSharePresentationState(), null)
        }
        // The picker is where a presented share lands, so arriving there by any
        // path (including the deep link that ingested it) latches the id: a
        // back-out then counts as a dismissal instead of a re-presentation.
        if (isOnProjectPicker && state.presentedShareId == null) {
            return IncomingSharePresentationTransition(
                state.copy(presentedShareId = pendingShareId),
                null,
            )
        }
        return IncomingSharePresentationTransition(state, null)
    }

    var next = state
    if (state.presentedShareId != null) {
        if (pendingShareId == state.presentedShareId) {
            return IncomingSharePresentationTransition(
                state.copy(presentedShareId = null, dismissedShareId = state.presentedShareId),
                null,
            )
        }
        next = next.copy(presentedShareId = null)
    }

    if (pendingShareId == null) {
        return IncomingSharePresentationTransition(IncomingSharePresentationState(), null)
    }
    if (next.dismissedShareId == pendingShareId) {
        return IncomingSharePresentationTransition(next, null)
    }
    return IncomingSharePresentationTransition(
        IncomingSharePresentationState(presentedShareId = pendingShareId),
        pendingShareId,
    )
}
