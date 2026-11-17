package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.QueuedRun
import club.touchtech.s5code.kotlin.transport.TransportJson
import club.touchtech.s5code.kotlin.transport.wire.V2ProjectionDto
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class QueuedRunPresentationTest {
    private fun obj(text: String) = TransportJson.parseToJsonElement(text).jsonObject
    private val metadata = obj("""{"id":"thread","projectId":"project","title":"Queue"}""")
    private fun queued(vararg messages: String) = V2ProjectionDto(
        metadata,
        runs = messages.mapIndexed { index, _ ->
            obj("""{"id":"run-$index","ordinal":${index + 1},"status":"queued","userMessageId":"message-$index"}""")
        },
        messages = messages.mapIndexed { index, json -> obj("""{"id":"message-$index",$json}""") },
    )

    @Test
    fun `queued messages carry who sent them`() {
        val runs = v2Presentation(queued(
            """"text":"mine","createdBy":"user"""",
            """"text":"nightly","createdBy":"agent","scheduledTaskId":"task"""",
            """"text":"from a helper","createdBy":"agent","creationSource":"agent","senderThreadId":"helper"""",
            """"text":"compact","createdBy":"agent","creationSource":"server"""",
            """"text":"hi","context":{"version":1,"guest":{"guestMessageId":"g","name":"Ada","text":"hi"},"records":[]}""",
        )).queuedRuns
        assertEquals(listOf(null, "Automation", "Agent", "S5 Code", null), runs.map { it.attribution })
        assertEquals("task", runs[1].scheduledTaskId)
        assertEquals("helper", runs[2].senderThreadId)
        assertEquals("Ada", runs[4].guestName)
    }

    @Test
    fun `the sender line names the task, thread, or guest and falls back to a kind`() {
        val run = QueuedRun("run", "message", "text", held = false)
        assertNull(queuedRunSenderLabel(run, null, null))
        assertEquals("Nightly triage", queuedRunSenderLabel(run.copy(attribution = "Automation"), "Nightly triage", null))
        assertEquals("Automation", queuedRunSenderLabel(run.copy(attribution = "Automation"), null, null))
        assertEquals("Fix the build", queuedRunSenderLabel(run.copy(attribution = "Agent"), null, " Fix the build "))
        assertEquals("Agent", queuedRunSenderLabel(run.copy(attribution = "Agent"), null, " "))
        assertEquals("Sent by S5 Code", queuedRunSenderLabel(run.copy(attribution = "S5 Code"), null, null))
        assertEquals("Ada", queuedRunSenderLabel(run.copy(guestName = "Ada"), null, null))
    }

    @Test
    fun `a running compact can be skipped even when the provider cannot steer`() {
        val projection = V2ProjectionDto(
            metadata,
            runs = listOf(
                obj("""{"id":"compact","ordinal":1,"status":"running","userMessageId":"message-compact"}"""),
                obj("""{"id":"queued","ordinal":2,"status":"queued","userMessageId":"message-queued"}"""),
            ),
            messages = listOf(
                obj("""{"id":"message-compact","role":"user","text":"/compact","attachments":[]}"""),
                obj("""{"id":"message-queued","role":"user","text":"Next","attachments":[]}"""),
            ),
        )
        val presented = v2Presentation(projection)
        assertFalse(presented.canSteer)
        assertTrue(presented.canPromoteQueued)
        assertFalse(presented.canSendNow)
        val prompt = projection.copy(messages = listOf(
            obj("""{"id":"message-compact","role":"user","text":"Do the thing","attachments":[]}"""),
        ))
        assertFalse(v2Presentation(prompt).canPromoteQueued)
    }
}
