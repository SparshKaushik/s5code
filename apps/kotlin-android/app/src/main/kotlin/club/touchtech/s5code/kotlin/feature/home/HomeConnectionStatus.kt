package club.touchtech.s5code.kotlin.feature.home

import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.model.Environment

/**
 * The line under the Home title, mirroring `workspaceConnectionStatusLabel` in
 * `apps/mobile/src/features/home/workspace-connection-status.ts`.
 *
 * Once any environment is connected the list is live, so the header speaks only
 * for connected environments still reading their threads. A sleeping secondary
 * machine's retries belong to its own row on Connections: reporting them here
 * would replace the brand with "Reconnecting…" for as long as that laptop is shut.
 * Without a connected environment it falls back to the connection story:
 * offline first, then reconnecting, then the first load.
 */
internal fun homeConnectionStatus(
    environments: List<Environment>,
    paired: Boolean,
    sessionRestored: Boolean,
): String? {
    // Switched-off environments have no session, so they are never the story.
    val enabled = environments.filter { it.isEnabled }
    val connected = enabled.filter { it.state == ConnectionState.Connected }
    if (connected.isNotEmpty()) {
        if (connected.all { it.snapshotLoaded }) return null
        return if (enabled.any { it.snapshotLoaded }) "Syncing threads…" else "Loading threads…"
    }
    val connecting =
        enabled.filter { it.state == ConnectionState.Connecting || it.state == ConnectionState.Recovering }
    return when {
        enabled.any { it.state == ConnectionState.Offline } -> "You are offline"
        connecting.size == 1 -> "Reconnecting to ${connecting.single().label}…"
        connecting.size > 1 -> "Reconnecting ${connecting.size} environments"
        environments.isEmpty() && sessionRestored && !paired -> "No environments"
        enabled.none { it.snapshotLoaded } && enabled.isNotEmpty() -> "Loading threads…"
        else -> null
    }
}
