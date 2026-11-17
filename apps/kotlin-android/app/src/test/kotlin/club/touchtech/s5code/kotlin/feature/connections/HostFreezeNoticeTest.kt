package club.touchtech.s5code.kotlin.feature.connections

import club.touchtech.s5code.kotlin.transport.wire.ServerEventLoopStallDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class HostFreezeNoticeTest {

    private val stall =
        ServerEventLoopStallDto(detectedAt = "2026-10-20T07:50:37.633Z", durationMs = 12_422, loadAverage = 219.4, cpuCount = 18)

    @Test
    fun `blames an overloaded CPU when the load is above the core count`() {
        assertEquals(
            HostFreezeNotice("leftbook froze for 12s", "Its CPU is overloaded (load 219 on 18 cores)."),
            hostFreezeNotice("leftbook", stall),
        )
    }

    @Test
    fun `blames S5 Code when the CPU had room or the host reports no load`() {
        val busy = "S5 Code on leftbook was too busy to answer."
        assertEquals(busy, hostFreezeNotice("leftbook", stall.copy(loadAverage = 4.0))?.description)
        assertEquals(busy, hostFreezeNotice("leftbook", stall.copy(loadAverage = null))?.description)
        assertNull(hostFreezeNotice("leftbook", null))
    }
}
