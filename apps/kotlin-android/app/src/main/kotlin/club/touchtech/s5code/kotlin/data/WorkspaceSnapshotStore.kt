package club.touchtech.s5code.kotlin.data

import android.content.Context
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.transport.wire.ShellSnapshotDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadDetailPageDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadDto
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/**
 * Last complete workspace snapshots, matching the RN client's
 * `EnvironmentCacheStore` behavior for shell and opened-thread reads.
 *
 * This is intentionally a tiny file cache rather than Room: each environment has
 * one replace-whole shell document, each opened thread has one replace-whole
 * document, and there are no partial queries. Files live in `filesDir`, not
 * `cacheDir`, because Android is allowed to evict the latter while offline.
 */
class WorkspaceSnapshotStore private constructor(
    private val root: File,
) {
    constructor(context: Context) : this(File(context.filesDir, WORKSPACE_SNAPSHOT_DIRECTORY))
    private val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
        encodeDefaults = true
    }

    suspend fun loadShell(environmentId: EnvironmentId): ShellSnapshotDto? =
        read(File(environmentDirectory(environmentId), SHELL_FILE), StoredShellSnapshot.serializer())
            ?.takeIf { it.environmentId == environmentId.value }
            ?.snapshot

    suspend fun saveShell(environmentId: EnvironmentId, snapshot: ShellSnapshotDto) {
        write(
            File(environmentDirectory(environmentId), SHELL_FILE),
            StoredShellSnapshot.serializer(),
            StoredShellSnapshot(environmentId = environmentId.value, snapshot = snapshot),
        )
    }

    suspend fun loadThread(environmentId: EnvironmentId, threadId: String): CachedThreadSnapshot? =
        read(
            File(File(environmentDirectory(environmentId), THREAD_DIRECTORY), safeName(threadId)),
            StoredThreadSnapshot.serializer(),
        )
            ?.takeIf {
                it.environmentId == environmentId.value && it.threadId == threadId
            }
            ?.let { CachedThreadSnapshot(thread = it.thread, page = it.page) }

    suspend fun saveThread(
        environmentId: EnvironmentId,
        thread: ThreadDto,
        page: ThreadDetailPageDto? = null,
    ) {
        write(
            File(File(environmentDirectory(environmentId), THREAD_DIRECTORY), safeName(thread.id)),
            StoredThreadSnapshot.serializer(),
            StoredThreadSnapshot(
                environmentId = environmentId.value,
                threadId = thread.id,
                thread = thread,
                page = page,
            ),
        )
    }

    suspend fun removeThread(environmentId: EnvironmentId, threadId: String) {
        withContext(Dispatchers.IO) {
            File(File(environmentDirectory(environmentId), THREAD_DIRECTORY), safeName(threadId))
                .delete()
        }
    }

    suspend fun clear(environmentId: EnvironmentId) {
        withContext(Dispatchers.IO) {
            val directory = environmentDirectory(environmentId)
            if (!directory.deleteRecursively() && directory.exists()) {
                error("Could not clear cached workspace state.")
            }
        }
    }

    private fun environmentDirectory(environmentId: EnvironmentId): File =
        File(root, safeName(environmentId.value))

    private suspend fun <T> read(
        file: File,
        serializer: kotlinx.serialization.KSerializer<T>,
    ): T? =
        withContext(Dispatchers.IO) {
            if (!file.isFile) return@withContext null
            runCatching { json.decodeFromString(serializer, file.readText()) }
                .getOrElse {
                    // A corrupt or newer record must not trap startup. Remove only
                    // the unreadable record; the rest of the environment remains.
                    file.delete()
                    null
                }
        }

    private suspend fun <T> write(
        file: File,
        serializer: kotlinx.serialization.KSerializer<T>,
        value: T,
    ) {
        withContext(Dispatchers.IO) {
            file.parentFile?.mkdirs()
            val temporary = File(file.parentFile, "${file.name}.tmp")
            temporary.writeText(json.encodeToString(serializer, value))
            if (!temporary.renameTo(file)) {
                // Do not delete the last known-good record before a replacement is
                // guaranteed. The fallback overwrite is less atomic on an exotic
                // filesystem, but a crash can only corrupt this cache record and
                // reads already fail open.
                file.writeText(temporary.readText())
                temporary.delete()
            }
        }
    }

    @Serializable
    private data class StoredShellSnapshot(
        val schemaVersion: Int = SCHEMA_VERSION,
        val environmentId: String,
        val snapshot: ShellSnapshotDto,
    )

    @Serializable
    private data class StoredThreadSnapshot(
        val schemaVersion: Int = SCHEMA_VERSION,
        val environmentId: String,
        val threadId: String,
        val thread: ThreadDto,
        val page: ThreadDetailPageDto? = null,
    )

    data class CachedThreadSnapshot(
        val thread: ThreadDto,
        val page: ThreadDetailPageDto? = null,
    )

    companion object {
        internal fun forRoot(root: File): WorkspaceSnapshotStore = WorkspaceSnapshotStore(root)

        private const val SCHEMA_VERSION = 1
        private const val THREAD_DIRECTORY = "threads"
        private const val SHELL_FILE = "shell.json"

        private fun safeName(value: String): String =
            java.security.MessageDigest.getInstance("SHA-256")
                .digest(value.toByteArray())
                .joinToString("") { "%02x".format(it) } + ".json"
    }
}

/** Durable last-known shell and opened-thread snapshots. */
const val WORKSPACE_SNAPSHOT_DIRECTORY = "workspace-snapshots"
