package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.HandoffEndpoint
import club.touchtech.s5code.kotlin.transport.v2String
import club.touchtech.s5code.kotlin.transport.wire.ServerProviderDto
import club.touchtech.s5code.kotlin.transport.wire.V2ProjectionDto
import kotlinx.serialization.json.JsonObject

/** A context handoff the next send would trigger: the model the thread last ran on, and the one selected. */
data class PendingHandoff(val from: HandoffEndpoint, val to: HandoffEndpoint)

/**
 * The source half of `resolvePendingHandoff`: the model selection of the latest
 * run that started and was not rolled back. A thread with an empty timeline has
 * nothing to hand off; any loaded row counts, since paging can leave a window
 * of only tool rows. The handoff row is committed before the provider starts,
 * so a starting run carrying a `contextHandoffId` already counts: otherwise the
 * footer would repeat the committed handoff during startup.
 */
internal fun handoffSourceOf(projection: V2ProjectionDto): HandoffEndpoint? {
    if (projection.visibleTurnItems.isEmpty()) return null
    val run =
        projection.runs.lastOrNull {
            it.v2String("status") != "rolled_back" &&
                (it.v2String("startedAt") != null ||
                    (it.v2String("status") == "starting" && it.v2String("contextHandoffId") != null))
        } ?: return null
    val selection = run["modelSelection"] as? JsonObject ?: return null
    return HandoffEndpoint(selection.v2String("instanceId") ?: return null, selection.v2String("model"))
}

/**
 * `resolvePendingHandoff` from `packages/client-runtime/src/handoff.ts`. On the
 * same instance only a model change on a provider that needs a new thread for
 * one hands off. Across instances the native session continues only when both
 * share a driver and the source's continuation group.
 */
internal fun resolvePendingHandoff(
    from: HandoffEndpoint?,
    selection: HandoffEndpoint?,
    providers: List<ServerProviderDto>,
): PendingHandoff? {
    if (from == null || selection == null) return null
    val source = providers.firstOrNull { it.instanceId == from.instanceId }
    val target = providers.firstOrNull { it.instanceId == selection.instanceId }
    val needsHandoff =
        if (from.instanceId == selection.instanceId) {
            from.model != selection.model && target?.requiresNewThreadForModelChange == true
        } else {
            val sourceGroup = source?.continuation?.groupKey
            sourceGroup == null || source.driver != target?.driver || sourceGroup != target.continuation?.groupKey
        }
    return if (needsHandoff) PendingHandoff(from, selection) else null
}
