package club.touchtech.s5code.kotlin.platform.notifications

import android.content.Context
import androidx.core.content.edit
import java.util.UUID
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Relay registration lifecycle shown by notification settings. */
enum class PushRegistrationStatus {
    Unconfigured,
    SignedOut,
    PermissionRequired,
    TokenPending,
    Registering,
    Registered,
    Failed,
}

data class PushRuntimeState(
    val firebaseConfigured: Boolean = false,
    val tokenAvailable: Boolean = false,
    val status: PushRegistrationStatus = PushRegistrationStatus.Unconfigured,
    val detail: String? = null,
    val updatedAtMillis: Long? = null,
)

/**
 * Small process-wide bridge between the FCM service, application startup, and the
 * ViewModel-owned relay registrar. Only opaque token metadata is published in the
 * flow; the token itself remains private and is read only by the registrar.
 */
object PushRuntime {
    private const val NAME = "s5code.push-runtime"
    private const val KEY_DEVICE_ID = "device-id"
    private const val KEY_TOKEN = "fcm-token"
    private const val KEY_IDENTITY = "registration-identity"
    private const val KEY_SIGNATURE = "registration-signature"

    private val _state = MutableStateFlow(PushRuntimeState())
    val state: StateFlow<PushRuntimeState> = _state.asStateFlow()

    @Volatile private var tokenListener: ((String) -> Unit)? = null

    fun initialize(context: Context, firebaseConfigured: Boolean) {
        val token = preferences(context).getString(KEY_TOKEN, null)
        _state.value =
            _state.value.copy(
                firebaseConfigured = firebaseConfigured,
                tokenAvailable = !token.isNullOrBlank(),
                status =
                    if (firebaseConfigured) PushRegistrationStatus.SignedOut
                    else PushRegistrationStatus.Unconfigured,
                detail =
                    if (firebaseConfigured) null
                    else "This build has no Firebase client for club.touchtech.s5code.kotlin.",
            )
    }

    fun deviceId(context: Context): String {
        val saved = preferences(context).getString(KEY_DEVICE_ID, null)
        if (!saved.isNullOrBlank()) return saved
        return UUID.randomUUID().toString().also { generated ->
            preferences(context).edit(commit = true) { putString(KEY_DEVICE_ID, generated) }
        }
    }

    fun savedToken(context: Context): String? =
        preferences(context).getString(KEY_TOKEN, null)?.trim()?.takeIf(String::isNotEmpty)

    fun onToken(context: Context, token: String) {
        val normalized = token.trim()
        if (normalized.isEmpty()) return
        preferences(context).edit(commit = true) { putString(KEY_TOKEN, normalized) }
        _state.value =
            _state.value.copy(
                tokenAvailable = true,
                status = PushRegistrationStatus.TokenPending,
                detail = "FCM token ready; waiting for relay registration.",
                updatedAtMillis = System.currentTimeMillis(),
            )
        tokenListener?.invoke(normalized)
    }

    fun setTokenListener(listener: ((String) -> Unit)?) {
        tokenListener = listener
    }

    fun publish(status: PushRegistrationStatus, detail: String? = null) {
        _state.value =
            _state.value.copy(
                status = status,
                detail = detail,
                updatedAtMillis = System.currentTimeMillis(),
            )
    }

    fun acceptedRegistration(context: Context, identity: String, signature: String) {
        preferences(context).edit {
            putString(KEY_IDENTITY, identity)
            putString(KEY_SIGNATURE, signature)
        }
        publish(PushRegistrationStatus.Registered, "This device is registered with S5 Connect.")
    }

    fun registrationMatches(context: Context, identity: String, signature: String): Boolean {
        val preferences = preferences(context)
        return preferences.getString(KEY_IDENTITY, null) == identity &&
            preferences.getString(KEY_SIGNATURE, null) == signature
    }

    fun clearAccountRegistration(context: Context) {
        preferences(context).edit {
            remove(KEY_IDENTITY)
            remove(KEY_SIGNATURE)
        }
        publish(
            if (_state.value.firebaseConfigured) PushRegistrationStatus.SignedOut
            else PushRegistrationStatus.Unconfigured
        )
    }

    private fun preferences(context: Context) =
        context.getSharedPreferences(NAME, Context.MODE_PRIVATE)
}
