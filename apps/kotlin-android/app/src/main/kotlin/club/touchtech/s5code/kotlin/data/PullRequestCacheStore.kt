package club.touchtech.s5code.kotlin.data

import android.content.Context
import java.io.File
import java.security.MessageDigest
import java.util.UUID
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.KSerializer
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement

/**
 * Last-known pull request reads, one file per (environment, query key), so the
 * Pull requests tab and an opened PR still draw while offline — the RN client's
 * persisted `pullRequests.*` query families.
 *
 * List controls live in their own file beside the cache, so clearing offline
 * pull requests in Settings keeps the filters a user picked.
 */
class PullRequestCacheStore private constructor(
    private val root: File,
    private val controlsFile: File,
) {
    constructor(context: Context) : this(
        File(context.filesDir, PULL_REQUEST_CACHE_DIRECTORY),
        File(context.filesDir, PULL_REQUEST_CONTROLS_FILE),
    )

    private val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
        encodeDefaults = true
    }

    data class Entry(val fetchedAt: Long, val value: JsonElement)

    /** One writer at a time, so overlapping writes cannot interleave or reorder. */
    private val writes = Mutex()

    /** Newest `fetchedAt` written per file, so a slower, older fetch never overwrites a newer one. */
    private val writtenAt = HashMap<String, Long>()

    suspend fun read(environmentId: String, key: String): Entry? =
        withContext(Dispatchers.IO) {
            val file = File(File(root, safeName(environmentId)), safeName(key))
            if (!file.isFile) return@withContext null
            runCatching { json.decodeFromString(StoredEntry.serializer(), file.readText()) }
                .getOrElse {
                    // An unreadable record must not pin the screen to an error.
                    file.delete()
                    null
                }
                ?.takeIf { it.schemaVersion == SCHEMA_VERSION && it.environmentId == environmentId && it.key == key }
                ?.let { Entry(it.fetchedAt, it.value) }
        }

    suspend fun write(environmentId: String, key: String, fetchedAt: Long, value: JsonElement) {
        writes.withLock {
            withContext(Dispatchers.IO) {
                val directory = File(root, safeName(environmentId))
                val file = File(directory, safeName(key))
                if ((writtenAt[file.path] ?: Long.MIN_VALUE) > fetchedAt) return@withContext
                writtenAt[file.path] = fetchedAt
                writeAtomically(
                    file,
                    StoredEntry.serializer(),
                    StoredEntry(environmentId = environmentId, key = key, fetchedAt = fetchedAt, value = value),
                )
                prune(directory)
            }
        }
    }

    suspend fun readListState(): PullRequestListState =
        withContext(Dispatchers.IO) {
            if (!controlsFile.isFile) return@withContext PullRequestListState()
            runCatching { json.decodeFromString(PullRequestListState.serializer(), controlsFile.readText()) }
                .getOrDefault(PullRequestListState())
        }

    suspend fun writeListState(state: PullRequestListState) {
        writes.withLock {
            withContext(Dispatchers.IO) { writeAtomically(controlsFile, PullRequestListState.serializer(), state) }
        }
    }

    /** Pages a user scrolled long ago go first; each environment keeps its newest [MAX_ENTRIES]. */
    private fun prune(directory: File) {
        val files = directory.listFiles { file -> file.isFile && file.name.endsWith(".json") } ?: return
        if (files.size <= MAX_ENTRIES) return
        files.sortedByDescending { it.lastModified() }.drop(MAX_ENTRIES).forEach { it.delete() }
    }

    private fun <T> writeAtomically(file: File, serializer: KSerializer<T>, value: T) {
        file.parentFile?.mkdirs()
        // A unique name, so a write cannot rename another one's half-written file.
        val temporary = File(file.parentFile, "${file.name}.${UUID.randomUUID()}.tmp")
        try {
            temporary.writeText(json.encodeToString(serializer, value))
            if (!temporary.renameTo(file)) file.writeText(temporary.readText())
        } finally {
            temporary.delete()
        }
    }

    @Serializable
    private data class StoredEntry(
        val schemaVersion: Int = SCHEMA_VERSION,
        val environmentId: String,
        val key: String,
        val fetchedAt: Long,
        val value: JsonElement,
    )

    companion object {
        private const val SCHEMA_VERSION = 1
        private const val MAX_ENTRIES = 200

        internal fun forRoot(root: File): PullRequestCacheStore =
            PullRequestCacheStore(File(root, PULL_REQUEST_CACHE_DIRECTORY), File(root, PULL_REQUEST_CONTROLS_FILE))

        private fun safeName(value: String): String =
            MessageDigest.getInstance("SHA-256")
                .digest(value.toByteArray())
                .joinToString("") { "%02x".format(it) } + ".json"
    }
}

/** What the Pull requests tab remembers between launches, kept out of the clearable cache. */
@Serializable
data class PullRequestListState(
    /** The environment the tab is scoped to; null for all of them. */
    val environmentId: String? = null,
    /** Keyed by environment id, "" for the all-environments scope. */
    val controls: Map<String, PullRequestListControls> = emptyMap(),
    /**
     * Environments seen serving pull requests, so the tab and its saved pages
     * stay reachable while they are offline and their capabilities unknown.
     */
    val environments: Set<String> = emptySet(),
)

/** Durable last-known pull request reads, cleared from Settings → Offline data. */
const val PULL_REQUEST_CACHE_DIRECTORY = "pull-request-cache"

private const val PULL_REQUEST_CONTROLS_FILE = "pull-request-list-filters.json"
