package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.v2String
import kotlinx.serialization.json.JsonObject

internal data class UserMessagePresentation(
    val text: String,
    val attribution: String?,
    /** The automation that sent it, for "Open automation" and the row's task title. */
    val scheduledTaskId: String? = null,
)

private val LEGACY_AUTOMATION_PREFIX = Regex("^\\[Triggered by schedule task: [^\\r\\n]+]\\r?\\n\\r?\\n")
private val LEGACY_AUTOMATION_MESSAGE_ID = Regex("^scheduled-task-message:(.+):\\d+:(?:scheduled|manual)$")

/** `resolveUserMessagePresentation` in `packages/client-runtime/src/userMessage.ts`. */
internal fun userMessagePresentation(item: JsonObject): UserMessagePresentation {
    val text = item.v2String("text").orEmpty()
    item.v2String("scheduledTaskId")?.let { return UserMessagePresentation(text, "Automation", it) }
    val prefix = LEGACY_AUTOMATION_PREFIX.find(text)
    val messageId = item.v2String("messageId")
    val legacyTaskId = if (prefix != null) messageId?.let { LEGACY_AUTOMATION_MESSAGE_ID.find(it)?.groupValues?.get(1) } else null
    if (prefix != null && (item.v2String("createdBy") == "agent" ||
            messageId?.startsWith("scheduled-task-message:") == true)) {
        return UserMessagePresentation(text.substring(prefix.range.last + 1), "Automation", legacyTaskId)
    }
    return UserMessagePresentation(text, when {
        item.v2String("createdBy") != "agent" -> null
        item.v2String("creationSource") == "server" -> "S5 Code"
        else -> "Agent"
    })
}
