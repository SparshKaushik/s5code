package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.TransportJson
import club.touchtech.s5code.kotlin.transport.v2String
import kotlinx.serialization.json.*

/** Read the captured app only when it belongs to the tool and server that produced this item. */
internal fun mcpAppReference(item: JsonObject): JsonObject? {
    fun read(value: JsonElement?, depth: Int = 0): JsonObject? {
        if (depth > 8) return null
        return when (value) {
            is JsonPrimitive -> value.contentOrNull?.takeIf { it.length <= 256 * 1024 }?.let {
                read(runCatching { TransportJson.parseToJsonElement(it) }.getOrNull(), depth + 1)
            }
            is JsonArray -> value.take(64).firstNotNullOfOrNull { read(it, depth + 1) }
            is JsonObject -> (value["t3McpApp"] as? JsonObject) ?: when {
                value["structuredContent"] != null -> read(value["structuredContent"], depth + 1)
                value["content"] != null -> read(value["content"], depth + 1)
                value.v2String("type") == "text" -> read(value["text"], depth + 1)
                else -> null
            }
            else -> null
        }
    }
    val app = read(item["output"]) ?: return null
    val server = app.v2String("server")?.takeIf { it.isNotBlank() && it.length <= 256 } ?: return null
    val tool = app.v2String("tool")?.takeIf { it.isNotBlank() && it.length <= 256 } ?: return null
    if (item.v2String("toolName") != "$server.$tool") return null
    if (app.v2String("attachmentId").isNullOrBlank() || app.v2String("resourceUri")?.startsWith("ui://") != true) return null
    return app
}
