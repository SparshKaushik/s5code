package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.WorktreeSetupSnapshot
import club.touchtech.s5code.kotlin.model.WorktreeSetupStage
import club.touchtech.s5code.kotlin.transport.wire.ThreadActivityDto
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.addJsonObject
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test

/**
 * `resolveVisibleWorktreeSetup` / `findRecordedWorktreeSetup` from
 * `client-runtime/worktreeSetup.ts`: the live stream wins on freshness, a
 * running setup always shows, and a clean finish leaves no trace once the turn
 * it prepared is live — while failures stay reachable.
 */
class WorktreeSetupTest {

    private fun snapshot(
        phase: String,
        sequence: Int = 0,
        threadId: String = "t-1",
        failedStage: Boolean = false,
    ) =
        WorktreeSetupSnapshot(
            threadId = threadId,
            phase = phase,
            sequence = sequence,
            stages =
                listOf(
                    WorktreeSetupStage(
                        id = "setup-script",
                        status = if (failedStage) "failed" else "done",
                    )
                ),
        )

    private fun setupActivity(threadId: String, phase: String, sequence: Int) =
        ThreadActivityDto(
            id = "worktree-setup:$threadId",
            tone = "activity",
            kind = "worktree-setup",
            summary = "Worktree setup",
            payload =
                buildJsonObject {
                    put("threadId", threadId)
                    put("phase", phase)
                    put("startedAt", "2026-03-01T12:00:00Z")
                    put("sequence", sequence)
                    putJsonArray("stages") {}
                },
            sequence = sequence.toLong(),
            createdAt = "2026-03-01T12:00:00Z",
        )

    @Test
    fun `the live stream wins while its sequence covers the record`() {
        val live = snapshot("running", sequence = 4)
        val recorded = snapshot("done", sequence = 4)
        assertSame(live, resolveVisibleWorktreeSetup(live, recorded, turnStarted = true, followUpSent = false))
    }

    @Test
    fun `a stale live frame loses to a newer record`() {
        val live = snapshot("running", sequence = 2)
        val recorded = snapshot("done", sequence = 4)
        assertSame(recorded, resolveVisibleWorktreeSetup(live, recorded, turnStarted = false, followUpSent = false))
    }

    @Test
    fun `a running setup always shows`() {
        val running = snapshot("running")
        assertSame(
            running,
            resolveVisibleWorktreeSetup(null, running, turnStarted = true, followUpSent = false),
        )
    }

    @Test
    fun `a follow-up turn retires the setup whatever its outcome`() {
        val failed = snapshot("failed")
        assertNull(
            resolveVisibleWorktreeSetup(null, failed, turnStarted = true, followUpSent = true)
        )
    }

    @Test
    fun `a clean finish disappears once the turn is live`() {
        val done = snapshot("done")
        assertNull(resolveVisibleWorktreeSetup(null, done, turnStarted = true, followUpSent = false))
        // Before the turn is live the finish stays: the card must not collapse
        // in the handoff gap between setup done and the first turn event.
        assertSame(
            done,
            resolveVisibleWorktreeSetup(null, done, turnStarted = false, followUpSent = false),
        )
    }

    @Test
    fun `a failed script stage keeps the finished card`() {
        val done = snapshot("done", failedStage = true)
        assertSame(
            done,
            resolveVisibleWorktreeSetup(null, done, turnStarted = true, followUpSent = false),
        )
    }

    @Test
    fun `the newest worktree-setup activity for the thread is the record`() {
        val recorded =
            findRecordedWorktreeSetup(
                listOf(
                    setupActivity("t-1", "running", 1),
                    setupActivity("t-2", "running", 2),
                    setupActivity("t-1", "done", 3),
                ),
                "t-1",
            )
        assertEquals("done", recorded?.phase)
        assertEquals(3, recorded?.sequence)
    }

    @Test
    fun `a malformed or foreign record is skipped`() {
        assertNull(
            findRecordedWorktreeSetup(
                listOf(
                    ThreadActivityDto(
                        id = "x",
                        tone = "activity",
                        kind = "worktree-setup",
                        summary = "nope",
                        payload = buildJsonObject { put("phase", "running") },
                        sequence = 1,
                        createdAt = "2026-03-01T12:00:00Z",
                    )
                ),
                "t-1",
            )
        )
    }
}
