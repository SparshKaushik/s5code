package club.touchtech.s5code.kotlin.cloud

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * How a registered client is described, against the wording
 * `MobileClientsUserProfilePage.logic.ts` uses on the web.
 *
 * The interesting cases are all "the relay knows less than the UI wants": an
 * Android device with no iOS version, an old build with no `appVersion`, a
 * platform this build predates, and an unparseable timestamp. Every one of them
 * has to produce a sentence rather than a hole.
 */
class CloudDevicePresentationTest {

    private fun device(
        deviceId: String = "device-1",
        label: String = "Pixel 9",
        platform: String = "android",
        iosMajorVersion: Int? = null,
        appVersion: String? = null,
        notifications: RelayDeviceNotificationsDto = RelayDeviceNotificationsDto(),
        updatedAt: String = "2026-02-01T10:00:00Z",
    ) =
        RelayDeviceDto(
            deviceId = deviceId,
            label = label,
            platform = platform,
            iosMajorVersion = iosMajorVersion,
            appVersion = appVersion,
            notifications = notifications,
            updatedAt = updatedAt,
        )

    @Test
    fun `android reports its app version and never an ios version`() {
        assertEquals(
            "Android · S5 Code 0.1.0-alpha.1",
            CloudDevicePresentation.platformLabel(
                device(platform = "android", appVersion = "0.1.0-alpha.1")
            ),
        )
        assertEquals("Android", CloudDevicePresentation.platformLabel(device()))
    }

    @Test
    fun `ios omits the version the relay does not know`() {
        assertEquals(
            "iOS 18",
            CloudDevicePresentation.platformLabel(device(platform = "ios", iosMajorVersion = 18)),
        )
        assertEquals("iOS", CloudDevicePresentation.platformLabel(device(platform = "ios")))
    }

    @Test
    fun `an unknown platform still names itself`() {
        assertEquals("visionos", CloudDevicePresentation.platformLabel(device(platform = "visionos")))
        assertEquals("Unknown device", CloudDevicePresentation.platformLabel(device(platform = " ")))
    }

    @Test
    fun `notification detail lists only the enabled alerts`() {
        assertEquals(
            "Alerts for approvals, failures.",
            CloudDevicePresentation.notificationDetail(
                device(
                    notifications =
                        RelayDeviceNotificationsDto(
                            enabled = true,
                            notifyOnApproval = true,
                            notifyOnFailure = true,
                        )
                )
            ),
        )
    }

    @Test
    fun `notifications on with nothing selected is its own sentence`() {
        assertEquals(
            "Push notifications are on, but no alert types are selected.",
            CloudDevicePresentation.notificationDetail(
                device(notifications = RelayDeviceNotificationsDto(enabled = true))
            ),
        )
    }

    @Test
    fun `notifications off says so rather than listing preferences`() {
        // The relay keeps per-type flags even while push is off, and reporting them
        // would tell the user they will be alerted when they will not.
        assertEquals(
            "Push notifications are off on this device.",
            CloudDevicePresentation.notificationDetail(
                device(
                    notifications =
                        RelayDeviceNotificationsDto(enabled = false, notifyOnApproval = true)
                )
            ),
        )
    }

    @Test
    fun `an unreadable timestamp does not become the epoch`() {
        assertEquals("Update time unavailable", CloudDevicePresentation.updatedLabel("whenever"))
        assertEquals("Update time unavailable", CloudDevicePresentation.updatedLabel(""))
        assertTrue(CloudDevicePresentation.updatedLabel("2026-02-01T10:00:00Z").startsWith("Updated "))
    }

    @Test
    fun `rows are newest first with a stable tiebreak`() {
        val rows =
            CloudDevicePresentation.rows(
                listOf(
                    device(deviceId = "b", label = "Beta", updatedAt = "2026-02-01T10:00:00Z"),
                    device(deviceId = "c", label = "Gamma", updatedAt = "2026-03-01T10:00:00Z"),
                    device(deviceId = "a", label = "Alpha", updatedAt = "2026-02-01T10:00:00Z"),
                )
            )

        assertEquals(listOf("Gamma", "Alpha", "Beta"), rows.map { it.label })
    }
}
