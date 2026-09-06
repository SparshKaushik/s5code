package club.touchtech.s5code.kotlin.app

import androidx.lifecycle.Lifecycle

internal fun shouldRefreshAfterLifecycleEvent(
    stopped: Boolean,
    event: String,
): Pair<Boolean, Boolean> =
    when (event) {
        "stop" -> true to false
        "start" -> false to stopped
        else -> stopped to false
    }

/**
 * Tracks whether an ON_START is a genuine return from background. A newly
 * composed root receives ON_START too, but its sessions already start from the
 * saved environment list and must not be restarted immediately.
 */
internal class ForegroundRefreshGate {
    private var stopped = false

    fun onEvent(event: Lifecycle.Event): Boolean {
        val (nextStopped, refresh) =
            shouldRefreshAfterLifecycleEvent(
                stopped = stopped,
                event =
                    when (event) {
                        Lifecycle.Event.ON_STOP -> "stop"
                        Lifecycle.Event.ON_START -> "start"
                        else -> "other"
                    },
            )
        stopped = nextStopped
        return refresh
    }
}
