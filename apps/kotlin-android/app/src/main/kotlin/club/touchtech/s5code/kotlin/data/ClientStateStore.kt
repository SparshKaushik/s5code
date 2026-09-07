package club.touchtech.s5code.kotlin.data

import android.content.Context
import androidx.core.content.edit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/** Persisted preferences. Non-secret, so plain storage rather than the Keystore. */
@Serializable
data class StoredPreferences(
    val themeMode: String = "System",
    val dynamicColor: Boolean = true,
    val projectGrouping: String = "ByProject",
    val threadSort: String = "Recent",
    val snoozedThreadsExpanded: Boolean = false,
    val settledThreadsExpanded: Boolean = false,
    val textScale: Float = 1f,
    val codeScale: Float = 1f,
    val wrapCode: Boolean = false,
    val terminalScale: Float = 1f,
    val terminalTheme: String = "App",
    val autoSettleOnMerge: Boolean = true,
    val notifyApprovals: Boolean = true,
    val notifyInput: Boolean = true,
    val notifyCompletion: Boolean = true,
    val notifyFailures: Boolean = true,
    val liveUpdatesEnabled: Boolean = true,
)

/** One persisted composer draft: text plus the attachments already copied to cache. */
@Serializable
data class StoredDraft(
    val text: String = "",
    val attachments: List<StoredAttachment> = emptyList(),
    val settings: StoredThreadSettings? = null,
)

/** Settings staged for the next turn of an existing thread. */
@Serializable
data class StoredThreadSettings(
    val provider: String = "codex",
    val providerDriver: String = "",
    val model: String = "",
    val runtimeMode: String = "Default",
    val approvalPolicy: String = "Ask",
    val options: Map<String, StoredOptionValue> = emptyMap(),
)

@Serializable
data class StoredAttachment(
    val id: String,
    val name: String,
    val mimeType: String,
    val sizeBytes: Long,
    val uri: String,
)

/** The new-task draft, which carries its own context alongside the prompt. */
@Serializable
data class StoredNewTaskDraft(
    val environmentId: String = "",
    val projectKey: String = "",
    val prompt: String = "",
    val attachments: List<StoredAttachment> = emptyList(),
    val branch: String = "",
    val workspaceMode: String = "CurrentCheckout",
    /**
     * The provider instance id, which is what a turn start routes on. Older files
     * hold a driver enum name ("Codex"); `storedProvider` reads both.
     */
    val provider: String = "codex",
    /** The instance's driver slug, for the label and glyph. Absent in older files. */
    val providerDriver: String = "",
    val model: String = "",
    val runtimeMode: String = "Default",
    val approvalPolicy: String = "Ask",
    /**
     * Provider option values, keyed by descriptor id. A string or a boolean, which
     * is what the contract's `ProviderOptionSelectionValue` allows.
     *
     * Stored as a map because the ids are provider-defined and the whole point of
     * item 7 is that the client does not know them: an older file that still holds
     * `reasoningEffort` decodes to a value the current model may not advertise, and
     * `providerOptionDescriptors` drops it on the way to the UI.
     */
    val options: Map<String, StoredOptionValue> = emptyMap(),
)

/** One persisted provider option value. Exactly one field is set. */
@Serializable
data class StoredOptionValue(val text: String? = null, val flag: Boolean? = null)

@Serializable
private data class StoredState(
    val preferences: StoredPreferences = StoredPreferences(),
    val threadDrafts: Map<String, StoredDraft> = emptyMap(),
    val newTask: StoredNewTaskDraft = StoredNewTaskDraft(),
    val recentThreads: List<StoredRecentThread> = emptyList(),
)

/** A thread the user opened, for launcher shortcuts. */
@Serializable
data class StoredRecentThread(val environmentId: String, val threadId: String, val title: String)

/**
 * Non-secret client state that has to survive process death: preferences, drafts,
 * and the recent threads launcher shortcuts are built from.
 *
 * A draft is the reason this exists. A user types three paragraphs, switches to
 * another app to copy a stack trace, and Android kills the process; without this
 * the prompt is gone. Attachments survive too, because intake already copied each
 * image into app cache, so the stored URI still resolves. Existing-thread keys are
 * environment-scoped, and a settings-only draft is retained because model/runtime
 * choices are staged until the next send.
 *
 * Writes are debounced by the caller (see `AppStore`), not here: this class stays
 * a dumb serializer so it can be exercised without a Looper. `apply()` rather than
 * `commit()` is right for this file — unlike a spent pairing token, a lost
 * keystroke is recoverable and a synchronous disk write per character is not.
 */
class ClientStateStore(context: Context) {

    private val preferences = context.getSharedPreferences(NAME, Context.MODE_PRIVATE)
    private val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = true
    }

    suspend fun load(): Loaded =
        withContext(Dispatchers.IO) {
            val raw = preferences.getString(KEY, null) ?: return@withContext Loaded()
            val stored =
                runCatching { json.decodeFromString<StoredState>(raw) }
                    // A file this build cannot read is treated as empty: losing a
                    // draft is annoying, a launch loop is not.
                    .getOrDefault(StoredState())
            Loaded(
                preferences = stored.preferences,
                threadDrafts = stored.threadDrafts,
                newTask = stored.newTask,
                recentThreads = stored.recentThreads,
            )
        }

    suspend fun save(
        preferencesValue: StoredPreferences,
        threadDrafts: Map<String, StoredDraft>,
        newTask: StoredNewTaskDraft,
        recentThreads: List<StoredRecentThread>,
    ) {
        val encoded =
            json.encodeToString(
                StoredState(
                    preferences = preferencesValue,
                    // Empty drafts are dropped rather than stored: a map that grows
                    // one entry per thread ever opened is a slow leak on disk.
                    threadDrafts =
                        threadDrafts.filterValues {
                            it.text.isNotBlank() ||
                                it.attachments.isNotEmpty() ||
                                it.settings != null
                        },
                    newTask = newTask,
                    recentThreads = recentThreads.take(MAX_RECENT_THREADS),
                )
            )
        withContext(Dispatchers.IO) { preferences.edit { putString(KEY, encoded) } }
    }

    data class Loaded(
        val preferences: StoredPreferences = StoredPreferences(),
        val threadDrafts: Map<String, StoredDraft> = emptyMap(),
        val newTask: StoredNewTaskDraft = StoredNewTaskDraft(),
        val recentThreads: List<StoredRecentThread> = emptyList(),
    )

    companion object {
        /** Launchers show about four shortcuts; one slot is the static New task. */
        const val MAX_RECENT_THREADS = 3

        private const val NAME = "s5code.client-state"
        private const val KEY = "state"
    }
}
