package club.touchtech.s5code.kotlin.cloud

import java.io.IOException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import okhttp3.FormBody
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.toRequestBody

/** Relay scopes, from `RelayDpopAccessTokenScope` in `packages/contracts/src/relay.ts`. */
object RelayScopes {
    const val EnvironmentConnect = "environment:connect"
    const val EnvironmentStatus = "environment:status"
    const val MobileRegistration = "mobile:registration"
}

/** A managed environment the signed-in account has linked to the relay. */
@Serializable
data class RelayEnvironmentDto(
    val environmentId: String,
    val label: String,
    val endpoint: RelayEndpointDto,
    val linkedAt: String = "",
)

@Serializable
data class RelayEndpointDto(
    val httpBaseUrl: String,
    val wsBaseUrl: String,
    val providerKind: String = "cloudflare_tunnel",
)

@Serializable
private data class RelayEnvironmentListDto(val environments: List<RelayEnvironmentDto> = emptyList())

/**
 * A client the account has registered for notifications, from
 * `RelayClientDeviceRecord`.
 *
 * Every field below `platform` is defaulted rather than required: the relay adds
 * device metadata over time (`appVersion` and `liveActivities` postdate the first
 * shipping clients), and a build that refuses to decode a record it does not
 * fully recognize would show an account no devices at all.
 */
@Serializable
data class RelayDeviceDto(
    val deviceId: String,
    val label: String,
    val platform: String,
    val iosMajorVersion: Int? = null,
    val appVersion: String? = null,
    val notifications: RelayDeviceNotificationsDto = RelayDeviceNotificationsDto(),
    val liveActivities: RelayDeviceLiveActivitiesDto = RelayDeviceLiveActivitiesDto(),
    val updatedAt: String = "",
)

@Serializable
data class RelayDeviceNotificationsDto(
    val enabled: Boolean = false,
    val notifyOnApproval: Boolean = false,
    val notifyOnInput: Boolean = false,
    val notifyOnCompletion: Boolean = false,
    val notifyOnFailure: Boolean = false,
)

@Serializable data class RelayDeviceLiveActivitiesDto(val enabled: Boolean = false)

@Serializable
data class RelayAgentAwarenessPreferencesDto(
    val liveActivitiesEnabled: Boolean,
    val notificationsEnabled: Boolean,
    val notifyOnApproval: Boolean,
    val notifyOnInput: Boolean,
    val notifyOnCompletion: Boolean,
    val notifyOnFailure: Boolean,
)

@Serializable
data class RelayDeviceRegistrationRequestDto(
    val deviceId: String,
    val label: String,
    val platform: String,
    val appVersion: String? = null,
    val fcmToken: String? = null,
    val preferences: RelayAgentAwarenessPreferencesDto,
)

@Serializable data class RelayAndroidLiveUpdateRegistrationRequestDto(val deviceId: String, val generationId: String)

@Serializable private data class RelayOkDto(val ok: Boolean)

@Serializable
private data class RelayDeviceListDto(val devices: List<RelayDeviceDto> = emptyList())

/** Short-lived credential for pairing with one managed environment. */
@Serializable
data class RelayConnectDto(
    val environmentId: String,
    val endpoint: RelayEndpointDto,
    val credential: String,
    val expiresAt: String = "",
)

@Serializable
data class RelayStatusDto(
    val environmentId: String,
    val endpoint: RelayEndpointDto,
    val status: String,
    val checkedAt: String = "",
    val error: String? = null,
)

@Serializable
private data class RelayAccessTokenDto(
    val access_token: String,
    val token_type: String = "DPoP",
    val expires_in: Long = 0,
    val scope: String = "",
)

/** A relay failure with a message a screen can show. */
class RelayError(override val message: String, val unauthorized: Boolean = false) : Exception(message)

/**
 * The S5 Connect relay, as this client uses it.
 *
 * Three things happen here and nothing else:
 *
 * 1. **Bearer calls** carry the Clerk token directly. Only listing linked
 *    environments works this way, matching `RelayClientGroup`.
 * 2. **DPoP calls** carry a relay-issued access token bound to this device's
 *    [DpopKey], plus a fresh proof per request. Status and connect are here.
 * 3. **Token exchange** trades a Clerk token for that access token, scoped. The
 *    result is cached in memory per scope set, because the relay charges a round
 *    trip for it and a status poll would otherwise pay twice.
 *
 * The cache is memory-only on purpose: these tokens live an hour, and persisting
 * them would put a bearer credential on disk to save one request after a cold
 * start.
 */
class RelayClient(
    private val relayUrl: String,
    private val client: OkHttpClient,
    private val key: DpopKey,
    private val clerkToken: suspend () -> String?,
    private val json: Json = Json { ignoreUnknownKeys = true },
) {
    private class CachedToken(val accessToken: String, val scopes: Set<String>, val expiresAtMillis: Long)

    private val tokenLock = Mutex()
    private var cached: CachedToken? = null

    private val base = relayUrl.trimEnd('/')

    val deviceThumbprint: String
        get() = key.thumbprint

    suspend fun listEnvironments(): List<RelayEnvironmentDto> {
        val token = requireClerkToken()
        val request =
            Request.Builder()
                .url("$base/v1/environments")
                .header("authorization", "Bearer $token")
                .get()
                .build()
        return execute(request, RelayEnvironmentListDto.serializer()).environments
    }

    /**
     * Clients registered to this account, as the web profile's "Mobile clients"
     * page lists them (`apps/web/src/cloud/linkEnvironment.ts:listCloudDevices`).
     *
     * Bearer, not DPoP: this is account metadata rather than access to an
     * environment, and the relay's client group authenticates it with the Clerk
     * token alone.
     */
    suspend fun listDevices(): List<RelayDeviceDto> {
        val token = requireClerkToken()
        val request =
            Request.Builder()
                .url("$base/v1/client/devices")
                .header("authorization", "Bearer $token")
                .get()
                .build()
        return execute(request, RelayDeviceListDto.serializer()).devices
    }

    suspend fun environmentStatus(environmentId: String): RelayStatusDto {
        val url = "$base/v1/environments/${encode(environmentId)}/status"
        return dpopRequest(
            url = url,
            method = "POST",
            scopes = setOf(RelayScopes.EnvironmentStatus, RelayScopes.EnvironmentConnect),
            body = EMPTY_JSON,
            serializer = RelayStatusDto.serializer(),
        )
    }

    /**
     * Mints a pairing credential for one managed environment.
     *
     * The device's thumbprint travels in the payload so the credential the
     * environment later accepts is bound to this key: a credential intercepted in
     * flight cannot be exchanged by anything that does not hold the Keystore key.
     */
    suspend fun connectEnvironment(environmentId: String, deviceId: String?): RelayConnectDto {
        val url = "$base/v1/environments/${encode(environmentId)}/connect"
        val payload = buildString {
            append("{\"clientKeyThumbprint\":\"").append(key.thumbprint).append("\"")
            append(",\"clientProofKeyThumbprint\":\"").append(key.thumbprint).append("\"")
            if (deviceId != null) append(",\"deviceId\":\"").append(deviceId).append("\"")
            append("}")
        }
        return dpopRequest(
            url = url,
            method = "POST",
            scopes = setOf(RelayScopes.EnvironmentConnect),
            body = payload.toRequestBody(JSON_MEDIA),
            serializer = RelayConnectDto.serializer(),
        )
    }

    suspend fun registerDevice(registration: RelayDeviceRegistrationRequestDto) {
        dpopRequest(
            url = "$base/v1/mobile/devices",
            method = "POST",
            scopes = setOf(RelayScopes.MobileRegistration),
            body = json.encodeToString(RelayDeviceRegistrationRequestDto.serializer(), registration).toRequestBody(JSON_MEDIA),
            serializer = RelayOkDto.serializer(),
        )
    }

    suspend fun registerAndroidLiveUpdate(deviceId: String, generationId: String) {
        val registration = RelayAndroidLiveUpdateRegistrationRequestDto(deviceId, generationId)
        dpopRequest(
            url = "$base/v1/mobile/android-live-updates",
            method = "POST",
            scopes = setOf(RelayScopes.MobileRegistration),
            body = json.encodeToString(
                    RelayAndroidLiveUpdateRegistrationRequestDto.serializer(),
                    registration,
                )
                .toRequestBody(JSON_MEDIA),
            serializer = RelayOkDto.serializer(),
        )
    }

    suspend fun unregisterDevice(deviceId: String) {
        dpopRequest(
            url = "$base/v1/mobile/devices/${encode(deviceId)}",
            method = "DELETE",
            scopes = setOf(RelayScopes.MobileRegistration),
            body = EMPTY_JSON,
            serializer = RelayOkDto.serializer(),
        )
    }

    /** Drops the cached access token, for sign-out. */
    suspend fun reset() = tokenLock.withLock { cached = null }

    /* ── Internals ───────────────────────────────────────────────────── */

    private suspend fun <T> dpopRequest(
        url: String,
        method: String,
        scopes: Set<String>,
        body: RequestBody,
        serializer: kotlinx.serialization.KSerializer<T>,
    ): T {
        suspend fun attempt(refresh: Boolean): T {
            val accessToken = accessToken(scopes, forceRefresh = refresh)
            val request =
                Request.Builder()
                    .url(url)
                    .header("authorization", "DPoP $accessToken")
                    .header("dpop", key.createProof(method, url, accessToken))
                    .method(method, body)
                    .build()
            return execute(request, serializer)
        }
        return try {
            attempt(refresh = false)
        } catch (error: RelayError) {
            // One retry on a rejected token: the relay revokes on sign-out
            // elsewhere, and a stale cache entry should not read as a sign-in
            // problem to the user.
            if (error.unauthorized) attempt(refresh = true) else throw error
        }
    }

    private suspend fun accessToken(scopes: Set<String>, forceRefresh: Boolean): String =
        tokenLock.withLock {
            val now = System.currentTimeMillis()
            val current = cached
            if (!forceRefresh &&
                current != null &&
                current.expiresAtMillis > now + 5_000 &&
                scopes.all { it in current.scopes }
            ) {
                return@withLock current.accessToken
            }
            val clerk = requireClerkToken()
            val tokenUrl = "$base/v1/client/dpop-token"
            val form =
                FormBody.Builder()
                    .add("grant_type", TOKEN_EXCHANGE_GRANT)
                    .add("subject_token", clerk)
                    .add("subject_token_type", JWT_TOKEN_TYPE)
                    .add("requested_token_type", ACCESS_TOKEN_TYPE)
                    // The relay checks `resource` against its own issuer, which is
                    // the origin with no trailing slash.
                    .add("resource", base)
                    .add("scope", scopes.sorted().joinToString(" "))
                    .add("client_id", MOBILE_CLIENT_ID)
                    .build()
            val request =
                Request.Builder()
                    .url(tokenUrl)
                    // No `ath` on the bootstrap: there is no access token yet.
                    .header("dpop", key.createProof("POST", tokenUrl))
                    .post(form)
                    .build()
            val response = execute(request, RelayAccessTokenDto.serializer())
            cached =
                CachedToken(
                    accessToken = response.access_token,
                    scopes = response.scope.split(' ').filter { it.isNotBlank() }.toSet(),
                    expiresAtMillis = now + response.expires_in * 1_000,
                )
            response.access_token
        }

    private suspend fun requireClerkToken(): String =
        clerkToken() ?: throw RelayError("Sign in to S5 Connect first.", unauthorized = true)

    private suspend fun <T> execute(
        request: Request,
        serializer: kotlinx.serialization.KSerializer<T>,
    ): T =
        withContext(Dispatchers.IO) {
            val response =
                try {
                    client.newCall(request).execute()
                } catch (cause: IOException) {
                    throw RelayError("Could not reach the S5 Connect relay.")
                }
            response.use {
                val body = it.body.string()
                if (!it.isSuccessful) throw relayFailure(it.code, body)
                try {
                    json.decodeFromString(serializer, body)
                } catch (cause: Exception) {
                    throw RelayError("The relay returned a response this build cannot read.")
                }
            }
        }

    /**
     * Turns a relay error body into something a screen can show. Relay errors are
     * tagged (`RelayProtectedError`), and the tag is more useful than the status:
     * "no active link" and "bad token" are both 401-shaped but have different
     * fixes.
     */
    private fun relayFailure(status: Int, body: String): RelayError {
        val tag = TAG_PATTERN.find(body)?.groupValues?.getOrNull(1)
        val reason = REASON_PATTERN.find(body)?.groupValues?.getOrNull(1)
        val message =
            when (tag) {
                "RelayAuthInvalidError" ->
                    when (reason) {
                        "invalid_dpop" -> "The relay rejected this device's proof key."
                        "not_authorized" -> "Your account is not authorized for that environment."
                        else -> "The relay rejected the sign-in token."
                    }
                "RelayEnvironmentConnectNotAuthorizedError" ->
                    if (reason == "environment_link_not_found") {
                        "The relay has no active link for this machine. Start S5 Code on it and " +
                            "enable S5 Connect."
                    } else {
                        "The relay refused the connection ($reason)."
                    }
                "RelayEnvironmentEndpointUnavailableError" ->
                    "That machine's tunnel is not reachable right now."
                "RelayEnvironmentEndpointTimedOutError" -> "That machine did not answer in time."
                else -> "The relay returned an error ($status)."
            }
        return RelayError(message, unauthorized = status == 401 || status == 403)
    }

    private fun encode(value: String): String = java.net.URLEncoder.encode(value, "UTF-8")

    private companion object {
        const val TOKEN_EXCHANGE_GRANT = "urn:ietf:params:oauth:grant-type:token-exchange"
        const val JWT_TOKEN_TYPE = "urn:ietf:params:oauth:token-type:jwt"
        const val ACCESS_TOKEN_TYPE = "urn:ietf:params:oauth:token-type:access_token"
        const val MOBILE_CLIENT_ID = "t3-mobile"

        val JSON_MEDIA = "application/json".toMediaType()
        val EMPTY_JSON: RequestBody = "{}".toRequestBody(JSON_MEDIA)

        val TAG_PATTERN = Regex("\"_tag\"\\s*:\\s*\"([^\"]+)\"")
        val REASON_PATTERN = Regex("\"reason\"\\s*:\\s*\"([^\"]+)\"")
    }
}
