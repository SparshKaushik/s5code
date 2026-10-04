package club.touchtech.s5code.kotlin.transport.wire

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

/** V2 entities have open/provider-shaped payloads. Preserve them for projection and replay. */
@Serializable
data class V2ProjectionDto(
    val thread: JsonObject,
    val runs: List<JsonObject> = emptyList(),
    val attempts: List<JsonObject> = emptyList(),
    val nodes: List<JsonObject> = emptyList(),
    val subagents: List<JsonObject> = emptyList(),
    val providerSessions: List<JsonObject> = emptyList(),
    val providerThreads: List<JsonObject> = emptyList(),
    val providerTurns: List<JsonObject> = emptyList(),
    val runtimeRequests: List<JsonObject> = emptyList(),
    val messages: List<JsonObject> = emptyList(),
    val plans: List<JsonObject> = emptyList(),
    val turnItems: List<JsonObject> = emptyList(),
    val checkpointScopes: List<JsonObject> = emptyList(),
    val checkpoints: List<JsonObject> = emptyList(),
    val contextHandoffs: List<JsonObject> = emptyList(),
    val contextTransfers: List<JsonObject> = emptyList(),
    val visibleTurnItems: List<V2VisibleItemDto> = emptyList(),
    val updatedAt: String? = null,
)

@Serializable
data class V2VisibleItemDto(
    val position: Int = 0,
    val visibility: String = "local",
    val sourceThreadId: String,
    val sourceItemId: String,
    val item: JsonObject,
)

@Serializable
data class V2ThreadSnapshotDto(
    val snapshotSequence: Long = 0,
    val projection: V2ProjectionDto,
    val historyCursor: String? = null,
    val hasMoreHistory: Boolean = false,
    val latestLocalTurnOrdinal: Long? = null,
)

@Serializable
data class V2ThreadStreamDto(
    val kind: String,
    val snapshotSequence: Long = 0,
    val sequence: Long = 0,
    val projection: V2ProjectionDto? = null,
    val event: JsonObject? = null,
    val historyCursor: String? = null,
    val hasMoreHistory: Boolean = false,
    val latestLocalTurnOrdinal: Long? = null,
)

@Serializable
data class V2HistoryPageDto(
    val snapshotSequence: Long = 0,
    val items: List<V2VisibleItemDto> = emptyList(),
    val nextCursor: String? = null,
    val hasMoreHistory: Boolean = false,
)
