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
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.merge
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
import kotlinx.serialization.json.putJsonArray
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
    /**
     * `resolveEnvironmentMachineKind`: the user's `environmentIcon` setting, or
     * `platform.machine` when unset. One of the ENVIRONMENT_MACHINE_KINDS
     * literals, or null on servers that publish neither.
     */
    val machineKind: String? = null,
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
 * - **Unauthorized parks rather than loops.** A rejected token cannot be fixed
 *   by waiting, so the loop stops — but the phase still answers a manual retry
 *   or a foreground wake, matching RN's `blocked`: a relay credential may have
 *   been refreshed while the app was away, and one retry is cheap.
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
    /**
     * The `clientId` this device reports in `server.reportClientActivity`
     * (`mobile-<deviceId>` in RN). Null disables reporting.
     */
    private val activityClientId: (() -> String)? = null,
    /** Whether the app is in the foreground, for activity reports. */
    private val appIsForegrounded: (() -> Boolean)? = null,
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

    /**
     * RN's `foregroundResubscriptions`: a foreground wake re-runs live
     * subscriptions on the same socket, healing a stream the server ended
     * while the app was away without needing a reconnect.
     */
    private val resubscribeSignals = MutableSharedFlow<Unit>(extraBufferCapacity = 1)

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
        if (saved() == null) return
        // Re-arm live subscriptions on the surviving socket (RN's
        // `foregroundResubscriptions`): a stream the server ended while the
        // app was away does not need a reconnect to heal.
        resubscribeSignals.tryEmit(Unit)
        // Unauthorized is parked, not dead: RN's `blocked` phase retries when
        // the app regains foreground, because the account behind a relay token
        // may have been refreshed while the app was away. A still-dead token
        // just lands back on Unauthorized — one attempt, not a loop.
        if (state.value.phase == SessionPhase.Unauthorized) {
            retryNow()
            return
        }
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
                if (answered) {
                    // The socket lived; tell the server the app is visible
                    // again so the background-activity lease does not lapse
                    // waiting for the next tick.
                    reportActivity(live)
                } else {
                    retryNow()
                }
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
                    // RN renders the relay's trace id beside the status so a
                    // reported failure can be found in relay logs.
                    lastError =
                        failure?.let {
                            it.message +
                                ((it as? club.touchtech.s5code.kotlin.cloud.RelayError)?.traceId
                                    ?.let { trace -> " Trace ID: $trace" } ?: "")
                        } ?: "The connection dropped.",
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
        // `attempt` is the failure count (1-based); the rung index is 0-based,
        // so the first retry waits rung 0 like RN's retryDelayMs(failureCount-1).
        val connectivity = online ?: run {
            delay(backoffMillis(attempt - 1))
            return
        }
        var remaining = backoffMillis(attempt - 1)
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
    /** The pieces of a landed attempt: socket, first config, and the config stream job. */
    private class Established(
        val opened: RpcConnection,
        val config: ServerConfigDto,
        val configJob: Job,
        val configEnded: CompletableDeferred<RpcTransportClosed>,
    )

    private suspend fun connect(environment: SavedEnvironment): Long {
        // One bound on authorize→open→first-config, matching RN's
        // CONNECTION_ESTABLISHMENT_TIMEOUT — the per-step timeouts inside stay
        // as backstops. A connectivity drop mid-attempt aborts it too, like
        // RN's offline watchdog, rather than letting a dead attempt run out
        // the clock.
        val established =
            coroutineScope {
                val attempt =
                    async { withTimeout(ESTABLISHMENT_TIMEOUT_MS) { establish(environment) } }
                val wentOffline =
                    online?.let { connectivity ->
                        async {
                            connectivity.first { !it }
                            RpcTransportClosed("The network went offline.")
                        }
                    }
                try {
                    select<Established> {
                        attempt.onAwait { it }
                        wentOffline?.onAwait { throw it }
                    }
                } finally {
                    wentOffline?.cancel()
                }
            }
        val opened = established.opened
        val config = established.config
        val configJob = established.configJob
        val configEnded = established.configEnded
        try {
            val descriptor = config.environment
            _label.value = descriptor.label.ifBlank { environment.label }
            _state.value =
                SessionState(
                    phase = SessionPhase.Connected,
                    attempt = 0,
                    lastError = null,
                    serverVersion = descriptor.serverVersion.ifBlank { null },
                    platformOs = descriptor.platform.os.ifBlank { null },
                    machineKind =
                        config.settings.environmentIcon ?: descriptor.platform.machine,
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
            val activityJob = startActivityReporting(opened)
            val connectedAt = System.currentTimeMillis()
            val reason =
                coroutineScope {
                    // RN drops the lease the moment connectivity reports
                    // offline rather than waiting for the dead socket to time
                    // out; parking beats sitting on a dead connection for the
                    // keepalive window.
                    val wentOffline =
                        online?.let { connectivity ->
                            async {
                                connectivity.first { !it }
                                RpcTransportClosed("The network went offline.")
                            }
                        }
                    try {
                        select {
                            opened.closed.onAwait { it }
                            configEnded.onAwait { it }
                            wentOffline?.onAwait { it }
                        }
                    } finally {
                        wentOffline?.cancel()
                    }
                }
            activityJob.cancel()
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

    /**
     * Authorize, open the socket, and wait for the first server-config frame.
     * Anything that throws before this returns has produced no visible session.
     */
    private suspend fun establish(environment: SavedEnvironment): Established {
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
        val config =
            try {
                withTimeout(CONFIG_WAIT_MS) { firstConfig.await() }
            } catch (failure: Throwable) {
                // A timed-out, cancelled, or offline-aborted attempt must not
                // leave a socket and config stream running detached.
                configJob.cancel()
                opened.close()
                this.authorized = null
                httpBaseUrl = null
                throw failure
            }
        return Established(
            opened = opened,
            config = config,
            configJob = configJob,
            configEnded = configEnded,
        )
    }

    /**
     * `server.reportClientActivity` on a cadence, matching
     * `mobileBackgroundActivityReporterLayer` in the RN client: 25s between
     * reports, 45s TTL, the baseline provider-status scope. The lease is what
     * keeps this client's work counted on the server's background-activity
     * snapshot while the app sits in the shade.
     */
    private fun startActivityReporting(opened: RpcConnection): Job =
        scope.launch {
            while (true) {
                reportActivity(opened)
                delay(ACTIVITY_REPORT_INTERVAL_MS)
            }
        }

    private suspend fun reportActivity(opened: RpcConnection) {
        val id = activityClientId?.invoke() ?: return
        val active = appIsForegrounded?.invoke() ?: true
        runCatching {
            opened.request(
                WsMethods.ServerReportClientActivity,
                buildJsonObject {
                    put("environmentId", environmentId)
                    put("clientId", id)
                    put("clientKind", "mobile")
                    put("visible", active)
                    put("focused", active)
                    put("recentlyInteracted", active)
                    put("appState", if (active) "active" else "background")
                    // BackgroundScope is a tagged struct, not a bare string.
                    putJsonArray("scopes") {
                        add(buildJsonObject { put("type", "provider-status") })
                    }
                    put("ttlMs", ACTIVITY_LEASE_TTL_MS)
                    put(
                        "observedAt",
                        java.time.Instant.ofEpochMilli(System.currentTimeMillis()).toString(),
                    )
                },
            )
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
    ): Flow<T> {
        // Prefetch once per connection instance, not per subscription: RN's
        // `hasAuthoritativeSnapshot` keeps a foreground resubscribe on the
        // in-memory cursor rather than re-fetching the whole snapshot.
        var prefetchedFor: RpcConnection? = null
        return merge(
                connection,
                resubscribeSignals.map { connection.value },
            )
            .transformLatest { live ->
                if (live == null) return@transformLatest
                if (prefetchedFor !== live) {
                    prefetchedFor = live
                    prefetch?.invoke(this@EnvironmentSession)
                }
                // RN's subscribeDynamic is durable on the same session: an RPC
                // failure (the server answered Exit(Failure), e.g. a not-yet-
                // materialized thread) resubscribes after 250ms rather than
                // killing the collector until the next reconnect. Transport
                // death and clean ends wait for the next connection, which
                // transformLatest delivers for free.
                while (true) {
                    try {
                        live.stream(method, payload(live)).collect { element ->
                            emit(TransportJson.decodeFromJsonElement(serializer, element))
                        }
                        return@transformLatest
                    } catch (expected: RpcFailure) {
                        delay(RESUBSCRIBE_DELAY_MS)
                    }
                }
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

    internal companion object {
        /**
         * RN's rungs (`RETRY_DELAYS_MS` in `connection/supervisor.ts`) plus
         * jitter, capped at the last rung. The jitter matters with several
         * environments saved: without it a dropped Wi-Fi link makes every
         * session retry in lockstep, and they all fail together on the same
         * congested radio.
         */
        private val RETRY_RUNGS_MS = longArrayOf(3_000L, 4_000L, 8_000L, 16_000L)

        /** Rung index 0..3 → base delay; jitter adds up to half on top. */
        fun backoffMillis(rung: Int): Long {
            val base = RETRY_RUNGS_MS[minOf(rung.coerceAtLeast(0), RETRY_RUNGS_MS.size - 1)]
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

        /** Total bound on authorize→open→first-config, matching RN's establishment timeout. */
        const val ESTABLISHMENT_TIMEOUT_MS = 15_000L

        /**
         * Client-activity reporting cadence and lease, matching RN's
         * `REPORT_INTERVAL_MS`/`LEASE_TTL_MS` in `connection/background-activity.ts`.
         */
        const val ACTIVITY_REPORT_INTERVAL_MS = 25_000L
        const val ACTIVITY_LEASE_TTL_MS = 45_000L

        /** Between a failed stream and the resubscribe, matching RN's 250ms. */
        const val RESUBSCRIBE_DELAY_MS = 250L
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
