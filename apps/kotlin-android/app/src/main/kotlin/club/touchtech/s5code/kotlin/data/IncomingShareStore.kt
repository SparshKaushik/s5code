package club.touchtech.s5code.kotlin.data

import android.content.Context
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json

/**
 * Durable inbox for sharesheet payloads, under `filesDir/incoming-shares/`.
 *
 * One directory per share: `<id>/draft.json` plus the payload files the draft's
 * attachments point at. Keeping the payloads beside the record (rather than in
 * the composer cache) means the inbox owns cleanup end to end and the copy
 * survives the cache's daily prune for as long as the share is pending.
 *
 * Every mutation runs under [mutex] and republishes [drafts] from disk, so a
 * stale in-memory list can never resurrect a consumed share — the same property
 * `IncomingShareInbox`'s serialized queue gives the RN client.
 */
class IncomingShareStore(context: Context) {

    private val directory = File(context.filesDir, "incoming-shares")
    private val json = Json { ignoreUnknownKeys = true }
    private val mutex = Mutex()

    private val _drafts = MutableStateFlow<List<IncomingShareDraft>>(emptyList())
    val drafts: StateFlow<List<IncomingShareDraft>> = _drafts.asStateFlow()

    /** Reloads the inbox from disk. Safe to call on launch and per share intent. */
    suspend fun refresh() = locked { reload() }

    /** Reads the inbox without republishing, for call sites already holding state. */
    suspend fun draftsNow() = locked { loadAll() }

    /**
     * Persists a built draft. An existing id replaces its file, which is what
     * makes the content-hash id an idempotency key for re-delivered shares.
     */
    suspend fun write(draft: IncomingShareDraft) = locked {
        writeLocked(draft)
        reload()
    }

    /** Deletes the share's record and payload files. Idempotent. */
    suspend fun consume(shareId: String) = locked {
        shareDirectory(shareId).deleteRecursively()
        reload()
    }

    /**
     * Pins a share to the project the user chose so an interrupted import can
     * only resume into that draft — matching `IncomingShareInbox.reserve`.
     * Returns the persisted list, or null when the share is already reserved for
     * a different destination or no longer exists.
     */
    suspend fun reserve(shareId: String, destination: IncomingShareDestination) =
        locked<List<IncomingShareDraft>?> {
            val persisted = loadAll()
            val target = persisted.firstOrNull { it.id == shareId } ?: return@locked null
            val existing = target.destination
            if (existing != null && existing != destination) return@locked null
            if (existing == null) writeLocked(target.copy(destination = destination))
            reload()
        }

    /**
     * Drops a reservation, but only the one the caller thinks it holds — a
     * conditional release, so racing a consume is a no-op rather than a rewrite
     * of a share another draft picked up.
     */
    suspend fun releaseReservation(
        shareId: String,
        expectedDestination: IncomingShareDestination,
    ) = locked {
        val persisted = loadAll()
        val target = persisted.firstOrNull { it.id == shareId }
        if (target != null && target.destination == expectedDestination) {
            writeLocked(target.copy(destination = null))
        }
        reload()
    }

    /** The share's own directory; ingest creates it before copying payloads. */
    fun shareDirectory(shareId: String): File =
        File(directory, sanitizeFileName(shareId))

    private fun writeLocked(draft: IncomingShareDraft) {
        val dir = shareDirectory(draft.id).apply { mkdirs() }
        File(dir, "draft.json").writeText(json.encodeToString(draft))
    }

    private fun reload(): List<IncomingShareDraft> =
        loadAll().also { _drafts.value = it }

    private fun loadAll(): List<IncomingShareDraft> {
        directory.mkdirs()
        val loaded =
            directory.listFiles().orEmpty().mapNotNull { dir ->
                val file = File(dir, "draft.json")
                if (!dir.isDirectory || !file.isFile) return@mapNotNull null
                runCatching { json.decodeFromString<IncomingShareDraft>(file.readText()) }
                    // A file this build cannot read is ignored rather than
                    // fatal: one malformed share must not swallow the inbox.
                    .getOrNull()
            }
        return sortAndDedupeIncomingShares(loaded)
    }

    private suspend fun <T> locked(block: () -> T): T =
        mutex.withLock { withContext(Dispatchers.IO) { block() } }

    private fun sanitizeFileName(shareId: String): String =
        // Ids are `share-<sha256>`; the whitelist keeps any future id safe as a
        // path segment.
        shareId.filter { it.isLetterOrDigit() || it == '-' || it == '_' }.ifBlank { "share" }
}
