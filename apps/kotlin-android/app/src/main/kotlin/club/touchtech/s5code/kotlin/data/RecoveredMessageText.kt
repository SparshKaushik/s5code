package club.touchtech.s5code.kotlin.data

/** Restore a rejected send without discarding text typed while it was queued. */
internal fun recoveredMessageText(draft: String, rejected: String): String =
    when {
        rejected.isBlank() || draft == rejected -> draft
        draft.isBlank() -> rejected
        else -> "$draft\n\n$rejected"
    }
