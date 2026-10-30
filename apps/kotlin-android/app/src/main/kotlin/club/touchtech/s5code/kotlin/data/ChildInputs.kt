package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.PendingChildInput
import club.touchtech.s5code.kotlin.transport.v2String
import club.touchtech.s5code.kotlin.transport.wire.FIELD_ABSENT
import club.touchtech.s5code.kotlin.transport.wire.ThreadShellDto

/** `PendingSubagentRequest` values: what a descendant subagent is blocked on. */
internal const val SUBAGENT_REQUEST_APPROVAL = "approval"
internal const val SUBAGENT_REQUEST_USER_INPUT = "user_input"

/**
 * An async question outlives the subagent's run, but once that run ends the
 * parent is no longer waiting on the answer. The activity run stays set while a
 * follow-up is queued behind a blocked run; older servers omit the field, so
 * fall back to the latest run's status. Mirrors `isChildRunLive` in
 * `packages/client-runtime/src/state/threadShell.ts`.
 */
private fun ThreadShellDto.childRunLive(): Boolean =
    if (activityRunStatus != FIELD_ABSENT) activityRunStatus != null
    else status in setOf("preparing", "starting", "running", "waiting")

/**
 * `subagentRequest`: what a live app-created subagent is waiting on, or null.
 * Provider-native subagents ask on their parent, so they are skipped. Dynamic
 * tool calls are answered by the requesting client, never by this one.
 */
internal fun subagentRequestOf(thread: ThreadShellDto): String? {
    val kind = thread.pendingRuntimeRequest?.v2String("kind") ?: return null
    if (kind in setOf("auth_refresh", "dynamic_tool_call")) return null
    if (thread.lineage?.v2String("relationshipToParent") != "subagent") return null
    if (thread.creationSource == "provider") return null
    if (!thread.childRunLive()) return null
    return if (kind == "user_input") SUBAGENT_REQUEST_USER_INPUT else SUBAGENT_REQUEST_APPROVAL
}

/**
 * `pendingSubagentRequestsByThread`: stamps every ancestor up a blocked
 * subagent's chain with its approval, so the top-level row a user sees asks for
 * it. A lineage cycle stops the walk.
 */
internal fun pendingSubagentRequestsByThread(threads: List<ThreadShellDto>): Map<String, String> {
    val requests = mutableMapOf<String, String>()
    var byId: Map<String, ThreadShellDto>? = null
    for (child in threads) {
        val request = subagentRequestOf(child) ?: continue
        // The coordinator answers its subagents' questions; only approvals need the user.
        if (request == SUBAGENT_REQUEST_USER_INPUT) continue
        val index = byId ?: threads.associateBy { it.id }.also { byId = it }
        val seen = hashSetOf(child.id)
        var current: ThreadShellDto? = child
        while (current?.lineage?.v2String("relationshipToParent") == "subagent") {
            val parentId = current.lineage.v2String("parentThreadId") ?: break
            if (!seen.add(parentId)) break
            if (requests[parentId] != SUBAGENT_REQUEST_APPROVAL) requests[parentId] = request
            current = index[parentId]
        }
    }
    return requests
}

/** A descendant subagent's question remains reachable even when its parent turn has finished. */
internal fun childInputs(threads: List<ThreadShellDto>, parentThreadId: String): List<PendingChildInput> {
    val parents = threads
        .filter { it.lineage?.v2String("relationshipToParent") == "subagent" }
        .associate { it.id to it.lineage?.v2String("parentThreadId") }
    return threads.mapNotNull { thread ->
        if (thread.id == parentThreadId) return@mapNotNull null
        val request = subagentRequestOf(thread) ?: return@mapNotNull null
        val seen = hashSetOf(thread.id)
        var ancestor = parents[thread.id]
        while (ancestor != null && ancestor != parentThreadId && seen.add(ancestor)) ancestor = parents[ancestor]
        if (ancestor != parentThreadId) return@mapNotNull null
        PendingChildInput(thread.id, thread.title.ifBlank { "Subagent" }, request == SUBAGENT_REQUEST_APPROVAL)
    }
}

/** `childThreadInputsTitle`: the one-line banner over a parent's blocked subagents. */
internal fun childInputsTitle(children: List<PendingChildInput>): String {
    val approvals = children.count { it.approval }
    val need = when (approvals) {
        0 -> "input"
        children.size -> "approval"
        else -> "you"
    }
    return if (children.size == 1) "Subagent needs $need" else "${children.size} subagents need $need"
}
