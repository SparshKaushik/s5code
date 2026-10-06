package club.touchtech.s5code.kotlin.transport

import android.os.Build
import club.touchtech.s5code.kotlin.connection.PairingTarget
import club.touchtech.s5code.kotlin.data.EnvironmentStore

/**
 * Turns a pairing target into a saved environment.
 *
 * The order matters and is not interchangeable:
 *
 * 1. read the unauthenticated descriptor, so the saved row carries the server's
 *    own id and label rather than the host string someone typed;
 * 2. exchange the one-time credential for a long-lived access token;
 * 3. persist both, encrypted, before returning.
 *
 * The credential is single-use, so a failure after step 2 loses it. That is why
 * the write happens here rather than in a caller that might forget: the token is
 * saved the moment it exists.
 */
class PairingClient(
    private val http: EnvironmentHttp,
    private val store: EnvironmentStore,
    private val deviceLabel: String = deviceLabel(),
) {

    /** Result of a successful pair, enough for the caller to route onward. */
    data class Paired(val environmentId: String, val label: String)

    suspend fun pair(target: PairingTarget): Paired {
        val descriptor = http.descriptor(target.httpBaseUrl)
        val token =
            http.exchangePairingCredential(
                    httpBaseUrl = target.httpBaseUrl,
                    credential = target.credential,
                    deviceLabel = deviceLabel,
                )
                .access_token
        val label = target.label?.takeIf { it.isNotBlank() } ?: descriptor.label
        store.save(
            environmentId = descriptor.environmentId,
            label = label,
            httpBaseUrl = target.httpBaseUrl,
            wsBaseUrl = target.wsBaseUrl,
            token = token,
            platform = descriptor.platform.display,
            serverVersion = descriptor.serverVersion.orEmpty(),
        )
        return Paired(descriptor.environmentId, label)
    }

    companion object {
        /**
         * What the server shows in its client list. The model is more useful there
         * than a generic "Android": a developer with a phone and a tablet paired
         * needs to tell the two rows apart.
         */
        fun deviceLabel(): String =
            listOf(Build.MANUFACTURER?.replaceFirstChar(Char::uppercase), Build.MODEL)
                .filter { !it.isNullOrBlank() }
                .joinToString(" ")
                .ifBlank { "Android device" }
    }
}
