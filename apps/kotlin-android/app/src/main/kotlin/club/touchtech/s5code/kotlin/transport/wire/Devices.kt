package club.touchtech.s5code.kotlin.transport.wire

import kotlinx.serialization.Serializable

/**
 * `DeviceServiceState`, decoded to what the preview surface reads: which
 * devices exist, which threads have one open, and per-host health. Fields the
 * picker/agent surfaces use (platform availability detail, onboarding flags)
 * stay on the wire until they have a Kotlin consumer.
 */
@Serializable
data class DeviceServiceStateDto(
    val supportsHostRetry: Boolean = false,
    val supportsToolInspection: Boolean = false,
    val hosts: List<DeviceHostSummaryDto> = emptyList(),
    /** disabled | idle | installing | starting | ready | failed */
    val hostStatus: String = "idle",
    val hostStatusDetail: String? = null,
    val hostStatuses: Map<String, DeviceHostStatusDto> = emptyMap(),
    val devices: List<DeviceSummaryDto> = emptyList(),
    val sessions: List<DeviceSessionDto> = emptyList(),
    /** Origin-relative path the client prefixes to hub routes (`/api/device-hub`). */
    val hubBasePath: String = "/api/device-hub",
)

@Serializable
data class DeviceHostSummaryDto(
    val id: String = "",
    /** local | ssh */
    val kind: String = "local",
    val label: String = "",
    val tools: DeviceToolVersionsDto? = null,
    val toolInspectionError: String? = null,
    val hubInstalled: Boolean = false,
    val agentDeviceInstalled: Boolean = false,
)

@Serializable
data class DeviceToolVersionsDto(
    val hub: DeviceToolVersionDto,
    val agent: DeviceToolVersionDto,
)

@Serializable
data class DeviceToolVersionDto(
    val requiredVersion: String = "",
    val installedVersions: List<String> = emptyList(),
    val runningVersion: String? = null,
)

@Serializable
data class DeviceHostStatusDto(
    val status: String = "idle",
    val detail: String? = null,
)

@Serializable
data class DeviceSummaryDto(
    val hostId: String = "",
    val id: String = "",
    /** ios | android */
    val platform: String = "android",
    val name: String = "",
    /** OS label such as "Android 15.0". */
    val version: String = "",
    val booted: Boolean = false,
    val physical: Boolean = false,
)

/** `DeviceSession`: a device a thread is looking at; the stream is shared. */
@Serializable
data class DeviceSessionDto(
    val threadId: String = "",
    val hostId: String = "",
    val deviceId: String = "",
    val platform: String = "android",
    val openedAt: String = "",
)
