package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.transport.wire.SearchThreadsResultDto
import org.junit.Assert.assertEquals
import org.junit.Test

class ThreadSearchWireTest {
    @Test
    fun `message search result decodes user and assistant snippets`() {
        val result =
            TransportJson.decodeFromString(
                SearchThreadsResultDto.serializer(),
                """
                {
                  "matches": [
                    {
                      "threadId": "thread-1",
                      "projectId": "project-1",
                      "source": "assistant",
                      "snippet": "The matching body text",
                      "messageCreatedAt": "2026-08-27T12:00:00.000Z",
                      "futureField": true
                    }
                  ]
                }
                """.trimIndent(),
            )

        assertEquals(1, result.matches.size)
        assertEquals("assistant", result.matches.single().source)
        assertEquals("The matching body text", result.matches.single().snippet)
    }
}
