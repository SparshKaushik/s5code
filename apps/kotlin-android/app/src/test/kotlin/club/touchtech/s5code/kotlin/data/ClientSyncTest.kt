package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ModelFavorite
import club.touchtech.s5code.kotlin.transport.*
import club.touchtech.s5code.kotlin.transport.wire.*
import kotlinx.serialization.json.*
import org.junit.Assert.*
import org.junit.Test

class ClientSyncTest {
    private fun json(text: String) = TransportJson.parseToJsonElement(text) as JsonObject

    @Test fun `an empty server favorite list wins over the device's old favorites`() {
        val legacy = listOf(ModelFavorite("codex", "sol"))
        assertEquals(legacy, ServerConfigSettingsDto().modelFavorites(legacy))
        assertEquals(emptyList<ModelFavorite>(), ServerConfigSettingsDto(favorites = emptyList()).modelFavorites(legacy))
        assertEquals(listOf(ModelFavorite("work", "luna")),
            ServerConfigSettingsDto(favorites = listOf(ModelFavoriteDto("work", "luna"))).modelFavorites(legacy))
    }

    @Test fun `model order tolerates stale IDs and keeps unlisted models in catalog order`() {
        assertEquals(listOf("b", "a", "c"), orderedProviderModels(listOf("a", "b", "c"),
            ProviderModelPreferencesDto(modelOrder = listOf("missing", "b"))))
    }

    @Test fun `compact snapshots restore local controls without copying inherited history`() {
        val local = json("""{"id":"local","threadId":"t","ordinal":3}""")
        val inherited = json("""{"id":"parent","threadId":"p","ordinal":1}""")
        val control = json("""{"id":"control"}""")
        val projection = V2ProjectionDto(json("""{"id":"t"}"""), turnItems = listOf(control),
            visibleTurnItems = listOf(V2VisibleItemDto(sourceThreadId = "p", sourceItemId = "parent", item = inherited, visibility = "inherited"),
                V2VisibleItemDto(sourceThreadId = "t", sourceItemId = "local", item = local)))
        assertEquals(listOf(local, control), projection.restoreCompactTurnItems(true).turnItems)
        assertSame(projection, projection.restoreCompactTurnItems(false))
    }

    @Test fun `a queued run overtaken by a steer still appears when it starts`() {
        val projection = V2ProjectionDto(json("""{"id":"t"}"""),
            runs = listOf(json("""{"id":"q","status":"queued","ordinal":2}""")))
        val item = json("""{"id":"item","threadId":"t","runId":"q","ordinal":2,"type":"assistant_message","text":"Queued answer"}""")
        val event = json("""{"threadId":"t","type":"turn-item.updated","payload":$item}""")
        assertEquals("item", applyV2Event(projection, event, true, 5).visibleTurnItems.single().sourceItemId)
        val finished = projection.copy(runs = listOf(json("""{"id":"q","status":"completed","ordinal":2}""")))
        assertTrue(applyV2Event(finished, event, true, 5).visibleTurnItems.isEmpty())
    }

    @Test fun `an app reference must name the tool that produced it`() {
        val item = json("""{"toolName":"bank.balance","output":{"t3McpApp":{"server":"bank","tool":"balance","resourceUri":"ui://balance","attachmentId":"asset"}}}""")
        assertEquals("asset", mcpAppReference(item)?.v2String("attachmentId"))
        assertNull(mcpAppReference(JsonObject(item + ("toolName" to JsonPrimitive("other.tool")))))
        val nested = JsonObject(item + ("output" to json("""{"structuredContent":${item["output"]}}""")))
        assertEquals("asset", mcpAppReference(nested)?.v2String("attachmentId"))
    }

    @Test fun `restart prompts and automation retain their sender attribution`() {
        assertEquals("S5 Code", userMessagePresentation(json("""{"createdBy":"agent","creationSource":"server","text":"Continue"}""")).attribution)
        assertNull(userMessagePresentation(json("""{"createdBy":"user","text":"Continue"}""")).attribution)
        val scheduled = userMessagePresentation(json("""{"createdBy":"agent","text":"[Triggered by schedule task: nightly]\n\nCheck builds"}"""))
        assertEquals("Automation", scheduled.attribution)
        assertEquals("Check builds", scheduled.text)
    }

    @Test fun `one email in different workspaces remains two quota accounts`() {
        assertNotEquals(accountKey("codex", "a@b.com", "one"), accountKey("codex", "a@b.com", "two"))
        assertEquals(accountKey("codex", "A@B.COM"), accountKey("codex", "a@b.com", ""))
    }

    @Test fun `closing one browser tab leaves the others available`() {
        val tabs = listOf(json("""{"tabId":"a"}"""), json("""{"tabId":"b"}"""))
        assertEquals("b", reduceBrowserTabs(tabs, json("""{"type":"closed","tabId":"a"}""")).single().v2String("tabId"))
    }

    @Test fun `descendant input stays reachable and lineage cycles terminate`() {
        val child = ThreadShellDto("child", title = "Reviewer", lineage = json("""{"parentThreadId":"parent"}"""))
        val grandchild = ThreadShellDto("grandchild", title = "Question", lineage = json("""{"parentThreadId":"child"}"""),
            pendingRuntimeRequest = json("""{"kind":"user_input"}"""))
        assertEquals("grandchild", childInputs(listOf(child, grandchild), "parent").single().threadId)
        val cycle = child.copy(lineage = json("""{"parentThreadId":"grandchild"}"""))
        assertTrue(childInputs(listOf(cycle, grandchild), "parent").isEmpty())
    }

    @Test fun `active waiting runs read as working and quota failures read as limited`() {
        val waiting = ThreadShellDto("t", status = "waiting", pendingBackgroundTasks = listOf(json("""{"kind":"subagent"}""")))
        assertEquals(club.touchtech.s5code.kotlin.model.ThreadStatus.Working, threadStatusOf(waiting.normalizedV2(), 0))
        val limited = ThreadShellDto("t", status = "failed", lastErrorClass = "usage_limit")
        assertEquals(club.touchtech.s5code.kotlin.model.ThreadStatus.Limited, threadStatusOf(limited.normalizedV2(), 0))
    }
}
