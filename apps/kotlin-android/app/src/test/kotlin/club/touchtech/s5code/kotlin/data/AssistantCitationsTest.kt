package club.touchtech.s5code.kotlin.data

import java.net.URLEncoder
import org.junit.Assert.assertEquals
import org.junit.Test

/** Ports `threadQueueControlPresentation.test.ts`'s quote case and the converter's guards. */
class AssistantCitationsTest {

    /** `formatAssistantCitationHref`: `URLSearchParams` serializes as form encoding. */
    private fun href(
        text: String = "Retry the sync after the lock clears.",
        start: String = "0",
        end: String = "37",
        comment: String? = null,
        path: String = "environment%3Atest/thread%3Atest/message%3Aassistant",
    ): String {
        val params =
            listOfNotNull(
                "text" to text,
                "start" to start,
                "end" to end,
                "prefix" to "",
                "suffix" to "",
                comment?.let { "comment" to it },
            )
        val query = params.joinToString("&") { (key, value) -> "$key=${URLEncoder.encode(value, "UTF-8")}" }
        return "t3-citation://v1/$path?$query"
    }

    @Test
    fun `shows an assistant quote as its quoted text`() {
        assertEquals(
            "Retry the sync after the lock clears. Can we add a backoff here?",
            assistantCitationsToPlainText("[Assistant quote](${href()}) Can we add a backoff here?"),
        )
    }

    @Test
    fun `a quote with a comment keeps the comment on its own line`() {
        assertEquals(
            "Retry the sync after the lock clears.\nComment: Why here?",
            assistantCitationsToPlainText("[Assistant quote](${href(comment = "Why here?")})"),
        )
    }

    @Test
    fun `invalid citation links are left untouched`() {
        val invalid =
            listOf(
                // `end` must come after `start`.
                href(start = "5", end = "5"),
                // Three path parts: environment, thread, message.
                href(path = "environment/thread"),
                // A malformed escape is rejected the way `decodeURIComponent` throws.
                href(path = "environment%zz/thread/message"),
                // Every required key is present exactly once.
                href().replace("&suffix=", ""),
                href() + "&extra=1",
            )
        invalid.forEach { link ->
            val prompt = "[Assistant quote]($link) after"
            assertEquals(prompt, assistantCitationsToPlainText(prompt))
        }
    }

    @Test
    fun `text without citations is unchanged`() {
        val prompt = "[Docs](https://example.test) and [Assistant quote](https://example.test)"
        assertEquals(prompt, assistantCitationsToPlainText(prompt))
    }
}
