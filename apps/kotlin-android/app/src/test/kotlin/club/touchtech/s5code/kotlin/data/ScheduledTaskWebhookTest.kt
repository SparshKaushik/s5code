package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.TransportJson
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ScheduledTaskWebhookTest {

    private fun json(text: String) = TransportJson.parseToJsonElement(text)

    private fun row(path: String, op: String, value: String = "") = emptyWebhookFilterDraft().copy(path = path, op = op, value = value)

    @Test
    fun `stored filters round trip through the form rows`() {
        val stored =
            json(
                """[{"path":"headers.x-github-event","op":"equals","value":"push"},
                   {"path":"body.action","op":"in","value":["opened","reopened"]},
                   {"path":"query.source","op":"exists"}]"""
            ).jsonArray
        val drafts = webhookFilterDrafts(stored)
        assertEquals(listOf("push", "opened, reopened", ""), drafts.map { it.value })
        assertEquals(WebhookFilterParse.Filters(stored), parseWebhookFilterDrafts(drafts))
    }

    @Test
    fun `a row the server would reject names itself and what it needs`() {
        assertEquals(
            WebhookFilterParse.Invalid("Filter 2 needs a path under body, headers or query, and at least one value."),
            parseWebhookFilterDrafts(listOf(row("body.ok", "exists"), row("body.action", "in", " , "))),
        )
        assertEquals(
            WebhookFilterParse.Invalid("Filter 1 needs a path under body, headers or query."),
            parseWebhookFilterDrafts(listOf(row("payload.action", "exists"))),
        )
        assertTrue(parseWebhookFilterDrafts(List(MAX_WEBHOOK_FILTERS + 1) { row("body", "exists") }) is WebhookFilterParse.Invalid)
    }

    @Test
    fun `filters count as changed only when a sent field changed`() {
        val saved = webhookFilterDrafts(json("""[{"path":"body.a","op":"exists"}]""") as JsonArray)
        // An exists row's value is never sent, so typing into it is no change.
        assertFalse(webhookFilterDraftsChanged(saved, saved.map { it.copy(value = "x") }))
        assertTrue(webhookFilterDraftsChanged(saved, saved.map { it.copy(op = "equals") }))
        assertTrue(webhookFilterDraftsChanged(saved, emptyList()))
    }

    @Test
    fun `dedupe templates need a request placeholder and no stray braces`() {
        assertTrue(isWebhookDedupeTemplate("{{headers.x-github-delivery}}"))
        assertTrue(isWebhookDedupeTemplate("pr-{{ body.pull_request.number }}-{{request}}"))
        assertFalse(isWebhookDedupeTemplate("static"))
        assertFalse(isWebhookDedupeTemplate("{{secrets.token}}"))
        assertFalse(isWebhookDedupeTemplate("{{body.id}} {{"))
    }

    @Test
    fun `dedupe fields validate and blank fields clear what is stored`() {
        assertNull(webhookDedupeError(WebhookDedupeDraft(replayKey = "{{body.id}}", coalesceWindowSeconds = "120")))
        assertEquals(
            "Coalesce key must contain a valid request placeholder.",
            webhookDedupeError(WebhookDedupeDraft(coalesceKey = "id")),
        )
        assertEquals(
            "Coalesce window must be whole seconds from 10 to 3600.",
            webhookDedupeError(WebhookDedupeDraft(coalesceWindowSeconds = "5")),
        )
        assertEquals(json("{}"), webhookDedupeJson(WebhookDedupeDraft()))
        val draft = webhookDedupeDraft(json("""{"replayKey":"{{body.id}}","coalesceWindowSeconds":60}""").jsonObject)
        assertEquals(json("""{"replayKey":"{{body.id}}","coalesceWindowSeconds":60}"""), webhookDedupeJson(draft))
    }

    @Test
    fun `delivery labels name new outcomes and keep unknown ones readable`() {
        assertEquals("Opened window", webhookDeliveryLabel("accepted", pendingWindow = true))
        assertEquals("Filtered", webhookDeliveryLabel("filtered", pendingWindow = false))
        assertEquals("some new outcome", webhookDeliveryLabel("some_new_outcome", pendingWindow = false))
    }
}
