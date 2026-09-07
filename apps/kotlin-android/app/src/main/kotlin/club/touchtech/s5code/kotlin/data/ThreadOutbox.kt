package club.touchtech.s5code.kotlin.data

import android.content.Context
import androidx.core.net.toUri
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadSettings
import club.touchtech.s5code.kotlin.transport.RpcFailure
import club.touchtech.s5code.kotlin.transport.RpcFailureKind
import java.io.File
import java.time.Instant
import java.util.UUID
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/** Stable command identity reused by every retry of one queued turn. */
data class TurnDeliveryMetadata(
    val commandId: String,
    val messageId: String,
    val createdAt: String,
)

/** One existing-thread message durably parked until it can be dispatched. */
data class QueuedThreadMessage(
    val environmentId: EnvironmentId,
    val threadId: ThreadId,
    val text: String,
    val attachments: List<ComposerAttachment>,
    val settings: ThreadSettings,
    val delivery: TurnDeliveryMetadata,
    val creation: StoredQueuedThreadCreation? = null,
) {
    val key: String get() = "${environmentId.value}/${threadId.value}"
}

/**
 * A restored server row proves a queued bootstrap reached the environment even if
 * Android died before the local outbox file was deleted.
 */
internal fun queuedCreationAlreadyExists(
    message: QueuedThreadMessage,
    existingThreadKeys: Set<String>,
): Boolean = message.creation != null && message.key in existingThreadKeys

/**
 * Recovery for the narrower acknowledgement gap where the shell has not projected
 * the accepted thread yet but replay receives the server's duplicate invariant.
 * Existing-thread sends never use this path: only client-generated creation ids are
 * safe to classify this way.
 */
internal fun duplicateCreationAcknowledgesDelivery(
    message: QueuedThreadMessage,
    error: Throwable,
): Boolean {
    if (message.creation == null) return false
    val failure = error as? RpcFailure ?: return false
    if (failure.kind != RpcFailureKind.Fail) return false
    val detail = failure.message.lowercase()
    return "thread '${message.threadId.value.lowercase()}' already exists" in detail &&
        "cannot be created twice" in detail
}

internal fun isPendingCreationMissingThread(
    error: Throwable,
    threadId: ThreadId,
): Boolean {
    val failure = error as? RpcFailure ?: return false
    return failure.kind == RpcFailureKind.Fail &&
        failure.tag == "OrchestrationGetSnapshotError" &&
        failure.message.contains("Thread ${threadId.value} was not found", ignoreCase = true)
}

/**
 * One JSON file and one attachment directory per queued message.
 *
 * Composer intake initially copies images into cache, which Android may evict.
 * Enqueue promotes those bytes into app-private files before publishing the queue,
 * making the message and its images survive process death and long reconnects.
 */
class ThreadOutboxStore(private val context: Context) {
    private val root = File(context.filesDir, THREAD_OUTBOX_DIRECTORY)
    private val mutex = Mutex()
    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }

    suspend fun load(): List<QueuedThreadMessage> =
        mutex.withLock {
            withContext(Dispatchers.IO) {
                val messages = File(root, MESSAGES_DIRECTORY)
                if (!messages.exists()) return@withContext emptyList()
                messages.listFiles()
                    .orEmpty()
                    .asSequence()
                    .filter { it.isFile && it.extension == "json" }
                    .mapNotNull { file ->
                        runCatching { decodeQueuedThreadMessage(file.readText()) }.getOrElse {
                            file.delete()
                            null
                        }
                    }
                    .distinctBy { it.delivery.messageId }
                    .sortedBy { it.delivery.createdAt }
                    .toList()
            }
        }

    /** Writes bytes and JSON atomically enough that load never sees a partial record. */
    suspend fun enqueue(message: QueuedThreadMessage): QueuedThreadMessage =
        mutex.withLock {
            withContext(Dispatchers.IO) {
                val durable = message.copy(attachments = promoteAttachments(message))
                val messages = File(root, MESSAGES_DIRECTORY).apply { mkdirs() }
                val target = File(messages, "${message.delivery.messageId}.json")
                val temporary = File(messages, ".${message.delivery.messageId}.${UUID.randomUUID()}.tmp")
                temporary.writeText(encodeQueuedThreadMessage(durable))
                if (!temporary.renameTo(target)) {
                    temporary.copyTo(target, overwrite = true)
                    temporary.delete()
                }
                durable
            }
        }

    suspend fun remove(message: QueuedThreadMessage) {
        mutex.withLock {
            withContext(Dispatchers.IO) {
                File(root, "$MESSAGES_DIRECTORY/${message.delivery.messageId}.json").delete()
                File(root, "$ATTACHMENTS_DIRECTORY/${message.delivery.messageId}").deleteRecursively()
            }
        }
    }

    suspend fun clear(environmentId: EnvironmentId) {
        load().filter { it.environmentId == environmentId }.forEach { remove(it) }
    }

    private fun promoteAttachments(message: QueuedThreadMessage): List<ComposerAttachment> {
        if (message.attachments.isEmpty()) return emptyList()
        val directory =
            File(root, "$ATTACHMENTS_DIRECTORY/${message.delivery.messageId}").apply { mkdirs() }
        return message.attachments.map { attachment ->
            val extension =
                when (attachment.mimeType) {
                    "image/gif" -> "gif"
                    "image/jpeg" -> "jpg"
                    "image/webp" -> "webp"
                    else -> "png"
                }
            val target = File(directory, "${attachment.id}.$extension")
            if (!target.exists()) {
                context.contentResolver.openInputStream(attachment.uri.toUri())?.use { input ->
                    target.outputStream().use(input::copyTo)
                } ?: error("${attachment.name} is no longer readable.")
            }
            attachment.copy(uri = target.toURI().toString(), sizeBytes = target.length())
        }
    }

    companion object {
        const val THREAD_OUTBOX_DIRECTORY = "thread-outbox"
        private const val MESSAGES_DIRECTORY = "messages"
        private const val ATTACHMENTS_DIRECTORY = "attachments"
    }
}

fun newQueuedThreadMessage(
    environmentId: EnvironmentId,
    text: String,
    attachments: List<ComposerAttachment>,
    settings: ThreadSettings,
    creation: StoredQueuedThreadCreation? = null,
    threadId: ThreadId = ThreadId(UUID.randomUUID().toString()),
): QueuedThreadMessage =
    QueuedThreadMessage(
        environmentId = environmentId,
        threadId = threadId,
        text = text,
        attachments = attachments,
        settings = settings,
        creation = creation,
        delivery =
            TurnDeliveryMetadata(
                commandId = UUID.randomUUID().toString(),
                messageId = UUID.randomUUID().toString(),
                createdAt = Instant.now().toString(),
            ),
    )

internal fun threadOutboxRetryDelayMillis(attempt: Int): Long =
    (1_000L shl (attempt - 1).coerceIn(0, 4)).coerceAtMost(16_000L)

@Serializable
data class StoredQueuedThreadCreation(
    val projectKey: String,
    val branch: String,
    val newWorktree: Boolean,
)

@Serializable
private data class StoredQueuedThreadMessage(
    val schemaVersion: Int = 1,
    val environmentId: String,
    val threadId: String,
    val commandId: String,
    val messageId: String,
    val createdAt: String,
    val text: String,
    val attachments: List<StoredAttachment>,
    val settings: StoredThreadSettings,
    val creation: StoredQueuedThreadCreation? = null,
)

private val outboxJson = Json { ignoreUnknownKeys = true; encodeDefaults = true }

internal fun encodeQueuedThreadMessage(message: QueuedThreadMessage): String =
    outboxJson.encodeToString(
        StoredQueuedThreadMessage(
            environmentId = message.environmentId.value,
            threadId = message.threadId.value,
            commandId = message.delivery.commandId,
            messageId = message.delivery.messageId,
            createdAt = message.delivery.createdAt,
            text = message.text,
            attachments = message.attachments.map { it.toStored() },
            settings = message.settings.toStoredThreadSettings(),
            creation = message.creation,
        )
    )

internal fun decodeQueuedThreadMessage(raw: String): QueuedThreadMessage {
    val stored = outboxJson.decodeFromString<StoredQueuedThreadMessage>(raw)
    require(stored.schemaVersion == 1) { "Unsupported outbox record version." }
    require(stored.environmentId.isNotBlank() && stored.threadId.isNotBlank())
    require(stored.commandId.isNotBlank() && stored.messageId.isNotBlank())
    Instant.parse(stored.createdAt)
    return QueuedThreadMessage(
        environmentId = EnvironmentId(stored.environmentId),
        threadId = ThreadId(stored.threadId),
        text = stored.text,
        attachments = stored.attachments.map { it.toRuntime() },
        settings = stored.settings.toRuntimeThreadSettings(),
        creation = stored.creation,
        delivery = TurnDeliveryMetadata(stored.commandId, stored.messageId, stored.createdAt),
    )
}
