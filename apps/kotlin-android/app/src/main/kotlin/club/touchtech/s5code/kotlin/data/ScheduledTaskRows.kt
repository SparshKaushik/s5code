package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.v2Objects
import club.touchtech.s5code.kotlin.transport.v2String
import kotlinx.serialization.json.JsonObject

/** Unknown triggers cannot be edited safely by this client's schedule form. */
internal fun scheduledTaskRows(payload: JsonObject): List<JsonObject> =
    payload.v2Objects("tasks").filter { task ->
        !task.v2String("id").isNullOrBlank() &&
            (task["schedule"] as? JsonObject)?.v2String("type") in
                setOf("interval", "fixed_time", "webhook")
    }
