package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.data.isSummarizedValue
import club.touchtech.s5code.kotlin.data.toolOutputImageCount
import club.touchtech.s5code.kotlin.transport.v2Bool
import club.touchtech.s5code.kotlin.transport.v2Long
import club.touchtech.s5code.kotlin.transport.v2Objects
import club.touchtech.s5code.kotlin.transport.v2String
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull

/**
 * Display text for the full turn item `orchestration.getTurnItem` returns,
 * ported from `packages/client-runtime/src/work-log/itemDetail.ts`. The wire
 * timeline withholds large tool payloads (`outputOmitted`, summarized input),
 * so an expanded row fetches the item and renders what the timeline never
 * carried.
 */

private const val MAX_TEXT_BLOCK_DEPTH = 4

private fun isRenderableImageBlock(value: JsonObject): Boolean =
    toolOutputImageCount(JsonArray(listOf(value))) > 0

private fun textFromBlocks(value: JsonElement?, depth: Int): String? {
    if (value == null || value == JsonNull || depth > MAX_TEXT_BLOCK_DEPTH) return null
    if (value is JsonPrimitive) return if (value.isString) value.content else null
    if (value is JsonArray) {
        val parts = value.map { textFromBlocks(it, depth + 1) }
        return if (parts.all { it != null }) parts.filter { it?.isNotEmpty() == true }.joinToString("\n") else null
    }
    if (value !is JsonObject) return null
    when (value.v2String("type")) {
        "text" -> return value.v2String("text")
        // Images clients can show render on their own (see turnItemDetailImageCount).
        "image" -> return if (isRenderableImageBlock(value)) "" else "[image]"
        "resource_link" -> return value.v2String("uri")
        "resource" -> {
            val resource = value["resource"] as? JsonObject
            if (resource != null) {
                resource.v2String("text")?.let { return it }
                resource.v2String("uri")?.let { return it }
            }
        }
    }
    val keys = value.keys.filter { it != "isError" && it != "is_error" }
    // MCP results and provider tool results wrap their text in `content`.
    // `structuredContent` usually repeats it as data, so it only shows when the
    // text is empty.
    if (keys.size == 1 && keys.first() == "content") return textFromBlocks(value["content"], depth + 1)
    if (keys.size == 2 && "content" in keys && "structuredContent" in keys) {
        return textFromBlocks(value["content"], depth + 1)?.takeIf { it.isNotBlank() }
    }
    return null
}

private fun parseJson(text: String): JsonElement? =
    runCatching { Json.parseToJsonElement(text) }.getOrNull()

private val prettyJson = Json { prettyPrint = true }

private fun JsonElement.encodePretty(): String =
    prettyJson.encodeToString(JsonElement.serializer(), this)

/** MCP tools often return JSON as minified text, one document per line. Indent each. */
private fun prettyJsonText(text: String): String {
    val trimmed = text.trim()
    if (!trimmed.startsWith('{') && !trimmed.startsWith('[')) return text
    val whole = parseJson(trimmed)
    if (whole != null) return whole.encodePretty()
    val lines = trimmed.split('\n').filter { it.isNotBlank() }
    val documents = lines.map { parseJson(it.trim()) }
    if (documents.any { it == null }) return text
    return documents.joinToString("\n\n") { it!!.encodePretty() }
}

/** Formats a tool input or output for display: text blocks as text, the rest as JSON. */
private fun formatToolValue(value: JsonElement?): String? {
    if (value == null || value == JsonNull) return null
    val text = textFromBlocks(value, 0)
    if (text != null) return if (text.isNotBlank()) prettyJsonText(text) else null
    val json =
        if (value is JsonPrimitive) value.contentOrNull
        else runCatching { value.encodePretty() }.getOrNull()
    if (json == null || json == "{}" || json == "[]") return null
    return json
}

/**
 * `toolCallLines` for a fetched `dynamic_tool` input: flat arguments render as
 * `key value` pairs, anything else (including the summarized stub) falls back
 * to the generic formatter.
 */
private fun toolInputText(input: JsonElement?): String? {
    if (input !is JsonObject || input.isSummarizedValue()) return formatToolValue(input)
    if (input.isEmpty()) return null
    return input.entries.joinToString("\n") { (key, value) ->
        val rendered =
            if (value is JsonPrimitive && value.isString && value.content.isNotEmpty()) {
                value.content
            } else {
                runCatching { value.encodePretty() }.getOrElse { value.toString() }
            }
        "$key $rendered"
    }
}

/** Older Claude bash rows stored the raw `{ stdout, stderr, interrupted, … }` result. */
private fun commandOutputText(output: String): String {
    if (!output.trimStart().startsWith("{\"stdout\"")) return output
    val parsed = parseJson(output.trim()) as? JsonObject ?: return output
    val stdout = parsed.v2String("stdout") ?: return output
    val stderr = parsed.v2String("stderr") ?: return output
    if ((parsed["interrupted"] as? JsonPrimitive)?.booleanOrNull == null) return output
    return listOf(stdout, stderr).filter { it.isNotBlank() }.joinToString("\n")
}

/** The tool output carried by a fetched item, formatted for display. */
internal fun turnItemOutputText(item: JsonObject): String? =
    when (item.v2String("type")) {
        "command_execution" ->
            item.v2String("output")
                ?.takeIf { it.isNotBlank() }
                ?.let { commandOutputText(it).ifBlank { null } }
        "dynamic_tool" ->
            if (item.v2Bool("outputOmitted")) null else formatToolValue(item["output"])
        "file_search" ->
            item.v2Objects("results")
                .map { result ->
                    listOfNotNull(
                        result.v2String("fileName")?.let {
                            it + (result.v2Long("line")?.let { line -> ":$line" } ?: "")
                        },
                        result.v2String("preview")?.trim(),
                    ).filter { it.isNotEmpty() }.joinToString("\n")
                }
                .filter { it.isNotEmpty() }
                .joinToString("\n")
                .ifEmpty { null }
        "web_search" ->
            item.v2Objects("results")
                .map { result ->
                    val title = result.v2String("title")?.trim().orEmpty()
                    listOfNotNull(
                        title.ifEmpty { result.v2String("url") },
                        if (title.isNotEmpty()) result.v2String("url") else null,
                        result.v2String("snippet")?.trim(),
                    ).filter { it.isNotEmpty() }.joinToString("\n")
                }
                .filter { it.isNotEmpty() }
                .joinToString("\n\n")
                .ifEmpty { null }
        else -> null
    }

/**
 * What an expanded row shows once `getTurnItem` has answered: the output for a
 * withheld command, and for a dynamic tool the fetched arguments plus output —
 * the row's own detail only ever carried the summarized input stub.
 */
internal fun fetchedTurnItemText(item: JsonObject): String? =
    when (item.v2String("type")) {
        "dynamic_tool" ->
            listOfNotNull(toolInputText(item["input"]), turnItemOutputText(item))
                .takeIf { it.isNotEmpty() }
                ?.joinToString("\n\n")
        else -> turnItemOutputText(item)
    }

/**
 * `turnItemOutputImages`, counted like the wire side: the detail read leaves
 * image bytes out, so each loads over HTTP through a `tool-output-image`
 * asset addressed by its index in the output.
 */
internal fun turnItemDetailImageCount(item: JsonObject): Int =
    if (item.v2String("type") != "dynamic_tool" || item.v2Bool("outputOmitted")) {
        0
    } else {
        toolOutputImageCount(item["output"])
    }
