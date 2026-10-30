package club.touchtech.s5code.kotlin.data

import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/**
 * Unsubmitted pull request reviews — summary, top-level comment and pending
 * line comments — keyed by [pullRequestReviewKey]. Kept on this device only,
 * like RN's `ReviewDraftDocument`; empty drafts are dropped on write.
 *
 * Not part of the clearable pull request cache: clearing offline reads must
 * not discard someone's unsent review.
 */
class PullRequestReviewDraftStore(private val file: File, private val scope: CoroutineScope) {
    private val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
    }
    private val state = MutableStateFlow<Map<String, ReviewDraft>>(emptyMap())
    private val loaded = MutableStateFlow(false)
    private val writes = Mutex()

    /** All drafts by key; empty until [ready]. */
    val drafts: StateFlow<Map<String, ReviewDraft>> = state.asStateFlow()

    /** Edits wait for the file so they cannot overwrite drafts it still holds. */
    val ready: StateFlow<Boolean> = loaded.asStateFlow()

    init {
        scope.launch {
            val stored =
                withContext(Dispatchers.IO) {
                    if (!file.isFile) return@withContext null
                    val document =
                        runCatching { json.decodeFromString(Document.serializer(), file.readText()) }
                            .getOrNull()
                            ?.takeIf { it.version == VERSION }
                    // A damaged or newer document is set aside, never overwritten by the next edit.
                    if (document == null) file.renameTo(File(file.parentFile, "${file.name}.damaged"))
                    document
                }
            stored?.let { state.value = it.drafts }
            loaded.value = true
        }
    }

    fun draft(key: String): ReviewDraft = state.value[key] ?: ReviewDraft()

    fun update(key: String, transform: (ReviewDraft) -> ReviewDraft) {
        if (!loaded.value) return
        state.update { drafts ->
            val next = transform(drafts[key] ?: ReviewDraft())
            if (next.isEmpty) drafts - key else drafts + (key to next)
        }
        scope.launch { flush() }
    }

    /** Writes the current drafts; awaited before a screen that saved one closes. */
    suspend fun flush() {
        writes.withLock {
            val snapshot = state.value
            withContext(Dispatchers.IO) {
                runCatching {
                    file.parentFile?.mkdirs()
                    val temporary = File(file.parentFile, "${file.name}.tmp")
                    temporary.writeText(json.encodeToString(Document.serializer(), Document(VERSION, snapshot)))
                    if (!temporary.renameTo(file)) {
                        file.writeText(temporary.readText())
                        temporary.delete()
                    }
                }
            }
        }
    }

    @Serializable
    private data class Document(val version: Int, val drafts: Map<String, ReviewDraft> = emptyMap())

    private companion object {
        const val VERSION = 1
    }
}
