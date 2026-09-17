package club.touchtech.s5code.kotlin.platform.notifications

import android.content.Context
import android.os.Build
import club.touchtech.s5code.kotlin.BuildConfig
import club.touchtech.s5code.kotlin.cloud.CloudAccountState
import club.touchtech.s5code.kotlin.cloud.RelayAgentAwarenessPreferencesDto
import club.touchtech.s5code.kotlin.cloud.RelayClient
import club.touchtech.s5code.kotlin.cloud.RelayDeviceRegistrationRequestDto
import club.touchtech.s5code.kotlin.cloud.RelayError
import club.touchtech.s5code.kotlin.data.RuntimePreferences
import club.touchtech.s5code.kotlin.transport.PairingClient
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.launchIn
import kotlinx.coroutines.flow.onEach
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * Keeps the relay's device record synchronized with token, account, permission,
 * and user preferences. Calls are serialized and signature-deduplicated so an FCM
 * rotation cannot overtake a preference update and restore stale data.
 */
class PushRegistrationCoordinator(
    private val context: Context,
    private val scope: CoroutineScope,
    private val relay: RelayClient?,
    private val account: StateFlow<CloudAccountState>,
    private val preferences: StateFlow<RuntimePreferences>,
) {
    private val registrationLock = Mutex()
    private var observation: Job? = null

    fun start() {
        PushRuntime.setTokenListener { scope.launch { refresh() } }
        observation =
            combine(account, preferences) { accountState, preferenceState ->
                    accountState to preferenceState
                }
                .onEach { refresh() }
                .launchIn(scope)
        if (PushRuntime.state.value.firebaseConfigured) {
            PushRuntime.publish(PushRegistrationStatus.TokenPending, "Requesting an FCM token…")
            S5Firebase.requestToken { result ->
                result.onSuccess { PushRuntime.onToken(context, it) }
                    .onFailure {
                        PushRuntime.publish(
                            PushRegistrationStatus.Failed,
                            it.message ?: "Firebase could not issue a messaging token.",
                        )
                    }
            }
        }
    }

    /**
     * A new turn un-dismisses the ongoing card so the next relay update shows.
     * The relay drives the card from its aggregate payload; there is no
     * server-side registration for it.
     */
    fun arm(threadTitle: String, projectTitle: String) {
        if (!preferences.value.liveUpdatesEnabled || Build.VERSION.SDK_INT < 36) return
        AndroidLiveUpdateNotifications.arm(context)
    }

    fun refresh() {
        scope.launch { synchronize() }
    }

    suspend fun signOut() {
        val deviceId = PushRuntime.deviceId(context)
        val relayClient = relay
        if (relayClient != null && account.value is CloudAccountState.SignedIn) {
            runCatching { relayClient.unregisterDevice(deviceId) }
        }
        AndroidLiveUpdateNotifications.clear(context)
        PushRuntime.clearAccountRegistration(context)
    }

    fun close() {
        observation?.cancel()
        PushRuntime.setTokenListener(null)
    }

    private suspend fun synchronize() =
        registrationLock.withLock {
            val runtime = PushRuntime.state.value
            if (!runtime.firebaseConfigured) {
                PushRuntime.publish(
                    PushRegistrationStatus.Unconfigured,
                    "This build has no Firebase client for club.touchtech.s5code.kotlin.",
                )
                return@withLock
            }
            val signedIn = account.value as? CloudAccountState.SignedIn
            if (signedIn == null || relay == null) {
                PushRuntime.publish(
                    PushRegistrationStatus.SignedOut,
                    if (relay == null) "S5 Connect is not configured." else "Sign in to S5 Connect to register this device.",
                )
                return@withLock
            }
            if (!notificationsAllowed(context)) {
                PushRuntime.publish(
                    PushRegistrationStatus.PermissionRequired,
                    "Allow Android notifications before this device can receive alerts.",
                )
                return@withLock
            }
            val token = PushRuntime.savedToken(context)
            if (token == null) {
                PushRuntime.publish(PushRegistrationStatus.TokenPending, "Waiting for an FCM token…")
                return@withLock
            }
            val preferenceState = preferences.value
            val registration =
                RelayDeviceRegistrationRequestDto(
                    deviceId = PushRuntime.deviceId(context),
                    label = PairingClient.deviceLabel(),
                    platform = "android",
                    androidApiLevel = Build.VERSION.SDK_INT,
                    appVersion = BuildConfig.VERSION_NAME,
                    bundleId = BuildConfig.APPLICATION_ID,
                    pushToken = token,
                    preferences =
                        RelayAgentAwarenessPreferencesDto(
                            liveActivitiesEnabled =
                                Build.VERSION.SDK_INT >= 36 && preferenceState.liveUpdatesEnabled,
                            notificationsEnabled = true,
                            notifyOnApproval = preferenceState.notifyApprovals,
                            notifyOnInput = preferenceState.notifyInput,
                            notifyOnCompletion = preferenceState.notifyCompletion,
                            notifyOnFailure = preferenceState.notifyFailures,
                        ),
                )
            val signature = registration.signature()
            if (PushRuntime.registrationMatches(context, signedIn.accountId, signature)) {
                PushRuntime.publish(PushRegistrationStatus.Registered, "This device is registered with S5 Connect.")
                // Identity may already be configured; configure is idempotent.
                AndroidLiveUpdateNotifications.configure(
                    context,
                    deviceId = registration.deviceId,
                    userId = signedIn.accountId,
                    ongoingEnabled = registration.preferences.liveActivitiesEnabled,
                )
                return@withLock
            }
            PushRuntime.publish(PushRegistrationStatus.Registering, "Registering this device with S5 Connect…")
            try {
                relay.registerDevice(registration)
                PushRuntime.acceptedRegistration(context, signedIn.accountId, signature)
                AndroidLiveUpdateNotifications.configure(
                    context,
                    deviceId = registration.deviceId,
                    userId = signedIn.accountId,
                    ongoingEnabled = registration.preferences.liveActivitiesEnabled,
                )
            } catch (error: Exception) {
                PushRuntime.publish(
                    PushRegistrationStatus.Failed,
                    (error as? RelayError)?.message ?: error.message ?: "Device registration failed.",
                )
            }
        }
    }

internal fun RelayDeviceRegistrationRequestDto.signature(): String =
    listOf(
            BuildConfig.RELAY_URL,
        deviceId,
            label,
            platform,
            androidApiLevel?.toString().orEmpty(),
            appVersion.orEmpty(),
            bundleId.orEmpty(),
            pushToken.orEmpty(),
            preferences.liveActivitiesEnabled,
            preferences.notificationsEnabled,
            preferences.notifyOnApproval,
            preferences.notifyOnInput,
            preferences.notifyOnCompletion,
            preferences.notifyOnFailure,
        )
        .joinToString("|")
