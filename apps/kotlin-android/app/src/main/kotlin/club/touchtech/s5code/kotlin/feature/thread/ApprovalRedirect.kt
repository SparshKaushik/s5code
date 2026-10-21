package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.app.ThreadDraft
import club.touchtech.s5code.kotlin.model.PendingApproval
import kotlin.coroutines.cancellation.CancellationException

/**
 * `declinePendingApprovals` from client-runtime: a message sent while approvals
 * wait declines every one of them first, since any one left pending would still
 * block the agent from reading the redirect. Stops at the first decline that
 * fails and returns its error, so the send does not go out behind a gate.
 */
internal suspend fun declinePendingApprovals(
    approvals: List<PendingApproval>,
    respond: suspend (approvalId: String) -> Unit,
): Exception? {
    for (approval in approvals) {
        try {
            respond(approval.id)
        } catch (cancellation: CancellationException) {
            throw cancellation
        } catch (error: Exception) {
            return error
        }
    }
    return null
}

/**
 * Whether the redirect may still go out once its declines settle. A draft edited
 * or pasted into during the wait is no longer what the user sent, and an approval
 * that arrived meanwhile would block the redirect; either way the draft stays so
 * the next send picks it up.
 */
internal fun redirectStillSendable(
    sent: ThreadDraft,
    current: ThreadDraft,
    declined: List<PendingApproval>,
    pending: List<PendingApproval>,
): Boolean =
    current == sent && pending.all { approval -> declined.any { it.id == approval.id } }
