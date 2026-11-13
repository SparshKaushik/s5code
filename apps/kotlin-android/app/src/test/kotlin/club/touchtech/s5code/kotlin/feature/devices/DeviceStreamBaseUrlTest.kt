package club.touchtech.s5code.kotlin.feature.devices

import org.junit.Assert.assertEquals
import org.junit.Test

/** Port of the mobile `device-stream-document` base URL cases for an Android viewer. */
class DeviceStreamBaseUrlTest {
    @Test
    fun `iOS targets load under the environment origin and Android targets under a secure placeholder`() {
        assertEquals("http://192.168.1.10:3773/", deviceStreamBaseUrl("ios", "http://192.168.1.10:3773/api/device-hub"))
        assertEquals("https://env.example/", deviceStreamBaseUrl("ios", "https://env.example/t3/api/device-hub"))
        assertEquals("https://device-stream.t3.invalid/", deviceStreamBaseUrl("android", "http://192.168.1.10:3773/api/device-hub"))
    }
}
