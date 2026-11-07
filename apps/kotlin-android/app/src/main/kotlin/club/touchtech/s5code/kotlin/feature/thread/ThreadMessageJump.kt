package club.touchtech.s5code.kotlin.feature.thread

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import club.touchtech.s5code.kotlin.model.FeedEntry
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * `threadMessageJump.ts` in the RN client: a Home search row listed for a
 * message match opens its thread at that message. The thread screen reads the
 * request while it shows that thread and finishes it once the feed is there or
 * the message cannot be found. A request whose thread never showed lapses, so
 * a later plain open does not jump.
 */
object ThreadMessageJumps {
    private const val PENDING_JUMP_MS = 30_000L

    data class Pending(val threadKey: String, val messageId: String, val atMillis: Long)

    private val state = MutableStateFlow<Pending?>(null)
    val pending = state.asStateFlow()

    fun request(threadKey: String, messageId: String, nowMillis: Long = System.currentTimeMillis()) {
        state.value = Pending(threadKey, messageId, nowMillis)
    }

    /** The message [threadKey] should open at, if a search pick asked for one. */
    fun messageFor(pending: Pending?, threadKey: String, nowMillis: Long = System.currentTimeMillis()): String? =
        pending?.takeIf { it.threadKey == threadKey && nowMillis - it.atMillis < PENDING_JUMP_MS }?.messageId

    fun finish(threadKey: String) {
        state.update { if (it?.threadKey == threadKey) null else it }
    }
}

/** The key [ThreadMessageJumps] files a thread under. */
fun threadJumpKey(environmentId: String, threadId: String) = "$environmentId:$threadId"

/** Whether [entry] is the message [messageId] names; V1 entries carry the message id as their own. */
internal fun FeedEntry.isMessage(messageId: String): Boolean =
    when (this) {
        is FeedEntry.UserMessage -> (this.messageId ?: id) == messageId
        is FeedEntry.AgentMessage -> (this.messageId ?: id) == messageId
        else -> false
    }

/** What the jump does next, given what the screen has loaded so far. */
internal sealed interface MessageJumpStep {
    data class Scroll(val rowIndex: Int) : MessageJumpStep
    data class Unfold(val turnId: String) : MessageJumpStep
    data object LoadEarlier : MessageJumpStep
    data object Wait : MessageJumpStep
    data object GiveUp : MessageJumpStep
}

/**
 * The ThreadDetailScreen/ThreadFeed jump in one decision: scroll when the row
 * is shown, unfold its turn when the message is folded away, page earlier
 * history while it is not loaded, and give up when history runs out.
 */
internal fun messageJumpStep(
    messageId: String,
    feed: List<FeedEntry>,
    rows: List<FeedRow>,
    expandedTurns: Set<String>,
    hasMoreHistory: Boolean,
    loadingHistory: Boolean,
): MessageJumpStep {
    val rowIndex = rows.indexOfFirst { it is FeedRow.Entry && it.entry.isMessage(messageId) }
    if (rowIndex >= 0) return MessageJumpStep.Scroll(rowIndex)
    val entry = feed.firstOrNull { it.isMessage(messageId) }
    if (entry != null) {
        val turnId = entry.turnId
        return if (turnId != null && turnId !in expandedTurns) MessageJumpStep.Unfold(turnId)
        else MessageJumpStep.GiveUp
    }
    return when {
        loadingHistory -> MessageJumpStep.Wait
        hasMoreHistory -> MessageJumpStep.LoadEarlier
        else -> MessageJumpStep.GiveUp
    }
}

/**
 * Runs a pending jump for the shown thread. [scrollToRow] takes an index into
 * [rows]; the screen maps it to its lazy list.
 */
@Composable
internal fun ThreadMessageJumpEffect(
    threadKey: String,
    feed: List<FeedEntry>,
    rows: List<FeedRow>,
    expandedTurns: Set<String>,
    hasMoreHistory: Boolean,
    loadingHistory: Boolean,
    loadEarlier: suspend () -> Unit,
    unfoldTurn: (String) -> Unit,
    scrollToRow: suspend (Int) -> Unit,
) {
    val pending by ThreadMessageJumps.pending.collectAsState()
    val messageId = ThreadMessageJumps.messageFor(pending, threadKey)
    // A jump this screen never reached lets go when the thread closes.
    DisposableEffect(threadKey) { onDispose { ThreadMessageJumps.finish(threadKey) } }
    // Paging runs outside the keyed effect: the page it loads restarts that effect,
    // which would otherwise cancel the request it is waiting on.
    val scope = rememberCoroutineScope()
    var paging by remember(threadKey) { mutableStateOf(false) }
    LaunchedEffect(messageId, feed, rows, expandedTurns, hasMoreHistory, loadingHistory) {
        if (messageId == null) return@LaunchedEffect
        when (val step = messageJumpStep(messageId, feed, rows, expandedTurns, hasMoreHistory, loadingHistory)) {
            is MessageJumpStep.Scroll -> {
                scrollToRow(step.rowIndex)
                ThreadMessageJumps.finish(threadKey)
            }
            is MessageJumpStep.Unfold -> unfoldTurn(step.turnId)
            MessageJumpStep.LoadEarlier ->
                if (!paging) {
                    paging = true
                    scope.launch {
                        try {
                            loadEarlier()
                        } catch (error: CancellationException) {
                            throw error
                        } catch (_: Exception) {
                            // RN's jump finishes when history reports an error.
                            ThreadMessageJumps.finish(threadKey)
                        } finally {
                            paging = false
                        }
                    }
                }
            MessageJumpStep.Wait -> Unit
            MessageJumpStep.GiveUp -> ThreadMessageJumps.finish(threadKey)
        }
    }
}
