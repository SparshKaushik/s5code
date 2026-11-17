package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.v2String
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

/*
 * Webhook trigger editing from `packages/client-runtime/src/scheduledTaskWebhook.ts`
 * and the dedupe checks in `packages/contracts/src/scheduledTask.ts`. Each check
 * mirrors the server schema, so a draft that passes here also saves.
 */

/** `MAX_WEBHOOK_FILTERS` in the contracts. */
const val MAX_WEBHOOK_FILTERS = 16

/**
 * One editable filter row. `in` takes a comma-separated list, which is
 * lossless because list values can't contain commas; `exists` ignores the value.
 * [id] is row identity for the form and never sent.
 */
data class WebhookFilterDraft(val id: String, val path: String, val op: String, val value: String)

/** `WEBHOOK_FILTER_OPS`: wire op to the label the form shows. */
val WEBHOOK_FILTER_OPS =
    listOf(
        "equals" to "equals",
        "notEquals" to "does not equal",
        "in" to "is one of",
        "startsWith" to "starts with",
        "exists" to "exists",
    )

private var lastWebhookFilterDraftId = 0

private fun nextWebhookFilterDraftId(): String = "filter-${++lastWebhookFilterDraftId}"

fun emptyWebhookFilterDraft(): WebhookFilterDraft =
    WebhookFilterDraft(nextWebhookFilterDraftId(), path = "", op = "equals", value = "")

/** The stored `schedule.filters` as form rows. */
fun webhookFilterDrafts(filters: JsonArray?): List<WebhookFilterDraft> =
    filters.orEmpty().mapNotNull { element ->
        val filter = element as? JsonObject ?: return@mapNotNull null
        val op = filter.v2String("op") ?: return@mapNotNull null
        val value =
            when (op) {
                "exists" -> ""
                "in" -> (filter["value"] as? JsonArray).orEmpty().mapNotNull { (it as? JsonPrimitive)?.content }.joinToString(", ")
                else -> filter.v2String("value").orEmpty()
            }
        WebhookFilterDraft(nextWebhookFilterDraftId(), filter.v2String("path").orEmpty(), op, value)
    }

private val FILTER_PATH = Regex("^(body|headers|query)(\\.|$)")

// `(?U)`: JavaScript's `\s` is Unicode-aware; Java's is ASCII unless asked.
private val FILTER_LIST_VALUE = Regex("(?U)^[^,\\s](?:[^,]*[^,\\s])?$")

sealed interface WebhookFilterParse {
    data class Filters(val filters: JsonArray) : WebhookFilterParse

    data class Invalid(val error: String) : WebhookFilterParse
}

/** `parseWebhookFilterDrafts`: the error names the first row that would not save. */
fun parseWebhookFilterDrafts(drafts: List<WebhookFilterDraft>): WebhookFilterParse {
    if (drafts.size > MAX_WEBHOOK_FILTERS) {
        return WebhookFilterParse.Invalid("Use at most $MAX_WEBHOOK_FILTERS filters.")
    }
    val filters =
        buildJsonArray {
            drafts.forEachIndexed { index, draft ->
                val path = draft.path.trim()
                val values =
                    if (draft.op == "in") draft.value.split(",").map { it.trim() }.filter { it.isNotEmpty() }
                    else emptyList()
                val valid =
                    path.isNotEmpty() && FILTER_PATH.containsMatchIn(path) &&
                        when (draft.op) {
                            "equals", "notEquals", "startsWith", "exists" -> true
                            "in" -> values.isNotEmpty() && values.all { FILTER_LIST_VALUE.matches(it) }
                            else -> false
                        }
                if (!valid) {
                    val needs =
                        when (draft.op) {
                            "exists" -> "."
                            "in" -> ", and at least one value."
                            else -> ", and a value."
                        }
                    return WebhookFilterParse.Invalid("Filter ${index + 1} needs a path under body, headers or query$needs")
                }
                add(
                    buildJsonObject {
                        put("path", path)
                        put("op", draft.op)
                        when (draft.op) {
                            "exists" -> Unit
                            "in" -> putJsonArray("value") { values.forEach { add(it) } }
                            else -> put("value", draft.value)
                        }
                    }
                )
            }
        }
    return WebhookFilterParse.Filters(filters)
}

/**
 * Whether the rows differ from the ones the form opened with. A save leaves
 * `filters` out unless they did, so editing only the title can't overwrite
 * filters another device changed in the meantime.
 */
fun webhookFilterDraftsChanged(saved: List<WebhookFilterDraft>, current: List<WebhookFilterDraft>): Boolean =
    saved.size != current.size ||
        saved.indices.any { index ->
            val before = saved[index]
            val after = current[index]
            before.path != after.path || before.op != after.op ||
                (before.op != "exists" && before.value != after.value)
        }

private val DEDUPE_PLACEHOLDER =
    Regex("""(?U)\{\{\s*(?:(?:body|headers|query)(?:\.[^{}.\s]+)*|request)\s*\}\}""")

/** `isWebhookDedupeTemplate`: at least one request placeholder and no stray braces. */
fun isWebhookDedupeTemplate(template: String): Boolean {
    if (!DEDUPE_PLACEHOLDER.containsMatchIn(template)) return false
    val remainder = DEDUPE_PLACEHOLDER.replace(template, "")
    return "{{" !in remainder && "}}" !in remainder
}

/** The stored `schedule.dedupe` as the form's three text fields. */
data class WebhookDedupeDraft(val replayKey: String = "", val coalesceKey: String = "", val coalesceWindowSeconds: String = "")

fun webhookDedupeDraft(dedupe: JsonObject?): WebhookDedupeDraft =
    WebhookDedupeDraft(
        replayKey = dedupe?.v2String("replayKey").orEmpty(),
        coalesceKey = dedupe?.v2String("coalesceKey").orEmpty(),
        coalesceWindowSeconds = (dedupe?.get("coalesceWindowSeconds") as? JsonPrimitive)?.intOrNull?.toString().orEmpty(),
    )

/** `webhookDraftErrors` for the dedupe fields; null when the draft saves. */
fun webhookDedupeError(draft: WebhookDedupeDraft): String? {
    for ((label, value) in listOf("Replay key" to draft.replayKey, "Coalesce key" to draft.coalesceKey)) {
        if (value.isNotBlank() && !isWebhookDedupeTemplate(value.trim())) {
            return "$label must contain a valid request placeholder."
        }
    }
    val window = draft.coalesceWindowSeconds.trim()
    if (window.isNotEmpty() && window.toIntOrNull()?.let { it in 10..3600 } != true) {
        return "Coalesce window must be whole seconds from 10 to 3600."
    }
    return null
}

/**
 * The `dedupe` an upsert sends. Always present: an empty object clears what
 * the server stored, which is what blank fields mean.
 */
fun webhookDedupeJson(draft: WebhookDedupeDraft): JsonObject =
    buildJsonObject {
        draft.replayKey.trim().takeIf { it.isNotEmpty() }?.let { put("replayKey", it) }
        draft.coalesceKey.trim().takeIf { it.isNotEmpty() }?.let { put("coalesceKey", it) }
        draft.coalesceWindowSeconds.trim().toIntOrNull()?.let { put("coalesceWindowSeconds", it) }
    }

/** `webhookDeliveryLabel` in the web settings; an unknown outcome keeps its raw name. */
fun webhookDeliveryLabel(outcome: String, pendingWindow: Boolean): String =
    when (outcome) {
        "accepted" -> if (pendingWindow) "Opened window" else "Ran"
        "dispatch_failed" -> "Run failed"
        "duplicate" -> "Duplicate"
        "coalesced" -> "Coalesced"
        "filtered" -> "Filtered"
        "rejected_signature" -> "Bad signature"
        "disabled" -> "Task paused"
        "rate_limited" -> "Rate limited"
        "expired" -> "Too old"
        else -> outcome.replace("_", " ")
    }
