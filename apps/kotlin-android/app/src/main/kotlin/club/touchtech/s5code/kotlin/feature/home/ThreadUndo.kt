package club.touchtech.s5code.kotlin.feature.home

import club.touchtech.s5code.kotlin.data.parseInstant
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadSummary

/** The list actions Home offers to undo, named the way the notice reads ("Settled thread"). */
enum class ThreadUndoAction(val label: String) {
    Settled("Settled"),
    Snoozed("Snoozed"),
    Unpinned("Unpinned"),
}

/** One write that puts a thread back: `ThreadUndoStep` in client-runtime's `threadUndo.ts`. */
sealed interface ThreadUndoStep {
    data object Unsettle : ThreadUndoStep
    data object Unsnooze : ThreadUndoStep
    data class Pin(val orderKey: String?) : ThreadUndoStep
    data class Snooze(val untilIso: String) : ThreadUndoStep
}

/**
 * The writes that reverse [action], following `planThreadUndo`. Planned from the
 * row as it was *before* the action, because the action itself erases what has
 * to come back: settling clears a pin, and pinning or settling clears a snooze
 * on the server. The pin is restored before the snooze for the same reason —
 * the pin write would clear a snooze restored ahead of it.
 */
fun planThreadUndo(action: ThreadUndoAction, before: ThreadSummary): List<ThreadUndoStep> {
    if (action == ThreadUndoAction.Snoozed) return listOf(ThreadUndoStep.Unsnooze)
    val pin = ThreadUndoStep.Pin(before.pinOrderKey)
    val snooze = listOfNotNull(before.snoozedUntil?.let(ThreadUndoStep::Snooze))
    return when (action) {
        ThreadUndoAction.Unpinned -> listOf(pin) + snooze
        else -> listOf(ThreadUndoStep.Unsettle) + listOfNotNull(pin.takeIf { before.pinned }) + snooze
    }
}

/**
 * Runs [plan] in order, following `executeThreadUndo`: a failed step throws and
 * stops the rest, and a snooze that has expired since is skipped, because the
 * server rejects restoring a wake time in the past.
 */
suspend fun executeThreadUndo(
    plan: List<ThreadUndoStep>,
    nowMillis: () -> Long,
    execute: suspend (ThreadUndoStep) -> Unit,
) {
    for (step in plan) {
        if (step is ThreadUndoStep.Snooze && (parseInstant(step.untilIso) ?: 0) <= nowMillis()) continue
        execute(step)
    }
}

/**
 * The pending Undo for one thread. A later action on the same thread replaces
 * it. Identity, not value, equality: the same action twice is two notices, each
 * with its own expiry.
 */
class ThreadUndoNotice(
    val action: ThreadUndoAction,
    val environmentId: EnvironmentId,
    val threadId: ThreadId,
    val plan: List<ThreadUndoStep>,
)
