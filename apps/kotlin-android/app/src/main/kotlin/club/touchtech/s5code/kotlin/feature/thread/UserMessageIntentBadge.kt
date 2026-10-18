package club.touchtech.s5code.kotlin.feature.thread

enum class UserMessageIntentTone { Queued, Steer }

/** Footer pill on a sent user message recording how it entered the turn. */
data class UserMessageIntentBadge(
    val label: String,
    val accessibilityLabel: String,
    val explanation: String,
    val tone: UserMessageIntentTone,
)

/**
 * Maps a user message's `inputIntent` to its badge. A plain turn start has no
 * badge. Mirrors mobile's `userMessageIntentBadge`.
 */
fun userMessageIntentBadge(inputIntent: String?): UserMessageIntentBadge? =
    when (inputIntent) {
        "queued_turn" ->
            UserMessageIntentBadge(
                label = "from queue",
                accessibilityLabel = "Queued while the agent was busy",
                explanation =
                    "This badge records that the message was queued. It stays visible after the message leaves the queue.",
                tone = UserMessageIntentTone.Queued,
            )
        "steer" ->
            UserMessageIntentBadge(
                label = "steer",
                accessibilityLabel = "Steered the active turn",
                explanation =
                    "This message was intended to steer the agent while its turn was still running.",
                tone = UserMessageIntentTone.Steer,
            )
        "promoted_queued_to_steer" ->
            UserMessageIntentBadge(
                label = "queued → steer",
                accessibilityLabel = "Originally queued, then promoted to steer the active turn",
                explanation =
                    "This message was originally queued, then promoted to steer the agent while its turn was still running.",
                tone = UserMessageIntentTone.Steer,
            )
        else -> null
    }
