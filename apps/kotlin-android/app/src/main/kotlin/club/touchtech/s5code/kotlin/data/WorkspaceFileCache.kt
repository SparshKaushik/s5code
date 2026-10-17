package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.SourceFile
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.WorkspaceAsset
import java.util.LinkedHashMap
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * Small process-local workspace-file cache, shared by prewarm and viewer reads.
 *
 * Signed URLs are retained only until shortly before their server expiry. Source
 * documents are bounded by entry count and character count, so hovering through a
 * monorepo cannot turn prewarming into unbounded process memory.
 */
internal class WorkspaceFileCache(
    private val maxSourceEntries: Int = 24,
    private val maxSourceCharacters: Int = 2 * 1024 * 1024,
    private val now: () -> Long = System::currentTimeMillis,
) {
    private data class SourceEntry(val file: SourceFile, val characters: Int)

    private val mutex = Mutex()
    private val sources = LinkedHashMap<String, SourceEntry>(16, 0.75f, true)
    private val assets = LinkedHashMap<String, WorkspaceAsset>(16, 0.75f, true)
    private val sourceLoads = mutableMapOf<String, Deferred<SourceFile>>()
    private val assetLoads = mutableMapOf<String, Deferred<WorkspaceAsset>>()
    private var sourceCharacters = 0

    suspend fun source(
        environmentId: EnvironmentId,
        threadId: ThreadId,
        path: String,
        load: suspend () -> SourceFile,
    ): SourceFile {
        val key = key(environmentId, threadId, path)
        var leader = false
        val deferred =
            mutex.withLock {
                sources[key]?.file?.let { return it }
                sourceLoads[key]
                    ?: CompletableDeferred<SourceFile>().also {
                        sourceLoads[key] = it
                        leader = true
                    }
            }
        if (!leader) return deferred.await()
        return try {
            load().also { file ->
                mutex.withLock {
                    sourceLoads.remove(key)
                    putSource(key, file)
                    (deferred as CompletableDeferred).complete(file)
                }
            }
        } catch (cause: Throwable) {
            mutex.withLock {
                sourceLoads.remove(key)
                (deferred as CompletableDeferred).completeExceptionally(cause)
            }
            throw cause
        }
    }

    suspend fun asset(
        environmentId: EnvironmentId,
        threadId: ThreadId,
        path: String,
        load: suspend () -> WorkspaceAsset,
    ): WorkspaceAsset {
        val key = key(environmentId, threadId, path)
        var leader = false
        val deferred =
            mutex.withLock {
                assets[key]?.takeIf { it.expiresAtMillis - EXPIRY_SAFETY_MILLIS > now() }
                    ?.let { return it }
                assetLoads[key]
                    ?: CompletableDeferred<WorkspaceAsset>().also {
                        assetLoads[key] = it
                        leader = true
                    }
            }
        if (!leader) return deferred.await()
        return try {
            load().also { asset ->
                mutex.withLock {
                    assetLoads.remove(key)
                    assets[key] = asset
                    trimAssets()
                    (deferred as CompletableDeferred).complete(asset)
                }
            }
        } catch (cause: Throwable) {
            mutex.withLock {
                assetLoads.remove(key)
                (deferred as CompletableDeferred).completeExceptionally(cause)
            }
            throw cause
        }
    }

    private fun putSource(key: String, file: SourceFile) {
        sources.remove(key)?.let { sourceCharacters -= it.characters }
        val characters = file.lines.sumOf(String::length)
        if (characters > maxSourceCharacters || file.byteLength > MAX_PREWARM_SOURCE_BYTES) return
        sources[key] = SourceEntry(file, characters)
        sourceCharacters += characters
        while (sources.size > maxSourceEntries || sourceCharacters > maxSourceCharacters) {
            val eldest = sources.entries.firstOrNull() ?: break
            sources.remove(eldest.key)
            sourceCharacters -= eldest.value.characters
        }
    }

    private fun trimAssets() {
        val expired = assets.filterValues { it.expiresAtMillis - EXPIRY_SAFETY_MILLIS <= now() }.keys
        expired.forEach(assets::remove)
        while (assets.size > MAX_ASSET_ENTRIES) assets.remove(assets.entries.first().key)
    }

    private fun key(environmentId: EnvironmentId, threadId: ThreadId, path: String) =
        "${environmentId.value}\u0000${threadId.value}\u0000$path"

    companion object {
        /** RN's per-file syntax preload limit; Kotlin retains the same bounded read. */
        private const val MAX_PREWARM_SOURCE_BYTES = 256 * 1024L
        private const val MAX_ASSET_ENTRIES = 48
        private const val EXPIRY_SAFETY_MILLIS = 60_000L
    }
}
