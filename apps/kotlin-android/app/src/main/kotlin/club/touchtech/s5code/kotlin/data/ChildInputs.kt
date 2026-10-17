package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.PendingChildInput
import club.touchtech.s5code.kotlin.transport.v2String
import club.touchtech.s5code.kotlin.transport.wire.ThreadShellDto

/** A descendant's question remains reachable even when its parent turn has finished. */
internal fun childInputs(threads: List<ThreadShellDto>, parentThreadId: String): List<PendingChildInput> {
    val parents = threads.associate { it.id to it.lineage?.v2String("parentThreadId") }
    return threads.mapNotNull { thread ->
        if (thread.id == parentThreadId) return@mapNotNull null
        val kind = thread.pendingRuntimeRequest?.v2String("kind") ?: return@mapNotNull null
        if (kind in setOf("auth_refresh", "dynamic_tool_call")) return@mapNotNull null
        val seen = hashSetOf(thread.id)
        var ancestor = parents[thread.id]
        while (ancestor != null && ancestor != parentThreadId && seen.add(ancestor)) ancestor = parents[ancestor]
        if (ancestor != parentThreadId) return@mapNotNull null
        PendingChildInput(thread.id, thread.title.ifBlank { "Subagent" }, kind != "user_input")
    }
}
