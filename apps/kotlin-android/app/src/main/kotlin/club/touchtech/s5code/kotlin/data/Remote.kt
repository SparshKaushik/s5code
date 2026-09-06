package club.touchtech.s5code.kotlin.data

import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.State
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import kotlinx.coroutines.CancellationException

/**
 * A value read from an environment over the wire.
 *
 * Every tool read (files, diffs, git status, terminals, usage) can be slow,
 * absent, or refused, and a screen that models it as a plain value has to invent
 * something to render for each of those. Three states is the smallest set that
 * lets a screen be honest: it is loading, it has data, or it failed and here is
 * why.
 */
@Immutable
sealed interface Remote<out T> {
    data object Loading : Remote<Nothing>

    data class Loaded<T>(val value: T) : Remote<T>

    data class Failed(val message: String, val retryable: Boolean = true) : Remote<Nothing>

    val valueOrNull: T?
        get() = (this as? Loaded)?.value
}

/**
 * Runs a one-shot read and exposes it as [Remote]. Re-runs when [keys] change,
 * and cancels with the composition, so leaving a screen mid-read does not keep
 * the request alive.
 */
@Composable
fun <T> rememberRemote(vararg keys: Any?, load: suspend () -> T): State<Remote<T>> {
    return produceState<Remote<T>>(Remote.Loading, keys = keys) {
        value = Remote.Loading
        value =
            try {
                Remote.Loaded(load())
            } catch (cancellation: CancellationException) {
                throw cancellation
            } catch (cause: Throwable) {
                Remote.Failed(cause.message ?: "That read failed.")
            }
    }
}

/**
 * Same as [rememberRemote] but hands back a retry action. Used where a failed
 * read is worth offering to repeat rather than requiring a screen change: a
 * dropped socket mid-read is common and re-navigating to fix it is silly.
 */
@Composable
fun <T> rememberRetryableRemote(
    vararg keys: Any?,
    load: suspend () -> T,
): Pair<State<Remote<T>>, () -> Unit> {
    val attempt = remember(*keys) { mutableIntStateOf(0) }
    val state =
        produceState<Remote<T>>(Remote.Loading, keys = arrayOf(*keys, attempt.intValue)) {
            value = Remote.Loading
            value =
                try {
                    Remote.Loaded(load())
                } catch (cancellation: CancellationException) {
                    throw cancellation
                } catch (cause: Throwable) {
                    Remote.Failed(cause.message ?: "That read failed.")
                }
        }
    return state to { attempt.intValue += 1 }
}
