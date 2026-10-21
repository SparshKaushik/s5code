package club.touchtech.s5code.kotlin.data

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/**
 * Why a thread's last message did not go out, shown above that thread's
 * composer. The outbox drain can reject a message after the user has left the
 * thread, so the reason is kept per thread until they dismiss it, send again,
 * or the message it describes is delivered after all.
 *
 * Keys are `"<environmentId>:<threadId>"` — `scopedThreadKey` in the RN client
 * (`apps/mobile/src/state/thread-composer-error.ts`, which this mirrors).
 * In-memory only, like the atom it replaces: a stale banner is cleared by the
 * next send, not worth persisting across process death.
 */
data class ThreadComposerError(
    val message: String,
    /** The queued message this error is about, when it is about one. */
    val messageId: String? = null,
)

class ThreadComposerErrorStore {
    private val _errors = MutableStateFlow<Map<String, ThreadComposerError>>(emptyMap())

    /** `threadComposerErrorsAtom`: the whole map, for screens observing one thread. */
    val errors: StateFlow<Map<String, ThreadComposerError>> = _errors.asStateFlow()

    fun set(threadKey: String, message: String, messageId: String? = null) {
        _errors.update { it + (threadKey to ThreadComposerError(message, messageId)) }
    }

    /** With [messageId], clears only an error that describes that message. */
    fun clear(threadKey: String, messageId: String? = null) {
        _errors.update { current ->
            val existing = current[threadKey] ?: return@update current
            if (messageId != null && existing.messageId != messageId) current
            else current - threadKey
        }
    }

    fun clearForEnvironment(environmentId: String) {
        val prefix = "$environmentId:"
        _errors.update { current -> current.filterKeys { !it.startsWith(prefix) } }
    }
}
