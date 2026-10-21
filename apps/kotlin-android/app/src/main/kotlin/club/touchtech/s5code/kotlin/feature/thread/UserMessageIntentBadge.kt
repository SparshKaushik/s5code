package club.touchtech.s5code.kotlin.feature.thread

/** Footer marker on a sent user message recording that it steered a turn. */
data class UserMessageIntentBadge(
    val label: String,
    val accessibilityLabel: String,
)

/**
 * Maps a user message's `inputIntent` to its badge. A plain turn start and a
 * queued turn have none: once a queued message is sent it is just a message.
 * Mirrors mobile's `resolveUserMessageIntentBadge`.
 */
fun userMessageIntentBadge(inputIntent: String?): UserMessageIntentBadge? =
    when (inputIntent) {
        "steer" -> UserMessageIntentBadge(label = "Steer", accessibilityLabel = "Steered the active turn")
        "promoted_queued_to_steer" ->
            UserMessageIntentBadge(
                label = "Steer",
                accessibilityLabel = "Originally queued, then promoted to steer the active turn",
            )
        else -> null
    }
