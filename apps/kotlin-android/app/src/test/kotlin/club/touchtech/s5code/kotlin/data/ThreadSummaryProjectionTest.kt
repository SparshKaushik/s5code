package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.transport.wire.LatestTurnDto
import club.touchtech.s5code.kotlin.transport.wire.ModelSelectionDto
import club.touchtech.s5code.kotlin.transport.wire.SessionDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadShellDto
import club.touchtech.s5code.kotlin.transport.wire.TitleRegenerationDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ThreadSummaryProjectionTest {

    @Test
    fun `title regeneration state is projected from the server shell`() {
        val summary =
            threadSummaryFrom(
                environmentId = EnvironmentId("env"),
                shell =
                    ThreadShellDto(
                        id = "thread",
                        projectId = "project",
                        title = "A title",
                        modelSelection = ModelSelectionDto(instanceId = "codex", model = "o3"),
                        titleRegeneration = TitleRegenerationDto(requestId = "regen-1"),
                    ),
                driverFor = { ProviderInstance(it, "codex") },
                nowMillis = 0L,
            )

        assertTrue(summary.titleRegenerating)
    }

    @Test
    fun `running shell retains parsed start time for a live Home timer`() {
        val summary =
            threadSummaryFrom(
                EnvironmentId("env"),
                ThreadShellDto(
                    id = "thread",
                    projectId = "project",
                    title = "Working",
                    session = SessionDto(status = "running"),
                    latestTurn =
                        LatestTurnDto(
                            turnId = "turn",
                            state = "running",
                            startedAt = "2026-08-30T12:00:00Z",
                        ),
                ),
                { ProviderInstance(it, "codex") },
                java.time.Instant.parse("2026-08-30T12:01:02Z").toEpochMilli(),
            )

        assertEquals(java.time.Instant.parse("2026-08-30T12:00:00Z").toEpochMilli(), summary.activeTurnStartedAtMillis)
        assertEquals("1m 2s", summary.elapsedLabel)
    }

    @Test
    fun `stale running turn does not keep a Home timer alive after session stops`() {
        val summary =
            threadSummaryFrom(
                EnvironmentId("env"),
                ThreadShellDto(
                    id = "thread",
                    projectId = "project",
                    title = "Idle",
                    session = SessionDto(status = "idle"),
                    latestTurn =
                        LatestTurnDto(
                            turnId = "turn",
                            state = "running",
                            startedAt = "2026-08-30T12:00:00Z",
                        ),
                ),
                { ProviderInstance(it, "codex") },
                java.time.Instant.parse("2026-08-30T12:01:02Z").toEpochMilli(),
            )

        assertNull(summary.activeTurnStartedAtMillis)
    }

    @Test
    fun `missing server title regeneration state projects as idle`() {
        val summary =
            threadSummaryFrom(
                EnvironmentId("env"),
                ThreadShellDto(id = "thread", projectId = "project", title = "A title"),
                { ProviderInstance(it, "codex") },
                0L,
            )

        assertFalse(summary.titleRegenerating)
    }
}
