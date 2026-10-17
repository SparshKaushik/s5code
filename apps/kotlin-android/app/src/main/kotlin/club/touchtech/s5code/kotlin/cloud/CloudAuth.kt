package club.touchtech.s5code.kotlin.cloud

import android.content.Context
import com.clerk.api.Clerk
import com.clerk.api.network.serialization.successOrNull
import com.clerk.api.session.GetTokenOptions
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.launchIn
import kotlinx.coroutines.flow.onEach
import kotlinx.coroutines.launch

/** What the UI needs to know about the S5 Connect account on this device. */
sealed interface CloudAccountState {
    /** This build carries no Clerk/relay configuration, so Connect is off. */
    data object Unconfigured : CloudAccountState

    /** The SDK is starting; a cold start passes through here for a frame or two. */
    data object Loading : CloudAccountState

    data class SignedOut(val error: String? = null) : CloudAccountState

    data class SignedIn(
        val accountId: String,
        val email: String?,
        val name: String?,
        val imageUrl: String?,
    ) : CloudAccountState {
        /** What to show as the account's identity, preferring the email. */
        val label: String
            get() = email ?: name ?: accountId
    }
}

/**
 * The S5 Connect account, backed by Clerk's Android SDK.
 *
 * This is the Kotlin counterpart of `CloudAuthProvider` in
 * `apps/mobile/src/features/cloud/CloudAuthProvider.tsx`, and deliberately keeps
 * the same three responsibilities:
 *
 * - **Initialize once per process.** Clerk's `initialize` is non-blocking and its
 *   `isInitialized` flow reports when the client is usable, so the UI gates on
 *   [state] rather than assuming the SDK is ready.
 * - **Expose the session, not the SDK.** Screens read [state]; nothing outside
 *   this file touches `Clerk` directly. That is what lets the relay slice change
 *   without editing feature code.
 * - **Hand out relay tokens on demand.** [readRelayToken] mints a JWT from the
 *   configured template, which is the credential the relay accepts. `skipCache`
 *   matches `relayClerkTokenOptions` in `packages/shared/src/relayAuth.ts`: the
 *   relay rejects a stale token, and a cached one is exactly what a device that
 *   slept for an hour would hand it.
 *
 * Sign-out clears the account and, once the relay slice lands, is where the DPoP
 * key and any registered device are dropped. Every relay-touching side effect
 * belongs here rather than in a screen, because sign-out has to happen even when
 * no screen is mounted.
 */
class CloudAuth(
    context: Context,
    private val scope: CoroutineScope,
    val config: CloudPublicConfig = CloudPublicConfig.fromBuildConfig(),
    /** Cleared on sign-out, along with the account. Null in builds without a relay. */
    private val onSignOut: suspend () -> Unit = {},
) {
    private val _state =
        MutableStateFlow<CloudAccountState>(
            if (config.configured) CloudAccountState.Loading else CloudAccountState.Unconfigured
        )
    val state: StateFlow<CloudAccountState> = _state.asStateFlow()

    /**
     * The relay account this device currently acts as, or null. Kept separate from
     * [state] because the relay layer only cares about the id, and re-deriving it
     * from a sealed hierarchy at every call site is noise.
     */
    val accountId: StateFlow<String?>
        get() = _accountId.asStateFlow()

    private val _accountId = MutableStateFlow<String?>(null)

    init {
        val publishableKey = config.publishableKey
        if (publishableKey != null) {
            Clerk.initialize(context, publishableKey = publishableKey)
            observe()
        }
    }

    /**
     * Mirrors the RN bridge's `useEffect`: the account is whatever Clerk says it
     * is, and every transition is published rather than inferred at read time.
     */
    private fun observe() {
        combine(Clerk.isInitialized, Clerk.userFlow, Clerk.initializationError) {
                initialized,
                user,
                error ->
                when {
                    error != null -> CloudAccountState.SignedOut(error.message ?: "Sign-in is unavailable.")
                    !initialized -> CloudAccountState.Loading
                    user != null ->
                        CloudAccountState.SignedIn(
                            accountId = user.id,
                            email = user.primaryEmailAddress?.emailAddress,
                            name =
                                listOfNotNull(user.firstName, user.lastName)
                                    .joinToString(" ")
                                    .ifBlank { null } ?: user.username,
                            imageUrl = user.imageUrl,
                        )
                    else -> CloudAccountState.SignedOut()
                }
            }
            .onEach { next ->
                _state.value = next
                _accountId.value = (next as? CloudAccountState.SignedIn)?.accountId
            }
            .launchIn(scope)
    }

    /**
     * A relay-scoped JWT for the signed-in account, or null when signed out.
     *
     * Returning null rather than throwing is deliberate: the caller is usually a
     * reconnect, and a signed-out device should stop reaching for the relay, not
     * surface an error the user cannot act on.
     */
    suspend fun readRelayToken(): String? {
        val template = config.jwtTemplate ?: return null
        if (_state.value !is CloudAccountState.SignedIn) return null
        return Clerk.auth
            .getToken(GetTokenOptions(template = template, skipCache = true))
            .successOrNull()
    }

    fun signOut() {
        scope.launch {
            // Optimistically local: Clerk's own flow is the source of truth, but a
            // failed network sign-out must still leave this device signed out.
            // Unregister while Clerk can still mint the relay token. Once the
            // session is gone, a local cleanup could not remove the server-side
            // FCM target and the signed-out phone would keep receiving pushes.
            onSignOut()
            Clerk.auth.signOut()
        }
    }
}
