package club.touchtech.s5code.kotlin.data

/**
 * On-device cache inventory.
 *
 * The client persists four things: composer attachment copies, Coil's image disk
 * cache, durable workspace snapshots, and the client state file (drafts and
 * preferences, which is not a cache and never appears here). Diffs, highlighted
 * lines, and terminal scrollback remain memory-only.
 *
 * This file exists because the Client storage screen used to list four categories
 * with hardcoded megabyte figures and a clear button that only set local state. A
 * settings screen that reports invented numbers and does nothing is worse than not
 * having the screen: the user clears it, believes the space came back, and the app
 * has lied twice.
 */

/** What a cache category is, independent of where its bytes live. */
enum class ClientCacheKind {
    /** Images copied out of a clipboard/picker grant so a draft stays sendable. */
    Attachments,

    /** Coil's disk cache for project icons and workspace image previews. */
    Images,

    /** Last-known shell rows and opened transcripts, scoped per environment. */
    Workspace,
}

/** One measured category. [bytes] is real, measured from the filesystem. */
data class ClientCacheEntry(val kind: ClientCacheKind, val bytes: Long) {
    val label: String
        get() =
            when (kind) {
                ClientCacheKind.Attachments -> "Draft attachments"
                ClientCacheKind.Images -> "Image previews"
                ClientCacheKind.Workspace -> "Offline chats"
            }

    val detail: String
        get() =
            when (kind) {
                ClientCacheKind.Attachments ->
                    "Copies of images attached to unsent drafts. Cleared automatically after a day."
                ClientCacheKind.Images ->
                    "Project icons and workspace image previews, refetched on demand."
                ClientCacheKind.Workspace ->
                    "Last-known project, chat-list, and opened transcript snapshots."
            }
}

/** Every category, in a fixed order, so the list does not reshuffle as sizes change. */
data class ClientCacheInventory(val entries: List<ClientCacheEntry>) {
    val totalBytes: Long
        get() = entries.sumOf { it.bytes }

    val isEmpty: Boolean
        get() = totalBytes == 0L
}

/**
 * Byte count as a settings row reads it.
 *
 * Deliberately coarse — one decimal place, binary units — because the number is
 * only ever used to answer "is this worth clearing". Zero says "Empty" rather than
 * "0 B": a row that reads "0 B" invites a tap that does nothing.
 */
fun cacheSizeLabel(bytes: Long): String {
    if (bytes <= 0) return "Empty"
    if (bytes < 1024) return "$bytes B"
    val units = listOf("KB", "MB", "GB")
    var value = bytes.toDouble() / 1024
    var unit = 0
    while (value >= 1024 && unit < units.lastIndex) {
        value /= 1024
        unit += 1
    }
    // Whole numbers lose the decimal: "2 MB" reads better than "2.0 MB", and the
    // extra digit is noise at this precision anyway.
    return if (value >= 100 || value == Math.floor(value)) {
        "${Math.round(value)} ${units[unit]}"
    } else {
        String.format(java.util.Locale.US, "%.1f %s", value, units[unit])
    }
}
