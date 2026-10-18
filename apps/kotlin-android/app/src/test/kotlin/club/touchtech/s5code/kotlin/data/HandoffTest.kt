package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.HandoffEndpoint
import club.touchtech.s5code.kotlin.transport.TransportJson
import club.touchtech.s5code.kotlin.transport.wire.ServerProviderContinuationDto
import club.touchtech.s5code.kotlin.transport.wire.ServerProviderDto
import club.touchtech.s5code.kotlin.transport.wire.V2ProjectionDto
import club.touchtech.s5code.kotlin.transport.wire.V2VisibleItemDto
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class HandoffTest {

    private fun obj(text: String) = TransportJson.parseToJsonElement(text).jsonObject

    private val providers =
        listOf(
            ServerProviderDto(instanceId = "codex", driver = "codex", continuation = ServerProviderContinuationDto("openai")),
            ServerProviderDto(instanceId = "codex-work", driver = "codex", continuation = ServerProviderContinuationDto("openai")),
            ServerProviderDto(instanceId = "claudeAgent", driver = "claudeAgent", requiresNewThreadForModelChange = true),
            ServerProviderDto(instanceId = "cursor", driver = "cursor"),
        )

    private fun pending(from: HandoffEndpoint, to: HandoffEndpoint) = resolvePendingHandoff(from, to, providers)

    @Test
    fun `the source is the latest started run that was not rolled back`() {
        val item = V2VisibleItemDto(sourceThreadId = "t", sourceItemId = "i", item = JsonObject(emptyMap()))
        val projection =
            V2ProjectionDto(
                thread = JsonObject(emptyMap()),
                runs =
                    listOf(
                        obj("""{"id":"a","status":"completed","startedAt":"x","modelSelection":{"instanceId":"codex","model":"gpt"}}"""),
                        obj("""{"id":"b","status":"rolled_back","startedAt":"x","modelSelection":{"instanceId":"cursor","model":"c"}}"""),
                        obj("""{"id":"c","status":"queued","startedAt":null,"modelSelection":{"instanceId":"cursor","model":"c"}}"""),
                    ),
                visibleTurnItems = listOf(item),
            )
        assertEquals(HandoffEndpoint("codex", "gpt"), handoffSourceOf(projection))
        // Nothing sent yet: nothing to hand off.
        assertNull(handoffSourceOf(projection.copy(visibleTurnItems = emptyList())))
    }

    @Test
    fun `same instance hands off only when the provider needs a new thread for a model change`() {
        assertNull(pending(HandoffEndpoint("codex", "a"), HandoffEndpoint("codex", "b")))
        assertNull(pending(HandoffEndpoint("claudeAgent", "a"), HandoffEndpoint("claudeAgent", "a")))
        assertEquals(
            PendingHandoff(HandoffEndpoint("claudeAgent", "a"), HandoffEndpoint("claudeAgent", "b")),
            pending(HandoffEndpoint("claudeAgent", "a"), HandoffEndpoint("claudeAgent", "b")),
        )
    }

    @Test
    fun `switching instances continues only within one driver and continuation group`() {
        assertNull(pending(HandoffEndpoint("codex", "a"), HandoffEndpoint("codex-work", "a")))
        assertEquals(
            HandoffEndpoint("cursor", "c"),
            pending(HandoffEndpoint("codex", "a"), HandoffEndpoint("cursor", "c"))?.to,
        )
        // A source without a continuation group always hands off.
        assertEquals(
            HandoffEndpoint("cursor", "c"),
            pending(HandoffEndpoint("cursor", "c"), HandoffEndpoint("codex", "a"))?.from,
        )
        assertNull(resolvePendingHandoff(null, HandoffEndpoint("codex", "a"), providers))
    }
}
