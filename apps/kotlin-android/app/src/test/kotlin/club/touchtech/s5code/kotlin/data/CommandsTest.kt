package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ApprovalPolicy
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ProviderOptionSelection
import club.touchtech.s5code.kotlin.model.ProviderOptionValue
import club.touchtech.s5code.kotlin.model.RuntimeMode
import club.touchtech.s5code.kotlin.model.ThreadSettings
import club.touchtech.s5code.kotlin.model.UserInputAnswer
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class CommandsTest {
    @Test
    fun `structured input response carries every question with its requested value shape`() {
        val command =
            Commands.respondToUserInput(
                threadId = "thread-1",
                requestId = "request-1",
                answers =
                    linkedMapOf(
                        "target" to UserInputAnswer.Text("Listings"),
                        "checks" to UserInputAnswer.Choices(listOf("Lint", "Tests")),
                    ),
            )

        val answers = command.getValue("answers").jsonObject
        assertEquals("Listings", answers.getValue("target").jsonPrimitive.content)
        assertEquals(
            listOf("Lint", "Tests"),
            answers.getValue("checks").jsonArray.map { it.jsonPrimitive.content },
        )
    }

    @Test
    fun `turn retry can reuse stable command message and creation ids`() {
        val settings = ThreadSettings()
        val command =
            Commands.startTurn(
                threadId = "thread-1",
                text = "Retry me",
                attachments = emptyList(),
                attachmentDataUrls = emptyMap(),
                settings = settings,
                commandId = "command-stable",
                messageId = "message-stable",
                createdAt = "2026-08-28T00:00:00Z",
            )

        assertEquals("command-stable", command.getValue("commandId").jsonPrimitive.content)
        assertEquals(
            "message-stable",
            command.getValue("message").jsonObject.getValue("messageId").jsonPrimitive.content,
        )
        assertEquals("2026-08-28T00:00:00Z", command.getValue("createdAt").jsonPrimitive.content)
    }

    @Test
    fun `existing thread turn carries the staged model and runtime settings`() {
        val settings =
            ThreadSettings(
                provider = ProviderInstance(instanceId = "codex-work", driver = "codex"),
                model = "gpt-5.4",
                runtimeMode = RuntimeMode.Plan,
                approvalPolicy = ApprovalPolicy.Full,
                options =
                    listOf(
                        ProviderOptionSelection(
                            "reasoningEffort",
                            ProviderOptionValue.Text("high"),
                        ),
                        ProviderOptionSelection("fastMode", ProviderOptionValue.Flag(true)),
                    ),
            )

        val command =
            Commands.startTurn(
                threadId = "thread-1",
                text = "Use the new model",
                attachments = emptyList(),
                attachmentDataUrls = emptyMap(),
                settings = settings,
            )

        val modelSelection = command.getValue("modelSelection").jsonObject
        assertEquals("codex-work", modelSelection.getValue("instanceId").jsonPrimitive.content)
        assertEquals("gpt-5.4", modelSelection.getValue("model").jsonPrimitive.content)
        assertEquals("full-access", command.getValue("runtimeMode").jsonPrimitive.content)
        assertEquals("plan", command.getValue("interactionMode").jsonPrimitive.content)
        val options = modelSelection.getValue("options").jsonArray
        assertEquals(2, options.size)
        assertTrue(options.any { it.jsonObject["id"]?.jsonPrimitive?.content == "reasoningEffort" })
    }

    @Test
    fun `snooze command carries explicit wake timestamp`() {
        val command = Commands.snooze("thread-1", "2026-09-03T18:00:00Z")
        assertEquals("thread.snooze", command.getValue("type").jsonPrimitive.content)
        assertEquals("thread-1", command.getValue("threadId").jsonPrimitive.content)
        assertEquals("2026-09-03T18:00:00Z", command.getValue("snoozedUntil").jsonPrimitive.content)
    }
}
