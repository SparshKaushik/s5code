package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.data.EnvironmentStore
import club.touchtech.s5code.kotlin.data.SavedEnvironment
import club.touchtech.s5code.kotlin.transport.wire.ServerConfigDto
import club.touchtech.s5code.kotlin.transport.wire.ServerConfigStreamEventDto
import club.touchtech.s5code.kotlin.transport.wire.ServerProvidersUpdatedDto
import kotlin.random.Random
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
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
import kotlinx.coroutines.selects.select
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.KSerializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.OkHttpClient

/** What the UI needs to know about one environment's connection. */
enum class SessionPhase {
    /** No attempt yet, or deliberately stopped. Not emitted for a newly paired session. */
    Idle,
    Connecting,
    Connected,
    /** Lost the connection and waiting out a backoff before retrying. */
    Backoff,
    /**
     * The device has no usable network. The backoff ladder is parked rather than
     * consumed, so a long offline stretch does not strand the session at the top
     * rung when connectivity returns.
     */
    Offline,
    /** The token was rejected. Retrying will not help; re-pairing will. */
    Unauthorized,
}

data class SessionState(
    val phase: SessionPhase = SessionPhase.Idle,
    val attempt: Int = 0,
    val lastError: String? = null,
    val serverVersion: String? = null,
    /** `environment.platform.os` from the config — "darwin", "linux", "windows". */
    val platformOs: String? = null,
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
    /** `thread.pin.reorder` is accepted; without it Move up/down stays hidden. */
    val threadPinReorder: Boolean = false,
    /** `thread.active.reorder` is accepted. */
    val threadActiveReorder: Boolean = false,
    val threadTitleRegeneration: Boolean = false,
    val pullRequests: Boolean = false,
    /** `attachments.createUploadUrl`/`attachments.delete` exist. */
    val attachmentUploads: Boolean = false,
    /** `thread.user-input.respond` accepts `attachmentsByQuestionId`. */
    val questionAttachments: Boolean = false,
    /** Non-image upload ceiling; null means only image uploads are accepted. */
    val fileAttachmentsMaxUploadBytes: Long? = null,
    /** `server.probe` exists; cheaper than a full config fetch for wake checks. */
    val connectionProbe: Boolean = false,
    /** The shell stream can mark the end of an `afterSequence` catch-up replay. */
    val shellResumeCompletionMarker: Boolean = false,
    val threadResumeCompletionMarker: Boolean = false,
    /** Thread reads accept `turnLimit`/`beforeCursor` windows and `page` metadata. */
    val threadSnapshotPagination: Boolean = false,
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
    /**
     * Network reachability. When supplied, the supervisor parks in
     * [SessionPhase.Offline] instead of spending backoff rungs failing sockets
     * that cannot open, and a network restore interrupts the wait.
     */
    private val online: Flow<Boolean>? = null,
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

    /**
     * The credential and origin from the in-flight (or live) authorize step.
     * Snapshot prefetch over HTTP uses them: it must work while the socket is
     * still opening, and must not outlive the attempt that minted them.
     */
    @Volatile private var authorized: EnvironmentAuthorizer.Authorized? = null

    /** `server.probe` support, learned from the config snapshot per connection. */
    @Volatile private var probeSupported = false

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
        authorized = null
        _state.value = SessionState(phase = SessionPhase.Idle)
    }

    /**
     * React to Android returning from the background.
     *
     * Mirrors the RN client's probe-then-decide policy: a blip under
     * [SHORT_WAKE_MS] is checked with a lightweight probe (the socket's own
     * keepalive would catch it seconds later anyway) and only a dead probe —
     * or a long absence — pays for a full authorize→ticket→socket restart.
     * The socket may be half-open without a close frame after a network change,
     * so the probe is capped at [PROBE_TIMEOUT_MS]; a timeout counts as dead.
     */
    fun refreshAfterForeground(backgroundedMillis: Long) {
        if (saved() == null || state.value.phase == SessionPhase.Unauthorized) return
        val live = connection.value
        if (live != null && backgroundedMillis in 0..SHORT_WAKE_MS) {
            scope.launch {
                val method =
                    if (probeSupported) WsMethods.ServerProbe else WsMethods.ServerGetConfig
                val answered =
                    runCatching {
                            withTimeout(PROBE_TIMEOUT_MS) {
                                live.request(method, JsonObject(emptyMap()))
                            }
                        }
                        .isSuccess
                if (!answered) retryNow()
            }
            return
        }
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
        authorized = null
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

            // Offline parking: spend no rung while the radio cannot reach
            // anything, and leave the loop as soon as it can again.
            val connectivity = online
            if (connectivity != null && !connectivity.first()) {
                _state.value = _state.value.copy(phase = SessionPhase.Offline, lastError = null)
                connectivity.first { it }
                _state.value = _state.value.copy(phase = SessionPhase.Connecting)
            } else {
                _state.value = _state.value.copy(phase = SessionPhase.Connecting, attempt = attempt)
            }

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

            // connect() returns the millis the session stayed connected, so a
            // socket that lived past the stability threshold reset the ladder —
            // an hours-old connection dropping must not inherit a deep rung.
            val connectedMillis = outcome.getOrNull()
            attempt = if (connectedMillis != null && connectedMillis >= STABLE_CONNECTION_MS) 0
            else attempt + 1
            _state.value =
                _state.value.copy(
                    phase = SessionPhase.Backoff,
                    attempt = attempt,
                    lastError = failure?.message ?: "The connection dropped.",
                )
            backoff(attempt)
        }
    }

    /**
     * Waits out one rung, but wakes early when connectivity returns. A screen
     * full of "retrying in 27s" while the phone is already back on Wi-Fi reads
     * as dead; the wait is interruptible for exactly that reason.
     */
    private suspend fun backoff(attempt: Int) {
        val connectivity = online ?: run {
            delay(backoffMillis(attempt))
            return
        }
        var remaining = backoffMillis(attempt)
        while (remaining > 0) {
            val started = System.currentTimeMillis()
            val wentOffline =
                withTimeoutOrNull(remaining) {
                    connectivity.first { !it }
                    true
                } == true
            val waited = System.currentTimeMillis() - started
            if (wentOffline) {
                _state.value = _state.value.copy(phase = SessionPhase.Offline)
                connectivity.first { it }
                _state.value = _state.value.copy(phase = SessionPhase.Backoff, attempt = attempt)
                // Back online: retry now instead of finishing the rung.
                return
            }
            if (waited >= remaining) return
            remaining -= waited
        }
    }

    /**
     * Authorizes, opens a socket, and keeps the server-config stream open for
     * the session's life. Returns the millis the connection stayed up; throws
     * when the attempt never got there.
     *
     * Config is a stream, not a unary fetch (`subscribeServerConfig`, matching
     * `packages/client-runtime/src/rpc/session.ts`): the server pushes provider
     * status changes and settings updates on it, and — load-bearing — the end
     * of that stream means the session is half-dead even while the socket looks
     * open, so it closes the connection to force a reconnect. Authorization
     * happens per attempt rather than once per session: a relay-managed
     * environment's credential expires in minutes, so reusing the one from the
     * first attempt would make every reconnect fail.
     */
    @OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)
    private suspend fun connect(environment: SavedEnvironment): Long {
        val authorized = authorizer.authorize(environment)
        this.authorized = authorized
        val socketUrl =
            http.resolveSocketUrl(
                authorized.httpBaseUrl,
                authorized.wsBaseUrl,
                authorized.credential,
                authorized.connectionMethod,
            )
        val opened = RpcConnection.open(client, socketUrl, scope)
        httpBaseUrl = authorized.httpBaseUrl
        val firstConfig = CompletableDeferred<ServerConfigDto>()
        val configEnded = CompletableDeferred<RpcTransportClosed>()
        val configJob =
            scope.launch {
                try {
                    opened
                        .stream(WsMethods.SubscribeServerConfig, JsonObject(emptyMap()))
                        .collect { element ->
                            val event =
                                TransportJson.decodeFromJsonElement(
                                    ServerConfigStreamEventDto.serializer(),
                                    element,
                                )
                            when {
                                event.type == "snapshot" && event.config != null -> {
                                    firstConfig.complete(event.config)
                                }
                                event.type == "providerStatuses" -> {
                                    event.payload?.providers?.let { _providers.value = it }
                                }
                                else -> Unit
                            }
                        }
                    configEnded.complete(
                        RpcTransportClosed("The server ended the config stream.")
                    )
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (error: Throwable) {
                    firstConfig.completeExceptionally(error)
                    configEnded.complete(
                        error as? RpcTransportClosed
                            ?: RpcTransportClosed(error.message ?: "The config stream failed.")
                    )
                }
            }
        try {
            val config =
                withTimeout(CONFIG_WAIT_MS) { firstConfig.await() }
            val descriptor = config.environment
            _label.value = descriptor.label.ifBlank { environment.label }
            _state.value =
                SessionState(
                    phase = SessionPhase.Connected,
                    attempt = 0,
                    lastError = null,
                    serverVersion = descriptor.serverVersion.ifBlank { null },
                    platformOs = descriptor.platform.os.ifBlank { null },
                    capabilities =
                        ServerCapabilities(
                            threadSettlement = descriptor.capabilities.threadSettlement,
                            threadSnooze = descriptor.capabilities.threadSnooze,
                            threadPinning = descriptor.capabilities.threadPinning,
                            threadPinReorder = descriptor.capabilities.threadPinReorder,
                            threadActiveReorder = descriptor.capabilities.threadActiveReorder,
                            threadTitleRegeneration = descriptor.capabilities.threadTitleRegeneration,
                            pullRequests = descriptor.capabilities.pullRequests,
                            attachmentUploads = descriptor.capabilities.attachmentUploads,
                            questionAttachments = descriptor.capabilities.questionAttachments,
                            fileAttachmentsMaxUploadBytes =
                                descriptor.capabilities.fileAttachments?.maxUploadBytes,
                            connectionProbe = descriptor.capabilities.connectionProbe,
                            shellResumeCompletionMarker = config.shellResumeCompletionMarker,
                            threadResumeCompletionMarker = config.threadResumeCompletionMarker,
                            threadSnapshotPagination = config.threadSnapshotPagination,
                        ),
                )
            probeSupported = descriptor.capabilities.connectionProbe
            connection.value = opened
            _providers.value = config.providers
            val connectedAt = System.currentTimeMillis()
            val reason =
                select {
                    opened.closed.onAwait { it }
                    configEnded.onAwait { it }
                }
            _state.value = _state.value.copy(phase = SessionPhase.Backoff, lastError = reason.message)
            return System.currentTimeMillis() - connectedAt
        } finally {
            configJob.cancel()
            connection.value = null
            httpBaseUrl = null
            this.authorized = null
            opened.close()
        }
    }

    private val _providers = MutableStateFlow<List<club.touchtech.s5code.kotlin.transport.wire.ServerProviderDto>>(emptyList())

    /** The environment's configured provider instances, empty while disconnected. */
    val providers: StateFlow<List<club.touchtech.s5code.kotlin.transport.wire.ServerProviderDto>> =
        _providers.asStateFlow()

    /**
     * Re-runs provider discovery on the server, including a fresh model list.
     *
     * The answer replaces [providers] in place — the picker reads this flow, so
     * a refresh is a write to the source rather than a value handed back.
     * `refreshModels` marks the request as user-driven, which is what lets the
     * server open agent sessions for discovery instead of only rereading config.
     */
    suspend fun refreshProviders() {
        val updated =
            request(
                WsMethods.ServerRefreshProviders,
                buildJsonObject { put("refreshModels", true) },
                ServerProvidersUpdatedDto.serializer(),
            )
        _providers.value = updated.providers
    }

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
    ): Flow<T> = subscribe(method, { payload }, serializer)

    /**
     * Variant whose request body is built per connection. Resume subscriptions
     * need this: the `afterSequence` cursor is read when each socket opens, so a
     * fresh snapshot is only skipped when the local one is genuinely current.
     * [prefetch], when supplied, runs once per connection before the stream
     * opens — the HTTP snapshot fast-path from `state/shell.ts` /
     * `state/threads.ts` in `packages/client-runtime`, which lets a reconnect
     * resume over HTTP before the socket finishes.
     */
    @OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)
    fun <T> subscribe(
        method: String,
        payload: suspend (RpcConnection) -> JsonElement,
        serializer: KSerializer<T>,
        prefetch: (suspend (EnvironmentSession) -> Unit)? = null,
    ): Flow<T> =
        connection.transformLatest { live ->
            if (live == null) return@transformLatest
            prefetch?.invoke(this@EnvironmentSession)
            live.stream(method, payload(live)).collect { element ->
                emit(TransportJson.decodeFromJsonElement(serializer, element))
            }
        }

    /**
     * Authenticated GET against the environment's HTTP API, using the credential
     * the current (or most recent) connection attempt minted. This is the
     * snapshot fast-path (`/api/orchestration/shell`, `/api/orchestration/
     * threads/:id`); there is no response model on this side, callers decode.
     */
    suspend fun <T> getJson(path: String, serializer: KSerializer<T>): T {
        val current =
            authorized
                ?: throw EnvironmentHttpError(
                    EnvironmentHttpErrorKind.Unreachable,
                    "Not connected.",
                )
        return http.getAuthenticated(current.httpBaseUrl, path, current.credential, serializer)
    }

    val connected: Flow<Boolean> = connection.map { it != null }

    private suspend fun awaitConnection(): RpcConnection {
        connection.value?.let { return it }
        start()
        // A terminal phase means no connection is coming; waiting forever would
        // hang the outbox drain and any direct write path behind a dead token.
        val phase = state.value.phase
        if (phase == SessionPhase.Unauthorized) {
            throw EnvironmentHttpError(
                EnvironmentHttpErrorKind.Unauthorized,
                state.value.lastError ?: "This environment needs to be paired again.",
            )
        }
        return withTimeout(AWAIT_CONNECTION_MS) { connection.first { it != null }!! }
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

        /** A connection this stable resets the backoff ladder, matching RN's 30s. */
        const val STABLE_CONNECTION_MS = 30_000L

        /** Foreground wakes shorter than this probe instead of restarting. */
        const val SHORT_WAKE_MS = 10_000L

        /** Mobile wake probe, matching RN's 3s `application-active-probe`. */
        const val PROBE_TIMEOUT_MS = 3_000L

        /** How long a write waits for a socket before failing instead of hanging. */
        const val AWAIT_CONNECTION_MS = 15_000L

        /** The first config frame must arrive promptly or the attempt is dead. */
        const val CONFIG_WAIT_MS = 15_000L
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
