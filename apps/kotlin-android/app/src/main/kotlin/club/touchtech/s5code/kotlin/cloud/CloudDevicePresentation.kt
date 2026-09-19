package club.touchtech.s5code.kotlin.cloud

import club.touchtech.s5code.kotlin.data.absoluteLabel

/** One registered client as the Connect screen shows it. */
data class CloudDeviceRow(
    val deviceId: String,
    val label: String,
    /** Platform and app version, e.g. "Android · S5 Code 0.1.0" or "iOS 18". */
    val platform: String,
    /** What this device will be told about, in one sentence. */
    val notifications: String,
    val updated: String,
)

/**
 * How the account's registered clients are described.
 *
 * Ported from `mobileClientPlatformLabel` / `mobileClientNotificationDetail` /
 * `mobileClientUpdatedAtLabel` in
 * `apps/web/src/components/clerk/MobileClientsUserProfilePage.logic.ts`, the only
 * surface that renders `RelayClientDeviceRecord` today. Keeping the wording close
 * matters: a user comparing the phone against the web profile should not have to
 * work out whether two different sentences mean the same thing.
 *
 * Pure by construction so it can be tested without a relay: every input is a
 * decoded DTO and every output is a string a row can render.
 */
object CloudDevicePresentation {

    private val notificationPreferences:
        List<Pair<(RelayDeviceNotificationsDto) -> Boolean, String>> =
        listOf(
            { notifications: RelayDeviceNotificationsDto -> notifications.notifyOnApproval } to
                "approvals",
            { notifications: RelayDeviceNotificationsDto -> notifications.notifyOnInput } to
                "input requests",
            { notifications: RelayDeviceNotificationsDto -> notifications.notifyOnCompletion } to
                "completions",
            { notifications: RelayDeviceNotificationsDto -> notifications.notifyOnFailure } to
                "failures",
        )

    /**
     * `iosMajorVersion` is null for Android and absent on older iOS builds, so the
     * version is appended only when the relay actually knows it — "iOS" alone
     * beats "iOS null".
     */
    fun platformLabel(device: RelayDeviceDto): String {
        val platform =
            when (device.platform) {
                "android" -> "Android"
                "ios" -> listOfNotNull("iOS", device.iosMajorVersion?.toString()).joinToString(" ")
                // A platform this build has never heard of still names itself
                // rather than being dropped from the list.
                else -> device.platform.ifBlank { "Unknown device" }
            }
        return device.appVersion?.takeIf { it.isNotBlank() }?.let { "$platform · S5 Code $it" }
            ?: platform
    }

    fun notificationDetail(device: RelayDeviceDto): String {
        if (!device.notifications.enabled) return "Push notifications are off on this device."
        val enabled =
            notificationPreferences.filter { it.first(device.notifications) }.map { it.second }
        return if (enabled.isEmpty()) {
            "Push notifications are on, but no alert types are selected."
        } else {
            "Alerts for ${enabled.joinToString(", ")}."
        }
    }

    /** Absolute rather than relative: this is a registration time, not activity. */
    fun updatedLabel(updatedAt: String): String =
        absoluteLabel(updatedAt).takeIf { it.isNotEmpty() }?.let { "Updated $it" }
            ?: "Update time unavailable"

    fun row(device: RelayDeviceDto): CloudDeviceRow =
        CloudDeviceRow(
            deviceId = device.deviceId,
            label = device.label,
            platform = platformLabel(device),
            notifications = notificationDetail(device),
            updated = updatedLabel(device.updatedAt),
        )

    /**
     * Most recently updated first, ties broken by label.
     *
     * The relay returns whatever order its store hands back, and a list that
     * reshuffles between two refreshes reads as if devices came and went.
     */
    fun rows(devices: List<RelayDeviceDto>): List<CloudDeviceRow> =
        devices
            .sortedWith(compareByDescending<RelayDeviceDto> { it.updatedAt }.thenBy { it.label })
            .map(::row)
}
