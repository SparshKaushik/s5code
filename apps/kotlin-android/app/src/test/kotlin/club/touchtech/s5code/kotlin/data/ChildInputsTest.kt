package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.TransportJson
import club.touchtech.s5code.kotlin.transport.wire.ThreadShellDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ChildInputsTest {

    private fun shell(
        id: String,
        parent: String? = null,
        request: String? = null,
        relationship: String = "subagent",
        creationSource: String? = null,
        activityRunStatus: String? = "running",
    ): ThreadShellDto {
        val lineage = parent?.let { """"lineage":{"parentThreadId":"$it","relationshipToParent":"$relationship"},""" }.orEmpty()
        val pending = request?.let { """"pendingRuntimeRequest":{"kind":"$it"},""" }.orEmpty()
        val source = creationSource?.let { """"creationSource":"$it",""" }.orEmpty()
        val run = if (activityRunStatus == null) """"activityRunStatus":null,""" else """"activityRunStatus":"$activityRunStatus","""
        return TransportJson.decodeFromString(
            ThreadShellDto.serializer(),
            """{$lineage$pending$source$run"id":"$id","title":"$id"}""",
        )
    }

    @Test
    fun `a subagent's approval stamps every ancestor but its question flags none`() {
        val threads =
            listOf(
                shell("root"),
                shell("mid", parent = "root"),
                shell("asks", parent = "mid", request = "user_input"),
                shell("approves", parent = "mid", request = "command_approval"),
            )
        assertEquals(
            mapOf("mid" to SUBAGENT_REQUEST_APPROVAL, "root" to SUBAGENT_REQUEST_APPROVAL),
            pendingSubagentRequestsByThread(threads),
        )
        assertEquals(emptyMap<String, String>(), pendingSubagentRequestsByThread(threads.dropLast(1)))
    }

    @Test
    fun `ended runs, provider-native subagents and forks ask no one`() {
        assertNull(subagentRequestOf(shell("done", parent = "root", request = "user_input", activityRunStatus = null)))
        assertNull(subagentRequestOf(shell("native", parent = "root", request = "user_input", creationSource = "provider")))
        assertNull(subagentRequestOf(shell("fork", parent = "root", request = "user_input", relationship = "fork")))
        assertNull(subagentRequestOf(shell("auth", parent = "root", request = "auth_refresh")))
    }

    @Test
    fun `a lineage cycle stops the walk`() {
        val threads =
            listOf(
                shell("a", parent = "b", request = "command_approval"),
                shell("b", parent = "a"),
            )
        assertEquals(mapOf("b" to SUBAGENT_REQUEST_APPROVAL), pendingSubagentRequestsByThread(threads))
    }

    @Test
    fun `the banner reaches nested subagents and names what they need`() {
        val threads =
            listOf(
                shell("root"),
                shell("mid", parent = "root"),
                shell("asks", parent = "mid", request = "user_input"),
                shell("approves", parent = "root", request = "command_approval"),
            )
        val children = childInputs(threads, "root")
        assertEquals(listOf("asks", "approves"), children.map { it.threadId })
        assertEquals("2 subagents need you", childInputsTitle(children))
        assertEquals("Subagent needs input", childInputsTitle(children.take(1)))
        assertEquals("Subagent needs approval", childInputsTitle(children.drop(1)))
    }
}
