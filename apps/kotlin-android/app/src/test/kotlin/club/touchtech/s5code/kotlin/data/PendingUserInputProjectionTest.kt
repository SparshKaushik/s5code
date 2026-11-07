package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.UserInputKind
import club.touchtech.s5code.kotlin.transport.wire.ThreadActivityDto
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test

class PendingUserInputProjectionTest {
    @Test
    fun `projection preserves every structured input question`() {
        val payload =
            Json.parseToJsonElement(
                """
                {
                  "requestId": "request-1",
                  "questions": [
                    {
                      "id": "target",
                      "header": "Target",
                      "question": "Which page?",
                      "options": [
                        {"label": "Listings", "description": "The list"},
                        {"label": "Orders", "description": "The orders page"}
                      ]
                    },
                    {
                      "id": "checks",
                      "header": "Checks",
                      "question": "Which checks?",
                      "options": [
                        {"label": "Lint", "description": "Static analysis"},
                        {"label": "Tests", "description": "Unit tests"}
                      ],
                      "multiSelect": true
                    },
                    {
                      "id": "notes",
                      "header": "Notes",
                      "question": "Anything else?",
                      "options": []
                    }
                  ]
                }
                """.trimIndent(),
            )
        val request =
            pendingUserInputOf(
                listOf(
                    ThreadActivityDto(
                        id = "activity-1",
                        kind = "user-input.requested",
                        payload = payload,
                    )
                )
            )

        assertNotNull(request)
        assertEquals("request-1", request?.id)
        assertEquals(listOf("target", "checks", "notes"), request?.questions?.map { it.id })
        assertEquals(
            listOf(UserInputKind.SingleSelect, UserInputKind.MultiSelect, UserInputKind.Text),
            request?.questions?.map { it.kind },
        )
    }

    @Test
    fun `resolved event closes the whole multi-question request`() {
        val requested =
            ThreadActivityDto(
                id = "activity-1",
                kind = "user-input.requested",
                payload =
                    Json.parseToJsonElement(
                        """{"requestId":"request-1","questions":[{"id":"one","header":"One","question":"First?","options":[]}]}"""
                    ),
            )
        val resolved =
            ThreadActivityDto(
                id = "activity-2",
                kind = "user-input.resolved",
                payload = Json.parseToJsonElement("""{"requestId":"request-1"}"""),
            )

        assertEquals(null, pendingUserInputOf(listOf(requested, resolved)))
    }
}
