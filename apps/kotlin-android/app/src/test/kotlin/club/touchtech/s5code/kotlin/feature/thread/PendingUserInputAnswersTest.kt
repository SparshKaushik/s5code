package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.model.PendingUserInput
import club.touchtech.s5code.kotlin.model.UserInputAnswer
import club.touchtech.s5code.kotlin.model.UserInputKind
import club.touchtech.s5code.kotlin.model.UserInputQuestion
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PendingUserInputAnswersTest {
    private val request =
        PendingUserInput(
            id = "request-1",
            questions =
                listOf(
                    UserInputQuestion(
                        id = "target",
                        header = "Target",
                        prompt = "Which page?",
                        kind = UserInputKind.SingleSelect,
                        options = listOf("Listings", "Orders"),
                    ),
                    UserInputQuestion(
                        id = "checks",
                        header = "Checks",
                        prompt = "Which checks?",
                        kind = UserInputKind.MultiSelect,
                        options = listOf("Lint", "Tests", "Build"),
                    ),
                    UserInputQuestion(
                        id = "notes",
                        header = "Notes",
                        prompt = "Anything else?",
                        kind = UserInputKind.Text,
                        options = emptyList(),
                    ),
                ),
        )

    @Test
    fun `all questions resolve into one request-wide answer record`() {
        val answers =
            buildUserInputAnswers(
                request = request,
                textAnswers = mapOf("notes" to "Keep it compact"),
                selectedAnswers =
                    mapOf(
                        "target" to setOf("Listings"),
                        // Set order is intentionally unlike provider option order.
                        "checks" to linkedSetOf("Tests", "Lint"),
                    ),
            )

        assertEquals(UserInputAnswer.Text("Listings"), answers?.get("target"))
        assertEquals(
            UserInputAnswer.Choices(listOf("Lint", "Tests")),
            answers?.get("checks"),
        )
        assertEquals(UserInputAnswer.Text("Keep it compact"), answers?.get("notes"))
    }

    @Test
    fun `submission stays disabled until every question has an answer`() {
        assertNull(
            buildUserInputAnswers(
                request = request,
                textAnswers = mapOf("notes" to "Done"),
                selectedAnswers = mapOf("target" to setOf("Orders")),
            )
        )
    }

    @Test
    fun `custom answer overrides advertised options`() {
        val answers =
            buildUserInputAnswers(
                request = request.copy(questions = request.questions.take(1)),
                textAnswers = mapOf("target" to "Both"),
                selectedAnswers = mapOf("target" to setOf("Listings")),
            )

        assertEquals(UserInputAnswer.Text("Both"), answers?.get("target"))
    }
}
