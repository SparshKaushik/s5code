package club.touchtech.s5code.kotlin.transport

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

/**
 * The `effect/unstable/rpc` message envelopes, hand-written rather than
 * generated because the wire shape is small and stable while the schemas behind
 * it are not: see `RpcMessage.ts` in `.repos/effect-smol`. The server runs
 * `RpcSerialization.layerJson`, so one WebSocket frame carries either one
 * envelope object or an array of them, with no length framing.
 *
 * Payloads stay as [JsonElement] here. Per-method decoding belongs to the
 * caller, which knows the schema; the protocol layer only needs to route by
 * request id.
 */
internal val RpcJson: Json = Json {
    ignoreUnknownKeys = true
    explicitNulls = false
    encodeDefaults = true
}

/** Client → server. */
internal sealed interface RpcFromClient {
    fun toJson(): JsonObject

    data class Request(
        val id: Long,
        val tag: String,
        val payload: JsonElement,
        val headers: List<Pair<String, String>> = emptyList(),
    ) : RpcFromClient {
        override fun toJson(): JsonObject = buildJsonObject {
            put("_tag", "Request")
            put("id", id.toString())
            put("tag", tag)
            put("payload", payload)
            putJsonArray("headers") {
                headers.forEach { (name, value) ->
                    add(buildJsonArray { add(JsonPrimitive(name)); add(JsonPrimitive(value)) })
                }
            }
        }
    }

    /**
     * Required, not optional. The server holds a latch per streaming request
     * and will not emit the next chunk until the previous one is acknowledged,
     * so a client that skips acks receives exactly one chunk and then hangs.
     */
    data class Ack(val requestId: Long) : RpcFromClient {
        override fun toJson(): JsonObject = buildJsonObject {
            put("_tag", "Ack")
            put("requestId", requestId.toString())
        }
    }

    data class Interrupt(val requestId: Long) : RpcFromClient {
        override fun toJson(): JsonObject = buildJsonObject {
            put("_tag", "Interrupt")
            put("requestId", requestId.toString())
        }
    }

    data object Ping : RpcFromClient {
        override fun toJson(): JsonObject = buildJsonObject { put("_tag", "Ping") }
    }
}

/** Server → client. */
internal sealed interface RpcFromServer {
    data class Chunk(val requestId: Long, val values: List<JsonElement>) : RpcFromServer

    data class Exit(val requestId: Long, val outcome: RpcOutcome) : RpcFromServer

    data class Defect(val defect: JsonElement) : RpcFromServer

    data class ProtocolError(val message: String) : RpcFromServer

    data object Pong : RpcFromServer
}

/** The decoded `Exit` body: either a value or the first meaningful failure. */
internal sealed interface RpcOutcome {
    data class Success(val value: JsonElement) : RpcOutcome

    /**
     * `Fail` carries a typed, tagged contract error. `Die` and `Interrupt` are
     * not part of any contract, so they collapse into [kind] rather than
     * getting their own branches nothing would match on.
     */
    data class Failure(val kind: RpcFailureKind, val tag: String?, val message: String) : RpcOutcome
}

/** Public because [RpcFailure] carries it to callers outside this package. */
enum class RpcFailureKind {
    Fail,
    Die,
    Interrupted,
}

/** Parses one WebSocket text frame into zero or more server envelopes. */
internal fun parseRpcFrame(text: String): List<RpcFromServer> {
    val root = RpcJson.parseToJsonElement(text)
    val objects =
        when (root) {
            is JsonArray -> root.mapNotNull { it as? JsonObject }
            is JsonObject -> listOf(root)
            else -> emptyList()
        }
    return objects.mapNotNull(::parseServerEnvelope)
}

private fun parseServerEnvelope(json: JsonObject): RpcFromServer? {
    val requestId = json.requestId()
    return when (json["_tag"]?.jsonPrimitive?.contentOrNullSafe()) {
        "Chunk" ->
            requestId?.let {
                RpcFromServer.Chunk(
                    requestId = it,
                    values = json["values"]?.jsonArray?.toList() ?: emptyList(),
                )
            }
        "Exit" ->
            requestId?.let { RpcFromServer.Exit(requestId = it, outcome = parseOutcome(json["exit"])) }
        "Defect" -> RpcFromServer.Defect(json["defect"] ?: JsonNull)
        "ClientProtocolError" ->
            RpcFromServer.ProtocolError(
                json["error"]?.let(::describeError) ?: "The server rejected the request."
            )
        "Pong" -> RpcFromServer.Pong
        else -> null
    }
}

private fun parseOutcome(exit: JsonElement?): RpcOutcome {
    val obj = exit as? JsonObject ?: return unknownFailure()
    return when (obj["_tag"]?.jsonPrimitive?.contentOrNullSafe()) {
        "Success" -> RpcOutcome.Success(obj["value"] ?: JsonNull)
        "Failure" -> parseCause(obj["cause"]?.jsonArray)
        else -> unknownFailure()
    }
}

/**
 * Picks the first `Fail` in the cause, because that is the only reason a
 * contract describes and therefore the only one worth showing. A cause with no
 * `Fail` is reported by its strongest remaining reason.
 */
private fun parseCause(cause: JsonArray?): RpcOutcome.Failure {
    val reasons = cause?.mapNotNull { it as? JsonObject } ?: emptyList()
    reasons.firstOrNull { it["_tag"]?.jsonPrimitive?.contentOrNullSafe() == "Fail" }?.let { reason ->
        val error = reason["error"]
        return RpcOutcome.Failure(
            kind = RpcFailureKind.Fail,
            tag = (error as? JsonObject)?.get("_tag")?.jsonPrimitive?.contentOrNullSafe(),
            message = error?.let(::describeError) ?: "The request failed.",
        )
    }
    reasons.firstOrNull { it["_tag"]?.jsonPrimitive?.contentOrNullSafe() == "Die" }?.let { reason ->
        return RpcOutcome.Failure(
            kind = RpcFailureKind.Die,
            tag = null,
            message = reason["defect"]?.let(::describeError) ?: "The server hit an internal error.",
        )
    }
    if (reasons.any { it["_tag"]?.jsonPrimitive?.contentOrNullSafe() == "Interrupt" }) {
        return RpcOutcome.Failure(RpcFailureKind.Interrupted, null, "The request was interrupted.")
    }
    return unknownFailure()
}

private fun unknownFailure() =
    RpcOutcome.Failure(RpcFailureKind.Die, null, "The server sent a response this build cannot read.")

/**
 * Contract errors are tagged structs with a `message`; defects can be anything.
 * Falling back to the raw JSON keeps an unrecognized shape debuggable instead of
 * turning every unusual failure into the same empty string.
 */
private fun describeError(error: JsonElement): String {
    val obj = error as? JsonObject ?: return error.jsonPrimitive.contentOrNullSafe() ?: error.toString()
    obj["message"]?.jsonPrimitive?.contentOrNullSafe()?.takeIf { it.isNotBlank() }?.let { return it }
    obj["detail"]?.jsonPrimitive?.contentOrNullSafe()?.takeIf { it.isNotBlank() }?.let { return it }
    obj["_tag"]?.jsonPrimitive?.contentOrNullSafe()?.takeIf { it.isNotBlank() }?.let { return it }
    return obj.toString()
}

/**
 * Request ids are `string | number` on the wire. This client always sends
 * strings, but the server echoes whatever it received and other clients on the
 * same build may not, so both are accepted.
 */
private fun JsonObject.requestId(): Long? =
    this["requestId"]?.jsonPrimitive?.contentOrNullSafe()?.toLongOrNull()

private fun JsonPrimitive.contentOrNullSafe(): String? = if (this is JsonNull) null else content

internal fun JsonElement.asObjectOrNull(): JsonObject? = this as? JsonObject
