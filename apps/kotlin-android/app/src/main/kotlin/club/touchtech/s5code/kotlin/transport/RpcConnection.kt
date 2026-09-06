package club.touchtech.s5code.kotlin.transport

import java.util.concurrent.atomic.AtomicLong
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlinx.coroutines.CancellableContinuation
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.json.JsonElement
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener

/** A contract error, a defect, or a dead transport. */
class RpcFailure(
    val kind: RpcFailureKind,
    val tag: String?,
    override val message: String,
) : Exception(message)

/** The socket went away. Callers treat this as transient and reconnect. */
class RpcTransportClosed(override val message: String) : Exception(message)

/**
 * One live RPC session over one WebSocket.
 *
 * The Effect RPC protocol is request/response plus server-push streams
 * multiplexed by request id over a single socket. Two details are load-bearing:
 *
 * - **Acks are mandatory.** The server closes a latch after each stream chunk
 *   and reopens it on `Ack`, so an unacknowledged chunk stalls that stream
 *   forever while the socket looks healthy.
 * - **Ping is client-driven.** The server only answers `Ping` with `Pong`; it
 *   never initiates. A half-open socket is otherwise indistinguishable from an
 *   idle one, which on mobile happens every time a NAT drops a mapping.
 */
class RpcConnection
private constructor(
    private val scope: CoroutineScope,
    private val onClosed: (RpcTransportClosed) -> Unit,
) {
    private val requestIds = AtomicLong(1)
    private val lock = Mutex()
    private val pendingRequests = mutableMapOf<Long, CancellableContinuation<JsonElement>>()
    private val pendingStreams = mutableMapOf<Long, Channel<JsonElement>>()

    private lateinit var socket: WebSocket

    @Volatile private var failure: RpcTransportClosed? = null

    @Volatile private var pongSeen = true

    private val opened = CompletableDeferred<Unit>()

    /** Completes when the socket dies, for whatever reason. */
    val closed = CompletableDeferred<RpcTransportClosed>()

    /**
     * A unary call. Cancelling the caller sends `Interrupt` so the server drops
     * the work instead of finishing a request nobody is waiting for.
     */
    suspend fun request(tag: String, payload: JsonElement): JsonElement {
        val id = requestIds.getAndIncrement()
        return suspendCancellableCoroutine { continuation ->
            scope.launch {
                val closedNow = failure
                if (closedNow != null) {
                    continuation.resumeWithException(closedNow)
                    return@launch
                }
                lock.withLock { pendingRequests[id] = continuation }
                continuation.invokeOnCancellation {
                    scope.launch {
                        lock.withLock { pendingRequests.remove(id) }
                        send(RpcFromClient.Interrupt(id))
                    }
                }
                send(RpcFromClient.Request(id, tag, payload))
            }
        }
    }

    /**
     * A server-push stream. The flow ends when the server sends `Exit(Success)`
     * and fails on anything else, including the socket dying, so a collector
     * always learns why it stopped.
     */
    fun stream(tag: String, payload: JsonElement): Flow<JsonElement> = callbackFlow {
        val id = requestIds.getAndIncrement()
        // Rendezvous rather than buffered: the ack is what asks for the next
        // chunk, so buffering here would let the client fall behind the server
        // silently and grow unboundedly on a slow consumer.
        val chunks = Channel<JsonElement>(capacity = Channel.RENDEZVOUS)
        val closedNow = failure
        if (closedNow != null) {
            close(closedNow)
            return@callbackFlow
        }
        lock.withLock { pendingStreams[id] = chunks }
        send(RpcFromClient.Request(id, tag, payload))

        val pump = launch {
            for (chunk in chunks) {
                send(chunk)
                // Acked after the collector took the value, which is what makes
                // backpressure reach the server rather than stopping at us.
                this@RpcConnection.send(RpcFromClient.Ack(id))
            }
        }

        awaitClose {
            pump.cancel()
            scope.launch {
                lock.withLock { pendingStreams.remove(id) }?.close()
                this@RpcConnection.send(RpcFromClient.Interrupt(id))
            }
        }
    }

    private fun send(message: RpcFromClient) {
        if (failure != null) return
        runCatching { socket.send(RpcJson.encodeToString(JsonElement.serializer(), message.toJson())) }
    }

    private fun onFrame(text: String) {
        val envelopes = runCatching { parseRpcFrame(text) }.getOrElse { cause ->
            fail(RpcTransportClosed("The server sent a frame this build cannot parse: ${cause.message}"))
            return
        }
        envelopes.forEach(::dispatch)
    }

    private fun dispatch(envelope: RpcFromServer) {
        when (envelope) {
            RpcFromServer.Pong -> pongSeen = true
            is RpcFromServer.Chunk ->
                scope.launch {
                    val channel = lock.withLock { pendingStreams[envelope.requestId] } ?: return@launch
                    envelope.values.forEach { channel.send(it) }
                }
            is RpcFromServer.Exit ->
                scope.launch {
                    val continuation = lock.withLock { pendingRequests.remove(envelope.requestId) }
                    if (continuation != null) {
                        when (val outcome = envelope.outcome) {
                            is RpcOutcome.Success -> continuation.resume(outcome.value)
                            is RpcOutcome.Failure ->
                                continuation.resumeWithException(
                                    RpcFailure(outcome.kind, outcome.tag, outcome.message)
                                )
                        }
                        return@launch
                    }
                    val channel = lock.withLock { pendingStreams.remove(envelope.requestId) }
                    when (val outcome = envelope.outcome) {
                        is RpcOutcome.Success -> channel?.close()
                        is RpcOutcome.Failure ->
                            channel?.close(RpcFailure(outcome.kind, outcome.tag, outcome.message))
                    }
                }
            is RpcFromServer.Defect ->
                fail(RpcTransportClosed("The server reported an internal error and closed the session."))
            is RpcFromServer.ProtocolError -> fail(RpcTransportClosed(envelope.message))
        }
    }

    /**
     * Terminates every in-flight call with the same reason. Idempotent: a socket
     * failure and the close callback both land here.
     */
    private fun fail(reason: RpcTransportClosed) {
        if (failure != null) return
        failure = reason
        closed.complete(reason)
        opened.complete(Unit)
        scope.launch {
            val requests: List<CancellableContinuation<JsonElement>>
            val streams: List<Channel<JsonElement>>
            lock.withLock {
                requests = pendingRequests.values.toList()
                streams = pendingStreams.values.toList()
                pendingRequests.clear()
                pendingStreams.clear()
            }
            requests.forEach { it.resumeWithException(reason) }
            streams.forEach { it.close(reason) }
        }
        onClosed(reason)
        runCatching { socket.close(NORMAL_CLOSE, null) }
    }

    fun close() {
        fail(RpcTransportClosed("The client closed the session."))
    }

    private fun startKeepalive() {
        scope.launch {
            while (scope.isActive && failure == null) {
                delay(KEEPALIVE_INTERVAL_MS)
                if (failure != null) return@launch
                if (!pongSeen) {
                    fail(RpcTransportClosed("The server stopped answering keepalives."))
                    return@launch
                }
                pongSeen = false
                send(RpcFromClient.Ping)
            }
        }
    }

    companion object {
        private const val NORMAL_CLOSE = 1000
        private const val KEEPALIVE_INTERVAL_MS = 15_000L
        private const val OPEN_TIMEOUT_MS = 15_000L

        /**
         * Opens the socket and waits for the handshake. Returns a connection
         * whose first RPC can be issued immediately; a socket that never opens
         * surfaces as [RpcTransportClosed] rather than a call that hangs.
         */
        suspend fun open(
            client: OkHttpClient,
            url: String,
            scope: CoroutineScope,
            onClosed: (RpcTransportClosed) -> Unit = {},
        ): RpcConnection {
            val connection = RpcConnection(scope, onClosed)
            val listener =
                object : WebSocketListener() {
                    override fun onOpen(webSocket: WebSocket, response: Response) {
                        connection.opened.complete(Unit)
                        connection.startKeepalive()
                    }

                    override fun onMessage(webSocket: WebSocket, text: String) =
                        connection.onFrame(text)

                    override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                        val status = response?.code
                        connection.fail(
                            RpcTransportClosed(
                                when {
                                    status == 401 || status == 403 ->
                                        "This device is no longer authorized on that environment."
                                    status != null -> "The environment refused the connection ($status)."
                                    else -> t.message ?: "The connection dropped."
                                }
                            )
                        )
                    }

                    override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                        connection.fail(RpcTransportClosed(reason.ifBlank { "The environment closed the connection." }))
                    }
                }
            connection.socket =
                client.newWebSocket(Request.Builder().url(url).build(), listener)

            val opened = withTimeoutOrNull(OPEN_TIMEOUT_MS) { connection.opened.await() }
            if (opened == null) {
                connection.fail(RpcTransportClosed("The environment did not answer in time."))
            }
            connection.failure?.let { throw it }
            return connection
        }
    }
}
