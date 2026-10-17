package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.FeedEntry
import club.touchtech.s5code.kotlin.transport.RpcFailure
import club.touchtech.s5code.kotlin.transport.RpcFailureKind
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonObject

/**
 * The private-secret request flow, ported from
 * `packages/client-runtime/src/secretRequest.ts`.
 *
 * The value the user types never travels on the orchestration stream: the turn
 * item carries only what was asked and how it resolved, and `secrets.answerRequest`
 * hands the secret to the server, which keeps it under a one-use SecretRef.
 */

/** Shown under the field: the one promise the card makes about the value. */
const val SECRET_REQUEST_PRIVACY_NOTE = "Stored securely, never shown to the agent"

const val SECRET_REQUEST_DEFAULT_PLACEHOLDER = "Paste the secret"

/** What a secret-request card shows: the form while pending, otherwise a one-line outcome. */
sealed interface SecretRequestDisplay {
    data object Pending : SecretRequestDisplay

    /** Waiting, but the card belongs to another thread (a fork's inherited row). */
    data class PendingElsewhere(val label: String) : SecretRequestDisplay

    data class Answered(
        /** saved | declined | ended */
        val outcome: String,
        val label: String,
    ) : SecretRequestDisplay
}

/**
 * `secretRequestDisplay`: the card's one-line state. [answerable] is the feed
 * entry's — a pending request inherited from another thread waits for the
 * original thread, so it renders the outcome label instead of a form.
 */
fun secretRequestDisplay(entry: FeedEntry.SecretRequest): SecretRequestDisplay =
    when (entry.secretStatus) {
        "pending" ->
            if (entry.answerable) SecretRequestDisplay.Pending
            else SecretRequestDisplay.PendingElsewhere(
                "Waiting for an answer in the original thread",
            )
        "saved" -> SecretRequestDisplay.Answered("saved", "Saved securely and kept private")
        "declined" -> SecretRequestDisplay.Answered("declined", "Declined")
        else -> SecretRequestDisplay.Answered("ended", "Request ended")
    }

/** The user's reply to a pending request: a saved value or a decline. */
sealed interface SecretRequestAnswer {
    data class Save(val secret: String) : SecretRequestAnswer

    data object Decline : SecretRequestAnswer
}

/**
 * `secretRequestAnswerInput`: the `secrets.answerRequest` payload for an entry.
 * A save with a blank value returns null because the server rejects it —
 * callers keep Save disabled rather than round-tripping a refusal.
 */
fun secretRequestAnswerPayload(
    entry: FeedEntry.SecretRequest,
    answer: SecretRequestAnswer,
): JsonObject? = secretRequestAnswerPayload(entry.threadId, entry.id, answer)

/**
 * The same payload without a [FeedEntry.SecretRequest] in hand — the gateway
 * already carries the target thread and item ids.
 */
fun secretRequestAnswerPayload(
    threadId: String,
    turnItemId: String,
    answer: SecretRequestAnswer,
): JsonObject? {
    if (answer is SecretRequestAnswer.Save && answer.secret.trim().isEmpty()) return null
    return buildJsonObject {
        put("threadId", threadId)
        put("turnItemId", turnItemId)
        putJsonObject("answer") {
            when (answer) {
                is SecretRequestAnswer.Save -> {
                    put("type", "save")
                    put("secret", answer.secret.trim())
                }
                SecretRequestAnswer.Decline -> put("type", "decline")
            }
        }
    }
}

/**
 * `secretRequestFailureMessage`: only known server errors pass their message
 * through. Transport and encoding failures get the generic copy, so a typed
 * failure's payload can never surface in the UI.
 */
fun secretRequestFailureMessage(failure: Throwable): String {
    val rpc = failure as? RpcFailure
    if (
        rpc != null &&
        rpc.kind == RpcFailureKind.Fail &&
        rpc.tag in USER_FACING_SECRET_FAILURE_TAGS &&
        rpc.message.isNotBlank()
    ) {
        return rpc.message
    }
    return "Could not answer the request. Try again."
}

private val USER_FACING_SECRET_FAILURE_TAGS =
    setOf("SecretRequestError", "EnvironmentAuthorizationError")
