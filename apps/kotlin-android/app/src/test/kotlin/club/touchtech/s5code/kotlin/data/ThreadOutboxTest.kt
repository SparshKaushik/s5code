package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ApprovalPolicy
import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ProviderOptionSelection
import club.touchtech.s5code.kotlin.model.ProviderOptionValue
import club.touchtech.s5code.kotlin.model.RuntimeMode
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadSettings
import club.touchtech.s5code.kotlin.transport.RpcFailure
import club.touchtech.s5code.kotlin.transport.RpcFailureKind
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ThreadOutboxTest {
    private val settings =
        ThreadSettings(
            provider = ProviderInstance("work-codex", "codex"),
            model = "gpt-5.4",
            runtimeMode = RuntimeMode.Plan,
            approvalPolicy = ApprovalPolicy.Full,
            options = listOf(ProviderOptionSelection("effort", ProviderOptionValue.Text("high"))),
        )

    @Test
    fun `queued message round trip preserves routing settings attachments and stable ids`() {
        val message =
            QueuedThreadMessage(
                environmentId = EnvironmentId("env-1"),
                threadId = ThreadId("thread-1"),
                text = "ship it",
                attachments =
                    listOf(
                        ComposerAttachment(
                            id = "image-1",
                            name = "screen.png",
                            mimeType = "image/png",
                            sizeBytes = 12,
                            uri = "file:///private/outbox/image-1.png",
                        )
                    ),
                settings = settings,
                delivery = TurnDeliveryMetadata("command-1", "message-1", "2026-08-28T00:00:00Z"),
            )

        assertEquals(message, decodeQueuedThreadMessage(encodeQueuedThreadMessage(message)))
    }

    @Test
    fun `pending task creation survives serialization`() {
        val message =
            newQueuedThreadMessage(
                environmentId = EnvironmentId("env-1"),
                text = "Build the thing",
                attachments = emptyList(),
                settings = settings,
                creation = StoredQueuedThreadCreation("project-1", "main", newWorktree = true),
                threadId = ThreadId("pending-thread"),
            )

        assertEquals(message, decodeQueuedThreadMessage(encodeQueuedThreadMessage(message)))
    }

    @Test
    fun `restored creation with matching projected thread is acknowledged without replay`() {
        val pending = pendingCreation()

        assertTrue(queuedCreationAlreadyExists(pending, setOf("env-1/pending-thread")))
        assertFalse(queuedCreationAlreadyExists(pending, setOf("other/pending-thread")))
        assertFalse(
            queuedCreationAlreadyExists(
                pending.copy(creation = null),
                setOf("env-1/pending-thread"),
            )
        )
    }

    @Test
    fun `matching server duplicate acknowledges only pending creation`() {
        val pending = pendingCreation()
        val duplicate =
            RpcFailure(
                kind = RpcFailureKind.Fail,
                tag = "OrchestrationCommandInvariantError",
                message = "Thread 'pending-thread' already exists and cannot be created twice.",
            )

        assertTrue(duplicateCreationAcknowledgesDelivery(pending, duplicate))
        assertFalse(duplicateCreationAcknowledgesDelivery(pending.copy(creation = null), duplicate))
    }

    @Test
    fun `unrelated failures never acknowledge pending creation`() {
        val pending = pendingCreation()

        assertFalse(
            duplicateCreationAcknowledgesDelivery(
                pending,
                RpcFailure(
                    kind = RpcFailureKind.Fail,
                    tag = "OrchestrationCommandInvariantError",
                    message = "Thread 'another-thread' already exists and cannot be created twice.",
                ),
            )
        )
        assertFalse(
            duplicateCreationAcknowledgesDelivery(
                pending,
                RpcFailure(
                    kind = RpcFailureKind.Die,
                    tag = null,
                    message = "Thread 'pending-thread' already exists and cannot be created twice.",
                ),
            )
        )
        assertFalse(
            duplicateCreationAcknowledgesDelivery(
                pending,
                IllegalStateException(
                    "Thread 'pending-thread' already exists and cannot be created twice."
                ),
            )
        )
    }

    @Test
    fun `pending creation recognizes only matching missing-thread stream failure`() {
        assertTrue(
            isPendingCreationMissingThread(
                RpcFailure(
                    kind = RpcFailureKind.Fail,
                    tag = "OrchestrationGetSnapshotError",
                    message = "Thread pending-thread was not found",
                ),
                ThreadId("pending-thread"),
            )
        )
        assertFalse(
            isPendingCreationMissingThread(
                RpcFailure(
                    kind = RpcFailureKind.Fail,
                    tag = "OrchestrationGetSnapshotError",
                    message = "Thread another-thread was not found",
                ),
                ThreadId("pending-thread"),
            )
        )
        assertFalse(
            isPendingCreationMissingThread(
                RpcFailure(
                    kind = RpcFailureKind.Die,
                    tag = "OrchestrationGetSnapshotError",
                    message = "Thread pending-thread was not found",
                ),
                ThreadId("pending-thread"),
            )
        )
    }

    @Test
    fun `retry backoff doubles and caps at sixteen seconds`() {
        assertEquals(
            listOf(1_000L, 2_000L, 4_000L, 8_000L, 16_000L, 16_000L),
            (1..6).map(::threadOutboxRetryDelayMillis),
        )
    }

    @Test
    fun `new queued messages receive independent stable command identities`() {
        val first =
            newQueuedThreadMessage(
                environmentId = EnvironmentId("env"),
                text = "one",
                attachments = emptyList(),
                settings = settings,
                threadId = ThreadId("thread"),
            )
        val second =
            newQueuedThreadMessage(
                environmentId = EnvironmentId("env"),
                text = "two",
                attachments = emptyList(),
                settings = settings,
                threadId = ThreadId("thread"),
            )

        assertNotEquals(first.delivery.commandId, second.delivery.commandId)
        assertNotEquals(first.delivery.messageId, second.delivery.messageId)
    }

    private fun pendingCreation(): QueuedThreadMessage =
        newQueuedThreadMessage(
            environmentId = EnvironmentId("env-1"),
            text = "Build the thing",
            attachments = emptyList(),
            settings = settings,
            creation = StoredQueuedThreadCreation("project-1", "main", newWorktree = true),
            threadId = ThreadId("pending-thread"),
        )
}
