package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.v2String
import kotlinx.serialization.json.JsonObject

internal fun reduceBrowserTabs(tabs: List<JsonObject>, event: JsonObject): List<JsonObject> {
    val id = event.v2String("tabId") ?: return tabs
    if (event.v2String("type") == "closed") return tabs.filterNot { it.v2String("tabId") == id }
    val snapshot = event["snapshot"] as? JsonObject ?: return tabs
    val index = tabs.indexOfFirst { it.v2String("tabId") == id }
    return if (index < 0) tabs + snapshot else tabs.toMutableList().apply { this[index] = snapshot }
}
