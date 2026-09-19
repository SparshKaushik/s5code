package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.model.PendingUserInput
import club.touchtech.s5code.kotlin.model.QuestionAttachment
import club.touchtech.s5code.kotlin.model.QuestionAttachmentKind
import club.touchtech.s5code.kotlin.model.QuestionAttachmentStatus
import club.touchtech.s5code.kotlin.model.SentAttachment
import club.touchtech.s5code.kotlin.model.UserInputAnswer
import club.touchtech.s5code.kotlin.model.UserInputKind
import club.touchtech.s5code.kotlin.model.UserInputOption
import club.touchtech.s5code.kotlin.model.UserInputQuestion
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
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
                        options =
                            listOf(
                                UserInputOption(label = "Listings"),
                                UserInputOption(label = "Orders"),
                            ),
                    ),
                    UserInputQuestion(
                        id = "checks",
                        header = "Checks",
                        prompt = "Which checks?",
                        kind = UserInputKind.MultiSelect,
                        options =
                            listOf(
                                UserInputOption(label = "Lint"),
                                UserInputOption(label = "Tests"),
                                UserInputOption(label = "Build"),
                            ),
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

    @Test
    fun `an explicit option value is submitted instead of the label`() {
        val valued =
            PendingUserInput(
                id = "request-2",
                questions =
                    listOf(
                        UserInputQuestion(
                            id = "env",
                            header = "Environment",
                            prompt = "Deploy where?",
                            kind = UserInputKind.SingleSelect,
                            options =
                                listOf(
                                    UserInputOption(
                                        label = "Staging",
                                        value = "staging",
                                        description = "Safe first",
                                    ),
                                    UserInputOption(label = "Production", value = "prod"),
                                ),
                        ),
                    ),
            )

        val answers =
            buildUserInputAnswers(
                request = valued,
                textAnswers = emptyMap(),
                selectedAnswers = mapOf("env" to setOf("staging")),
            )

        assertEquals(UserInputAnswer.Text("staging"), answers?.get("env"))
    }

    @Test
    fun `custom answers are ignored when the provider disallows them`() {
        val locked =
            PendingUserInput(
                id = "request-3",
                questions =
                    listOf(
                        UserInputQuestion(
                            id = "pick",
                            header = "Pick",
                            prompt = "Choose one",
                            kind = UserInputKind.SingleSelect,
                            options = listOf(UserInputOption(label = "Only")),
                            allowCustomAnswer = false,
                        ),
                    ),
            )

        assertNull(
            buildUserInputAnswers(
                request = locked,
                textAnswers = mapOf("pick" to "Something else"),
                selectedAnswers = emptyMap(),
            )
        )
    }

    private fun stagedFile(
        localId: String,
        status: QuestionAttachmentStatus = QuestionAttachmentStatus.Ready,
        uploadId: String? = "upload-$localId",
    ) = QuestionAttachment(
        localId = localId,
        name = "$localId.png",
        mimeType = "image/png",
        sizeBytes = 128,
        kind = QuestionAttachmentKind.Image,
        localPath = "/cache/$localId",
        status = status,
        uploadedAttachmentId = uploadId,
    )

    @Test
    fun `ready attachments satisfy an unanswered question and join the payload`() {
        // An attachment-only answer is legal: RN resolves it to `""`.
        val submission =
            buildUserInputSubmission(
                request = request.copy(questions = request.questions.take(1)),
                textAnswers = emptyMap(),
                selectedAnswers = emptyMap(),
                attachments = mapOf("target" to listOf(stagedFile("a"))),
            )

        assertEquals(UserInputAnswer.Text(""), submission?.answers?.get("target"))
        assertEquals(
            listOf(
                SentAttachment(
                    id = "upload-a",
                    name = "a.png",
                    mimeType = "image/png",
                    sizeBytes = 128,
                    type = "image",
                )
            ),
            submission?.attachmentsByQuestionId?.get("target"),
        )
    }

    @Test
    fun `an uploading or failed attachment blocks the whole submission`() {
        listOf(QuestionAttachmentStatus.Uploading, QuestionAttachmentStatus.Failed).forEach { status ->
            assertNull(
                buildUserInputSubmission(
                    request = request.copy(questions = request.questions.take(1)),
                    textAnswers = emptyMap(),
                    selectedAnswers = emptyMap(),
                    attachments = mapOf("target" to listOf(stagedFile("a", status))),
                )
            )
        }
        // A question whose picker is still materializing files blocks too.
        assertNull(
            buildUserInputSubmission(
                request = request.copy(questions = request.questions.take(1)),
                textAnswers = emptyMap(),
                selectedAnswers = emptyMap(),
                attachments = emptyMap(),
                preparingQuestions = setOf("target"),
            )
        )
    }

    @Test
    fun `attachments do not satisfy a question that disallows custom answers`() {
        val locked =
            PendingUserInput(
                id = "request-4",
                questions =
                    listOf(
                        UserInputQuestion(
                            id = "pick",
                            header = "Pick",
                            prompt = "Choose one",
                            kind = UserInputKind.SingleSelect,
                            options = listOf(UserInputOption(label = "Only")),
                            allowCustomAnswer = false,
                        ),
                    ),
            )

        assertNull(
            buildUserInputSubmission(
                request = locked,
                textAnswers = emptyMap(),
                selectedAnswers = emptyMap(),
                attachments = mapOf("pick" to listOf(stagedFile("a"))),
            )
        )
    }

    @Test
    fun `a typed answer wins and its question still ships its attachments`() {
        val submission =
            buildUserInputSubmission(
                request = request.copy(questions = request.questions.take(2)),
                textAnswers = emptyMap(),
                selectedAnswers =
                    mapOf(
                        "target" to setOf("Orders"),
                        "checks" to setOf("Lint"),
                    ),
                attachments = mapOf("target" to listOf(stagedFile("a"))),
            )

        // The selected option stays the answer; the files ride alongside it.
        assertEquals(UserInputAnswer.Text("Orders"), submission?.answers?.get("target"))
        assertEquals(UserInputAnswer.Choices(listOf("Lint")), submission?.answers?.get("checks"))
        assertTrue(
            submission?.attachmentsByQuestionId?.keys == setOf("target")
        )
    }
}
