package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.projectCloneProgressSummary
import club.touchtech.s5code.kotlin.transport.wire.ProjectCloneSnapshotDto
import org.junit.Assert.assertEquals
import org.junit.Test

class ProjectClonesTest {
    private val receiving =
        ProjectCloneSnapshotDto(stage = "receiving", percent = 24, detail = "1.2 MiB | 300 KiB/s")

    @Test
    fun `a stalled clone says Stalled instead of its last speed`() {
        assertEquals(
            "Receiving objects · 24% · 1.2 MiB | 300 KiB/s",
            projectCloneProgressSummary(receiving.toModel()),
        )
        assertEquals(
            "Receiving objects · 24% · Stalled",
            projectCloneProgressSummary(receiving.copy(stalled = true).toModel()),
        )
    }
}
