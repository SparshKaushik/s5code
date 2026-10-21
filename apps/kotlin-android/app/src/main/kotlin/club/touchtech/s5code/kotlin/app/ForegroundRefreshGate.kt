package club.touchtech.s5code.kotlin.app

import androidx.lifecycle.Lifecycle

/**
 * Tracks whether an ON_START is a genuine return from background and how long it
 * lasted. A newly composed root receives ON_START too, but its sessions already
 * start from the saved environment list and must not be restarted immediately.
 * The duration feeds the probe-vs-restart decision: under a few seconds the
 * socket probably survived and only needs probing.
 */
internal class ForegroundRefreshGate {
    private var stoppedAtMillis: Long? = null

    /** Returns the backgrounded duration in millis on a real return, else null. */
    fun onEvent(event: Lifecycle.Event): Long? =
        when (event) {
            Lifecycle.Event.ON_STOP -> {
                stoppedAtMillis = System.currentTimeMillis()
                null
            }
            Lifecycle.Event.ON_START -> {
                val at = stoppedAtMillis
                stoppedAtMillis = null
                at?.let { System.currentTimeMillis() - it }
            }
            else -> null
        }
}
