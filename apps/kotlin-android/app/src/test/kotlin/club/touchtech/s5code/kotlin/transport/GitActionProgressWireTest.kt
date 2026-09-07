package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.transport.wire.GitActionProgressEventDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class GitActionProgressWireTest {
    @Test
    fun `phase event decodes with forward compatible fields`() {
        val event =
            TransportJson.decodeFromString(
                GitActionProgressEventDto.serializer(),
                """
                {
                  "actionId": "action-1",
                  "cwd": "/repo",
                  "action": "commit_push_pr",
                  "kind": "phase_started",
                  "phase": "push",
                  "label": "Pushing branch",
                  "futureField": { "ignored": true }
                }
                """.trimIndent(),
            )

        assertEquals("action-1", event.actionId)
        assertEquals("commit_push_pr", event.action)
        assertEquals("phase_started", event.kind)
        assertEquals("push", event.phase)
        assertEquals("Pushing branch", event.label)
    }

    @Test
    fun `finished event decodes toast and pull request URL`() {
        val event =
            TransportJson.decodeFromString(
                GitActionProgressEventDto.serializer(),
                """
                {
                  "actionId": "action-2",
                  "cwd": "/repo",
                  "action": "commit_push_pr",
                  "kind": "action_finished",
                  "result": {
                    "action": "commit_push_pr",
                    "pr": {
                      "status": "created",
                      "url": "https://github.com/example/repo/pull/42",
                      "number": 42,
                      "title": "Ship it"
                    },
                    "toast": {
                      "title": "Committed, pushed, and opened PR",
                      "description": "PR #42 is ready",
                      "cta": {
                        "kind": "open_url",
                        "label": "Open PR",
                        "url": "https://github.com/example/repo/pull/42"
                      }
                    }
                  }
                }
                """.trimIndent(),
            )

        assertEquals("action_finished", event.kind)
        assertEquals("Committed, pushed, and opened PR", event.result?.toast?.title)
        assertEquals("https://github.com/example/repo/pull/42", event.result?.pr?.url)
        assertEquals("Open PR", event.result?.toast?.cta?.label)
    }

    @Test
    fun `create pull request result decodes the contract cta kind`() {
        val event =
            TransportJson.decodeFromString(
                GitActionProgressEventDto.serializer(),
                """
                {
                  "actionId": "action-pr",
                  "cwd": "/repo",
                  "action": "create_pr",
                  "kind": "action_finished",
                  "result": {
                    "action": "create_pr",
                    "pr": {
                      "status": "opened_existing",
                      "url": "https://github.com/example/repo/pull/7",
                      "number": 7,
                      "title": "Existing PR"
                    },
                    "toast": {
                      "title": "Opened PR #7",
                      "cta": {
                        "kind": "open_pr",
                        "label": "Open PR",
                        "url": "https://github.com/example/repo/pull/7"
                      }
                    }
                  }
                }
                """.trimIndent(),
            )

        assertEquals("create_pr", event.result?.action)
        assertEquals("open_pr", event.result?.toast?.cta?.kind)
        assertEquals("https://github.com/example/repo/pull/7", event.result?.pr?.url)
    }

    @Test
    fun `failed event is valid without a result`() {
        val event =
            TransportJson.decodeFromString(
                GitActionProgressEventDto.serializer(),
                """{"action":"push","kind":"action_failed","message":"Rejected"}""",
            )

        assertEquals("Rejected", event.message)
        assertNull(event.result)
    }
}
