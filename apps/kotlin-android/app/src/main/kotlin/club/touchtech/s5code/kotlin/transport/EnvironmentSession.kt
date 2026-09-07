package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.data.EnvironmentStore
import club.touchtech.s5code.kotlin.data.SavedEnvironment
import club.touchtech.s5code.kotlin.transport.wire.ServerConfigDto
import kotlin.random.Random
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.transformLatest
import kotlinx.coroutines.launch
import kotlinx.serialization.KSerializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import okhttp3.OkHttpClient

/** What the UI needs to know about one environment's connection. */
enum class SessionPhase {
    /** No attempt yet, or deliberately stopped. Not emitted for a newly paired session. */
    Idle,
    Connecting,
    Connected,
    /** Lost the connection and waiting out a backoff before retrying. */
    Backoff,
    /** The token was rejected. Retrying will not help; re-pairing will. */
    Unauthorized,
}

data class SessionState(
    val phase: SessionPhase = SessionPhase.Idle,
    val attempt: Int = 0,
    val lastError: String? = null,
    val serverVersion: String? = null,
    val capabilities: ServerCapabilities = ServerCapabilities(),
)

/**
 * Server-advertised capabilities, defaulted off. A command the server does not
 * understand comes back as a protocol defect that kills the socket, so these
 * gate the write paths rather than merely hiding buttons.
 */
data class ServerCapabilities(
    val threadSettlement: Boolean = false,
    val threadSnooze: Boolean = false,
    val threadPinning: Boolean = false,
    val threadTitleRegeneration: Boolean = false,
    val pullRequests: Boolean = false,
)

/**
 * One supervised connection to one environment.
 *
 * The session owns reconnection, because everything above it is a projection
 * that must not care whether the socket is on its first or fiftieth attempt.
 * Two properties are what make that work:
 *
 * - **Subscriptions restart themselves.** [subscribe] re-issues its RPC on every
 *   new connection, so a caller collects one flow for the lifetime of a screen
 *   and receives a fresh snapshot after each reconnect instead of having to
 *   re-subscribe.
 * - **Unauthorized is terminal.** A rejected token cannot be fixed by waiting,
 *   so the loop stops and the UI is told to re-pair. Retrying a 401 forever is
 *   how a client turns one expired credential into a battery complaint.
 */
class EnvironmentSession(
    val environmentId: String,
    initialLabel: String,
    private val scope: CoroutineScope,
    private val http: EnvironmentHttp,
    private val client: OkHttpClient,
    private val authorizer: EnvironmentAuthorizer,
    private val saved: () -> SavedEnvironment?,
    /** A paired environment is connecting from its first visible frame. */
    initialPhase: SessionPhase = SessionPhase.Idle,
) {
    private val _state = MutableStateFlow(SessionState(phase = initialPhase))
    val state: StateFlow<SessionState> = _state.asStateFlow()

    private val _label = MutableStateFlow(initialLabel)
    val label: StateFlow<String> = _label.asStateFlow()

    /**
     * The HTTP origin the live connection is using, or null while disconnected.
     * Relay-managed endpoints are resolved per attempt, so anything building an
     * absolute URL (a signed asset, say) has to read it from here rather than from
     * the saved row.
     */
    @Volatile
    var httpBaseUrl: String? = null
        private set

    /**
     * The live connection, or null while disconnected. Emitting null explicitly
     * (rather than simply not emitting) is what lets subscriptions clear their
     * derived state when the socket drops.
     */
    private val connection = MutableStateFlow<RpcConnection?>(null)

    private var supervisor: Job? = null

    fun start() {
        if (supervisor?.isActive == true) return
        supervisor = scope.launch { supervise() }
    }

    fun stop() {
        supervisor?.cancel()
        supervisor = null
        connection.value?.close()
        connection.value = null
        _state.value = SessionState(phase = SessionPhase.Idle)
    }

    /**
     * Forces a fresh socket attempt after Android returns from the background.
     * Mobile networks can leave a WebSocket half-open without delivering a close
     * callback; waiting for the next keepalive makes a foreground app look dead.
     * A restart is safe here because [subscribe] follows the connection StateFlow
     * and reissues every live subscription on the replacement socket.
     */
    fun refreshAfterForeground() {
        if (saved() == null || state.value.phase == SessionPhase.Unauthorized) return
        retryNow()
    }

    /** Retries immediately, for the "Try again" affordance on a failed row. */
    fun retryNow() {
        restart()
    }

    private fun restart() {
        val previous = supervisor
        previous?.cancel()
        connection.value?.close()
        _state.value = _state.value.copy(phase = SessionPhase.Connecting)
        supervisor =
            scope.launch {
                // connect() clears the old StateFlow entry in finally. Never let
                // that stale cleanup race a replacement connection and erase it.
                previous?.join()
                supervise()
            }
    }

    private suspend fun supervise() {
        var attempt = 0
        while (true) {
            val environment = saved()
            if (environment == null) {
                _state.value = SessionState(phase = SessionPhase.Idle)
                return
            }
            _label.value = environment.label
            _state.value = _state.value.copy(phase = SessionPhase.Connecting, attempt = attempt)

            val outcome = runCatching { connect(environment) }
            val failure = outcome.exceptionOrNull()
            if (failure is CancellationException) throw failure

            if (failure is EnvironmentHttpError &&
                failure.kind == EnvironmentHttpErrorKind.Unauthorized
            ) {
                _state.value =
                    _state.value.copy(phase = SessionPhase.Unauthorized, lastError = failure.message)
                return
            }

            if (failure != null) {
                attempt += 1
                _state.value =
                    _state.value.copy(
                        phase = SessionPhase.Backoff,
                        attempt = attempt,
                        lastError = failure.message ?: "The connection failed.",
                    )
                delay(backoffMillis(attempt))
                continue
            }

            // connect() returns only once the socket has died.
            attempt += 1
            _state.value = _state.value.copy(phase = SessionPhase.Backoff, attempt = attempt)
            delay(backoffMillis(attempt))
        }
    }

    /**
     * Authorizes, opens a socket, reads the config, and suspends until the socket
     * closes. Authorization happens per attempt rather than once per session: a
     * relay-managed environment's credential expires in minutes, so reusing the
     * one from the first attempt would make every reconnect fail.
     */
    private suspend fun connect(environment: SavedEnvironment) {
        val authorized = authorizer.authorize(environment)
        val socketUrl =
            http.resolveSocketUrl(
                authorized.httpBaseUrl,
                authorized.wsBaseUrl,
                authorized.credential,
            )
        val opened = RpcConnection.open(client, socketUrl, scope)
        httpBaseUrl = authorized.httpBaseUrl
        try {
            val config =
                opened.call(WsMethods.ServerGetConfig, JsonObject(emptyMap()), ServerConfigDto.serializer())
            val descriptor = config.environment
            _label.value = descriptor.label.ifBlank { environment.label }
            _state.value =
                SessionState(
                    phase = SessionPhase.Connected,
                    attempt = 0,
                    lastError = null,
                    serverVersion = descriptor.serverVersion.ifBlank { null },
                    capabilities =
                        ServerCapabilities(
                            threadSettlement = descriptor.capabilities.threadSettlement,
                            threadSnooze = descriptor.capabilities.threadSnooze,
                            threadPinning = descriptor.capabilities.threadPinning,
                            threadTitleRegeneration = descriptor.capabilities.threadTitleRegeneration,
                            pullRequests = descriptor.capabilities.pullRequests,
                        ),
                )
            connection.value = opened
            _providers.value = config.providers
            val reason = opened.closed.await()
            _state.value = _state.value.copy(phase = SessionPhase.Backoff, lastError = reason.message)
        } finally {
            connection.value = null
            httpBaseUrl = null
            opened.close()
        }
    }

    private val _providers = MutableStateFlow<List<club.touchtech.s5code.kotlin.transport.wire.ServerProviderDto>>(emptyList())

    /** The environment's configured provider instances, empty while disconnected. */
    val providers: StateFlow<List<club.touchtech.s5code.kotlin.transport.wire.ServerProviderDto>> =
        _providers.asStateFlow()

    /**
     * Issues a unary RPC on the current connection, waiting briefly for one if
     * the socket is mid-reconnect. Waiting rather than failing immediately is
     * what keeps a tap that lands during a reconnect from surfacing an error the
     * user cannot act on.
     */
    suspend fun <T> request(
        method: String,
        payload: JsonElement,
        serializer: KSerializer<T>,
    ): T {
        val live = awaitConnection()
        return live.call(method, payload, serializer)
    }

    /**
     * Issues an RPC whose contract declares no success value.
     *
     * Effect encodes a `Schema.Void` success by omitting `value` from the exit, so
     * decoding one of these into any struct fails on a response that actually
     * succeeded. `terminal.write` and `terminal.resize` are both shaped this way.
     */
    suspend fun execute(method: String, payload: JsonElement) {
        awaitConnection().request(method, payload)
    }

    /**
     * A server-push subscription that survives reconnects. Each new connection
     * re-issues the RPC, so the collector sees a fresh snapshot frame and can
     * rebuild from it.
     */
    @OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)
    fun <T> subscribe(
        method: String,
        payload: JsonElement,
        serializer: KSerializer<T>,
    ): Flow<T> =
        connection.transformLatest { live ->
            if (live == null) return@transformLatest
            live.stream(method, payload).collect { element ->
                emit(TransportJson.decodeFromJsonElement(serializer, element))
            }
        }

    val connected: Flow<Boolean> = connection.map { it != null }

    private suspend fun awaitConnection(): RpcConnection {
        connection.value?.let { return it }
        start()
        return connection.first { it != null }!!
    }

    private companion object {
        /**
         * Exponential with jitter, capped at 30s. The jitter matters with several
         * environments saved: without it a dropped Wi-Fi link makes every session
         * retry in lockstep, and they all fail together on the same congested
         * radio.
         */
        fun backoffMillis(attempt: Int): Long {
            val base = (500L shl minOf(attempt, 6)).coerceAtMost(30_000L)
            return base + Random.nextLong(0, base / 2 + 1)
        }
    }
}

/** Lenient decoder shared by every wire mapping. */
internal val TransportJson: Json = Json {
    ignoreUnknownKeys = true
    explicitNulls = false
    coerceInputValues = true
    isLenient = true
}

internal suspend fun <T> RpcConnection.call(
    method: String,
    payload: JsonElement,
    serializer: KSerializer<T>,
): T = TransportJson.decodeFromJsonElement(serializer, request(method, payload))
