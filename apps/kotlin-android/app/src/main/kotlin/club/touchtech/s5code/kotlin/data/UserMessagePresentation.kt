package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.v2String
import kotlinx.serialization.json.JsonObject

internal data class UserMessagePresentation(val text: String, val attribution: String?)

internal fun userMessagePresentation(item: JsonObject): UserMessagePresentation {
    val text = item.v2String("text").orEmpty()
    if (item.v2String("scheduledTaskId") != null) return UserMessagePresentation(text, "Automation")
    val prefix = Regex("^\\[Triggered by schedule task: [^\\r\\n]+]\\r?\\n\\r?\\n").find(text)
    if (prefix != null && (item.v2String("createdBy") == "agent" ||
            item.v2String("messageId")?.startsWith("scheduled-task-message:") == true)) {
        return UserMessagePresentation(text.substring(prefix.range.last + 1), "Automation")
    }
    return UserMessagePresentation(text, when {
        item.v2String("createdBy") != "agent" -> null
        item.v2String("creationSource") == "server" -> "S5 Code"
        else -> "Agent"
    })
}
