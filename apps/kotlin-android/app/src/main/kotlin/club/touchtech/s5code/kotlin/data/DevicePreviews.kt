package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.DeviceHost
import club.touchtech.s5code.kotlin.model.DeviceHostStatus
import club.touchtech.s5code.kotlin.model.DeviceServiceSnapshot
import club.touchtech.s5code.kotlin.model.DeviceSession
import club.touchtech.s5code.kotlin.model.DeviceSummary
import club.touchtech.s5code.kotlin.model.DeviceToolVersion
import club.touchtech.s5code.kotlin.model.DeviceToolVersions
import club.touchtech.s5code.kotlin.model.ThreadDevicePreview
import club.touchtech.s5code.kotlin.transport.wire.DeviceServiceStateDto

/** `DeviceServiceStateDto` → the model the preview surface reads. */
fun DeviceServiceStateDto.toSnapshot(): DeviceServiceSnapshot =
    DeviceServiceSnapshot(
        hosts =
            hosts.map { host ->
                DeviceHost(
                    id = host.id,
                    label = host.label,
                    tools =
                        host.tools?.let { tools ->
                            DeviceToolVersions(
                                hub =
                                    DeviceToolVersion(
                                        tools.hub.requiredVersion,
                                        tools.hub.installedVersions,
                                        tools.hub.runningVersion,
                                    ),
                                agent =
                                    DeviceToolVersion(
                                        tools.agent.requiredVersion,
                                        tools.agent.installedVersions,
                                        tools.agent.runningVersion,
                                    ),
                            )
                        },
                    toolInspectionError = host.toolInspectionError,
                )
            },
        hostStatuses =
            hostStatuses.mapValues { (_, status) ->
                DeviceHostStatus(status = status.status, detail = status.detail)
            },
        devices =
            devices.map { device ->
                DeviceSummary(
                    hostId = device.hostId,
                    id = device.id,
                    platform = device.platform,
                    name = device.name,
                    version = device.version,
                )
            },
        sessions =
            sessions.map { session ->
                DeviceSession(
                    threadId = session.threadId,
                    hostId = session.hostId,
                    deviceId = session.deviceId,
                    platform = session.platform,
                )
            },
        supportsHostRetry = supportsHostRetry,
        supportsToolInspection = supportsToolInspection,
        hubBasePath = hubBasePath,
    )

/**
 * `threadDevicePreviews`: the devices this thread holds open, one row per
 * session. Host id stays in the key because Android serials repeat across
 * hosts.
 */
fun threadDevicePreviews(
    state: DeviceServiceSnapshot?,
    threadId: String,
): List<ThreadDevicePreview> =
    (state?.sessions ?: emptyList())
        .filter { it.threadId == threadId }
        .map { session ->
            val device =
                state?.devices?.firstOrNull {
                    it.hostId == session.hostId && it.id == session.deviceId
                }
            val host = state?.hosts?.firstOrNull { it.id == session.hostId }
            ThreadDevicePreview(
                key = "${session.hostId}/${session.deviceId}",
                session = session,
                name =
                    device?.name
                        ?: if (session.platform == "ios") "iOS Simulator" else "Android Emulator",
                description =
                    listOfNotNull(
                            device?.version?.takeIf { it.isNotEmpty() },
                            host?.label?.takeIf { it.isNotEmpty() },
                        )
                        .joinToString(" · "),
            )
        }

/** `selectedThreadDevicePreview`: the user's pick, else the first row. */
fun selectedThreadDevicePreview(
    previews: List<ThreadDevicePreview>,
    selectedKey: String?,
): ThreadDevicePreview? =
    previews.firstOrNull { it.key == selectedKey } ?: previews.firstOrNull()

/* ── Tool versions — `deviceToolVersionLabels`/`deviceToolUpdatePolicy` ── */

/** Unknown inventory is distinct from a completed check that found no install. */
fun deviceToolVersionLabels(tools: DeviceToolVersions?): List<String> {
    if (tools == null) return listOf("Device tool versions have not been checked.")
    return listOf("Device hub" to tools.hub, "Agent tools" to tools.agent).map { (name, tool) ->
        val installed =
            if (tool.installedVersions.isEmpty()) "none" else tool.installedVersions.joinToString(", ")
        "$name: installed $installed; required ${tool.requiredVersion}" +
            (tool.runningVersion?.let { "; running $it" } ?: "") +
            "."
    }
}

fun deviceToolUpdatePolicy(tools: DeviceToolVersions?): String {
    if (tools == null) return "Versions have not been checked. Reconnect the host and check versions."
    val outdated =
        listOf(tools.hub, tools.agent).filter {
            it.installedVersions.isNotEmpty() && it.requiredVersion !in it.installedVersions
        }
    return if (outdated.isNotEmpty()) {
        "Update pending. Required tools will install automatically when next used. " +
            "The host needs network access; an older install is not used as a fallback."
    } else {
        "Required tools are installed automatically when needed. " +
            "Checking versions does not install or start anything."
    }
}

const val DeviceToolUpdateOwnership =
    "This environment's T3 server chooses device tool versions for itself and its SSH hosts. " +
        "Update that server to receive newer tool versions; updating only your browser or " +
        "mobile app does not update a remote server."
