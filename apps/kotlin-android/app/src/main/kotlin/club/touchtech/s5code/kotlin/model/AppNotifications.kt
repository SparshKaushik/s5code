package club.touchtech.s5code.kotlin.model

/**
 * App-wide transient action presentation.
 *
 * Long operations publish here instead of owning a spinner inside the screen that
 * started them. That keeps progress visible when the user navigates, while the
 * underlying content stays interactive.
 */
data class ActionProgress(
    val id: Long,
    val phase: ActionProgressPhase,
    val label: String,
    val description: String? = null,
    val linkUrl: String? = null,
)

enum class ActionProgressPhase {
    Running,
    Success,
    Error,
}

/** A transient RPC/network failure shown above navigation rather than in a list. */
data class AppErrorNotice(val id: Long, val message: String)
