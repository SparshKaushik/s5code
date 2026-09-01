package club.touchtech.s5code.kotlin.transport

import java.io.IOException
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import okhttp3.FormBody
import okhttp3.OkHttpClient
import okhttp3.Request

/**
 * The HTTP half of the environment protocol. Only four endpoints matter to a
 * mobile client, all defined in `packages/contracts/src/environmentHttp.ts`:
 *
 * - `GET /.well-known/t3/environment` — unauthenticated identity, read before
 *   pairing so a saved environment gets its real id and label instead of the
 *   host string the user typed.
 * - `POST /oauth/token` — exchanges the one-time pairing credential for a
 *   long-lived access token.
 * - `POST /api/auth/session` — validates a stored token without opening a socket.
 * - `POST /api/auth/websocket-ticket` — short-lived ticket appended to the
 *   WebSocket URL, because a browser cannot set headers on a WebSocket upgrade
 *   and the server therefore authenticates the socket by query parameter.
 */
/**
 * How an authenticated request proves who it is.
 *
 * Direct pairing yields a bearer token: whoever holds it can use it. A relay
 * connection yields a DPoP-bound token, which is only usable alongside a
 * signature from the device key it was minted against. Both live behind this type
 * so the session, the socket ticket, and every RPC path take one credential and
 * never branch on "is this a cloud environment".
 */
sealed interface EnvironmentCredential {
    val token: String

    data class Bearer(override val token: String) : EnvironmentCredential

    data class Dpop(
        override val token: String,
        /** Signs one proof: method, absolute URL, and the access token for `ath`. */
        val proof: (method: String, url: String, accessToken: String) -> String,
    ) : EnvironmentCredential
}

class EnvironmentHttp(private val client: OkHttpClient, private val json: Json = LenientJson) {

    suspend fun descriptor(httpBaseUrl: String): EnvironmentDescriptorDto =
        get(httpBaseUrl, "/.well-known/t3/environment", authorization = null)

    /**
     * Same exchange, proof-bound. A relay-issued credential is minted against a
     * device key thumbprint, so the environment only accepts it from a client that
     * can sign for that key — hence the proof on the exchange itself, and the
     * `DPoP` token type that comes back.
     */
    suspend fun exchangeProofBoundCredential(
        httpBaseUrl: String,
        credential: String,
        deviceLabel: String,
        proof: (url: String) -> String,
    ): AccessTokenDto {
        val url = endpoint(httpBaseUrl, "/oauth/token")
        val body = tokenExchangeBody(credential, deviceLabel)
        val request = Request.Builder().url(url).header("dpop", proof(url)).post(body).build()
        return execute(request)
    }

    /**
     * Trades the pairing credential for an access token. The grant is RFC 8693
     * token exchange, form-encoded, and the scope list decides what the client
     * may do for the rest of its life on that environment.
     */
    suspend fun exchangePairingCredential(
        httpBaseUrl: String,
        credential: String,
        deviceLabel: String,
    ): AccessTokenDto {
        val request =
            Request.Builder()
                .url(endpoint(httpBaseUrl, "/oauth/token"))
                .post(tokenExchangeBody(credential, deviceLabel))
                .build()
        return execute(request)
    }

    private fun tokenExchangeBody(credential: String, deviceLabel: String): FormBody =
        FormBody.Builder()
            .add("grant_type", TOKEN_EXCHANGE_GRANT)
            .add("subject_token", credential)
            .add("subject_token_type", BOOTSTRAP_TOKEN_TYPE)
            .add("requested_token_type", ACCESS_TOKEN_TYPE)
            .add("scope", CLIENT_SCOPES.joinToString(" "))
            .add("client_label", deviceLabel)
            .add("client_device_type", "mobile")
            .add("client_os", "Android")
            .build()

    /**
     * Validates a stored token without opening a socket.
     *
     * `GET`, not `POST`: the contract defines this one as a read
     * (`EnvironmentAuthHttpApi` in `packages/contracts/src/environmentHttp.ts`),
     * and the server answers a POST here with 404.
     */
    suspend fun session(httpBaseUrl: String, credential: EnvironmentCredential): AuthSessionDto =
        authenticated(httpBaseUrl, "/api/auth/session", credential, method = "GET")

    suspend fun webSocketTicket(
        httpBaseUrl: String,
        credential: EnvironmentCredential,
    ): WebSocketTicketDto =
        authenticated(httpBaseUrl, "/api/auth/websocket-ticket", credential, method = "POST")

    /**
     * Resolves the socket URL for a connection attempt. The ticket is minted per
     * attempt on purpose: it is short-lived, so caching it across reconnects
     * trades a fast path for an authentication failure after a long sleep.
     */
    suspend fun resolveSocketUrl(
        httpBaseUrl: String,
        wsBaseUrl: String,
        credential: EnvironmentCredential,
    ): String {
        val ticket = webSocketTicket(httpBaseUrl, credential).ticket
        return socketUrl(wsBaseUrl, ticket)
    }

    private suspend inline fun <reified T> authenticated(
        httpBaseUrl: String,
        path: String,
        credential: EnvironmentCredential,
        method: String,
    ): T {
        val url = endpoint(httpBaseUrl, path)
        val body = if (method == "GET") null else EMPTY_BODY
        val builder = Request.Builder().url(url).method(method, body)
        when (credential) {
            is EnvironmentCredential.Bearer ->
                builder.header("authorization", "Bearer ${credential.token}")
            is EnvironmentCredential.Dpop -> {
                builder.header("authorization", "DPoP ${credential.token}")
                // The proof carries `ath` over the access token, which the server
                // checks; a proof without it is refused even when the signature is
                // good.
                builder.header("dpop", credential.proof(method, url, credential.token))
            }
        }
        return execute(builder.build())
    }

    private suspend inline fun <reified T> get(
        httpBaseUrl: String,
        path: String,
        authorization: String?,
    ): T {
        val builder = Request.Builder().url(endpoint(httpBaseUrl, path)).get()
        authorization?.let { builder.header("authorization", "Bearer $it") }
        return execute(builder.build())
    }

    private suspend inline fun <reified T> execute(request: Request): T =
        withContext(Dispatchers.IO) {
            val response =
                try {
                    client.newCall(request).execute()
                } catch (cause: IOException) {
                    throw EnvironmentHttpError.unreachable(cause)
                }
            response.use {
                val body = it.body.string()
                if (!it.isSuccessful) throw EnvironmentHttpError.forStatus(it.code, body)
                try {
                    json.decodeFromString<T>(body)
                } catch (cause: Exception) {
                    throw EnvironmentHttpError(
                        EnvironmentHttpErrorKind.Protocol,
                        "That address answered, but not like an S5 Code server.",
                        cause,
                    )
                }
            }
        }

    private fun endpoint(httpBaseUrl: String, path: String): String =
        httpBaseUrl.trimEnd('/') + path

    companion object {
        /**
         * Appends the ticket to the socket URL, matching `resolveRemoteSocketUrl`
         * in `packages/client-runtime/src/authorization/remote.ts`.
         *
         * `/ws` is only added when the base has no path of its own. A relay tunnel
         * can hand back a URL that already routes, and overwriting that path sends
         * the upgrade to the wrong place.
         */
        fun socketUrl(wsBaseUrl: String, ticket: String): String {
            val encoded = java.net.URLEncoder.encode(ticket, "UTF-8")
            val trimmed = wsBaseUrl.trim().trimEnd('/')
            val path = runCatching { java.net.URI(trimmed).rawPath }.getOrNull().orEmpty()
            val base = if (path.isEmpty()) "$trimmed/ws" else trimmed
            return "$base?wsTicket=$encoded"
        }

        const val TOKEN_EXCHANGE_GRANT = "urn:ietf:params:oauth:grant-type:token-exchange"
        const val ACCESS_TOKEN_TYPE = "urn:ietf:params:oauth:token-type:access_token"
        const val BOOTSTRAP_TOKEN_TYPE = "urn:t3:params:oauth:token-type:environment-bootstrap"

        /**
         * `AuthStandardClientScopes` from `packages/contracts/src/auth.ts`. The
         * administrative scopes are deliberately not requested: this client has
         * no UI that manages other clients' access, and asking for authority it
         * cannot exercise makes every paired phone a bigger loss if stolen.
         */
        val CLIENT_SCOPES =
            listOf(
                "orchestration:read",
                "orchestration:operate",
                "terminal:operate",
                "review:write",
                "relay:read",
            )

        private val EMPTY_BODY = FormBody.Builder().build()

        val LenientJson = Json {
            ignoreUnknownKeys = true
            explicitNulls = false
            coerceInputValues = true
        }

        /**
         * Cleartext HTTP to a LAN address is the normal case for this product,
         * so no upgrade policy is applied here; the manifest's network security
         * config is what scopes that.
         */
        fun defaultClient(): OkHttpClient =
            OkHttpClient.Builder()
                .connectTimeout(10, TimeUnit.SECONDS)
                .readTimeout(0, TimeUnit.SECONDS)
                .writeTimeout(10, TimeUnit.SECONDS)
                .pingInterval(0, TimeUnit.SECONDS)
                .retryOnConnectionFailure(true)
                .build()
    }
}

enum class EnvironmentHttpErrorKind {
    /** No route to the host, DNS failure, timeout: retrying may work. */
    Unreachable,
    /** The credential is wrong, expired, or already spent. */
    Unauthorized,
    /** The host answered, but not as an S5 Code server. */
    Protocol,
    /** The server reported a failure of its own. */
    Server,
}

class EnvironmentHttpError(
    val kind: EnvironmentHttpErrorKind,
    override val message: String,
    override val cause: Throwable? = null,
) : Exception(message, cause) {
    companion object {
        fun unreachable(cause: Throwable) =
            EnvironmentHttpError(
                EnvironmentHttpErrorKind.Unreachable,
                "Could not reach that address. Check the machine is awake and on the same network.",
                cause,
            )

        fun forStatus(status: Int, body: String): EnvironmentHttpError =
            when (status) {
                400, 401, 403 ->
                    EnvironmentHttpError(
                        EnvironmentHttpErrorKind.Unauthorized,
                        // A spent one-time token is by far the most common cause
                        // here, and the fix (mint a new one) is not obvious from
                        // a bare 401.
                        "That pairing token was rejected. It may have already been used — mint a fresh one.",
                    )
                404 ->
                    EnvironmentHttpError(
                        EnvironmentHttpErrorKind.Protocol,
                        "That address answered, but it is not an S5 Code server.",
                    )
                else ->
                    EnvironmentHttpError(
                        EnvironmentHttpErrorKind.Server,
                        "The server returned an error ($status).",
                    )
            }
    }
}

/* ── Wire DTOs ───────────────────────────────────────────────────────── */

@Serializable
data class EnvironmentDescriptorDto(
    val environmentId: String,
    val label: String,
    val platform: EnvironmentPlatformDto = EnvironmentPlatformDto(),
    val serverVersion: String? = null,
    val capabilities: EnvironmentCapabilitiesDto = EnvironmentCapabilitiesDto(),
)

/**
 * `ExecutionEnvironmentPlatform` from `packages/contracts/src/environment.ts`: a
 * struct, not a string. Both members are open-ended (`os` includes "unknown",
 * `arch` includes "other"), so they decode as strings defaulting to empty rather
 * than as enums a future server value would break.
 */
@Serializable
data class EnvironmentPlatformDto(val os: String = "", val arch: String = "") {
    /**
     * What a connection row shows. Derived here so the wire shape has one reader:
     * saved rows keep a display string and nothing else needs to know the
     * descriptor carries two fields.
     */
    val display: String
        get() =
            when {
                os.isBlank() -> arch
                arch.isBlank() -> os
                else -> "$os/$arch"
            }
}

/**
 * Capability flags are all optional and default to false: a server older than
 * this build simply does not advertise them, and treating absence as "off" is
 * what lets one client talk to several server versions. Only the flags this
 * client acts on are listed; the rest are ignored by the lenient decoder.
 */
@Serializable
data class EnvironmentCapabilitiesDto(
    val connectionProbe: Boolean = false,
    val threadSettlement: Boolean = false,
    val threadSnooze: Boolean = false,
    val threadPinning: Boolean = false,
    val threadTitleRegeneration: Boolean = false,
    val pullRequests: Boolean = false,
)

@Serializable
data class AccessTokenDto(
    val access_token: String,
    val token_type: String = "Bearer",
    val expires_in: Long = 0,
    val scope: String = "",
)

@Serializable data class AuthSessionDto(val authenticated: Boolean = false)

@Serializable data class WebSocketTicketDto(val ticket: String)
