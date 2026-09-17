package club.touchtech.s5code.kotlin.platform

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class VoiceInputControllerTest {

    private companion object {
        fun snapshot(
            text: String,
            start: Int = text.length,
            end: Int = start,
            revision: Long = 0,
        ) = VoiceDraftSnapshot("owner", text, start, end, revision)
    }

    /* ── resolveTranscriptCommit ─────────────────────────────────────── */

    @Test
    fun `a transcript replaces the selection`() {
        val result =
            resolveTranscriptCommit(
                captured = snapshot("hello world", start = 0, end = 5),
                current = snapshot("hello world", start = 0, end = 5),
                transcript = "goodbye",
                locale = "en-US",
            )
        assertEquals(
            TranscriptCommitResult.Commit(text = "goodbye world", cursor = 7),
            result,
        )
    }

    @Test
    fun `a caret at a word boundary pads the insertion`() {
        val result =
            resolveTranscriptCommit(
                captured = snapshot("hello world", start = 5, end = 5),
                current = snapshot("hello world", start = 5, end = 5),
                transcript = "there",
                locale = "en",
            )
        // The caret sits between two non-space characters, so both sides get
        // the English word boundary space.
        assertEquals(
            TranscriptCommitResult.Commit(text = "hello there world", cursor = 11),
            result,
        )
    }

    @Test
    fun `a caret at end of text needs no boundary`() {
        val result =
            resolveTranscriptCommit(
                captured = snapshot("hello "),
                current = snapshot("hello "),
                transcript = "world",
                locale = "en-US",
            )
        assertEquals(TranscriptCommitResult.Commit(text = "hello world", cursor = 11), result)
    }

    @Test
    fun `non-English locales skip the boundary spaces`() {
        val result =
            resolveTranscriptCommit(
                captured = snapshot("helloworld", start = 5, end = 5),
                current = snapshot("helloworld", start = 5, end = 5),
                transcript = "there",
                locale = "ja-JP",
            )
        assertEquals(TranscriptCommitResult.Commit(text = "hellothereworld", cursor = 10), result)
    }

    @Test
    fun `an edited draft refuses the transcript`() {
        val captured = snapshot("hello")
        val edited = snapshot("hello!", revision = 1)
        assertEquals(
            TranscriptCommitResult.Stale,
            resolveTranscriptCommit(captured, edited, "there", "en"),
        )
        assertEquals(
            TranscriptCommitResult.Stale,
            resolveTranscriptCommit(captured, snapshot("hello", revision = 1), "there", "en"),
        )
        assertEquals(
            TranscriptCommitResult.Stale,
            resolveTranscriptCommit(
                captured,
                snapshot("hello").copy(ownerKey = "other"),
                "there",
                "en",
            ),
        )
    }

    @Test
    fun `a blank transcript is empty rather than committed`() {
        val captured = snapshot("hello")
        assertEquals(
            TranscriptCommitResult.Empty,
            resolveTranscriptCommit(captured, snapshot("hello"), "   ", "en"),
        )
    }

    /* ── Controller ──────────────────────────────────────────────────── */

    private class FakeEngine(
        val listener: VoiceEngineListener,
        override val locale: String = "en-US",
    ) : VoiceEngine {
        var started = false
        var stopped = false
        var cancelled = false
        var destroyed = false

        override fun start() {
            started = true
        }

        override fun stop() {
            stopped = true
        }

        override fun cancel() {
            cancelled = true
        }

        override fun destroy() {
            destroyed = true
        }

        fun result(text: String) = listener.onVoiceResult(text)

        fun partial(text: String) = listener.onVoicePartial(text)

        fun fail(message: String) = listener.onVoiceError(message)
    }

    private class Harness(var draft: VoiceDraftSnapshot? = snapshot("hello ")) {
        val engines = mutableListOf<FakeEngine>()
        var permission = VoicePermissionResult(granted = true, canAskAgain = true)
        var committed: Pair<String, Int>? = null
    }

    private fun controllerFor(
        harness: Harness,
        scope: kotlinx.coroutines.CoroutineScope,
    ) = VoiceInputController(
        scope = scope,
        engineFactory = { listener ->
            FakeEngine(listener).also { harness.engines += it }
        },
        requestPermission = { harness.permission },
        readDraft = { harness.draft },
        commitDraft = { text, cursor -> harness.committed = text to cursor },
    )

    @Test
    fun `a session records and commits at the caret`() = runTest {
        VoiceInputController.resetForTests()
        val harness = Harness()
        val controller = controllerFor(harness, backgroundScope)

        controller.start()
        assertEquals(VoiceInputPhase.Recording, controller.state.value.phase)
        harness.engines.single().partial("the")
        assertEquals("the", controller.state.value.partial)

        controller.confirm()
        assertEquals(VoiceInputPhase.Transcribing, controller.state.value.phase)
        assertTrue(harness.engines.single().stopped)

        harness.engines.single().result("world")
        assertEquals("hello world" to 11, harness.committed)
        assertEquals(VoiceInputPhase.Idle, controller.state.value.phase)
        assertTrue(harness.engines.single().destroyed)
    }

    @Test
    fun `cancel discards without committing`() = runTest {
        VoiceInputController.resetForTests()
        val harness = Harness()
        val controller = controllerFor(harness, backgroundScope)

        controller.start()
        controller.cancel()
        assertEquals(VoiceInputPhase.Idle, controller.state.value.phase)
        assertTrue(harness.engines.single().cancelled)
        assertNull(harness.committed)
    }

    @Test
    fun `a stale draft turns the result into a retryable error`() = runTest {
        VoiceInputController.resetForTests()
        val harness = Harness()
        val controller = controllerFor(harness, backgroundScope)

        controller.start()
        // An edit mid-recording bumps the revision the snapshot captured.
        harness.draft = harness.draft?.copy(text = "hello?", revision = 1)
        controller.confirm()
        harness.engines.single().result("world")
        assertEquals(VoiceInputPhase.Error, controller.state.value.phase)
        assertEquals(VoiceInputErrorAction.Retry, controller.state.value.errorAction)
        assertNull(harness.committed)

        controller.dismissError()
        assertEquals(VoiceInputPhase.Idle, controller.state.value.phase)
    }

    @Test
    fun `no speech is a retryable error, not an insertion`() = runTest {
        VoiceInputController.resetForTests()
        val harness = Harness()
        val controller = controllerFor(harness, backgroundScope)

        controller.start()
        controller.confirm()
        harness.engines.single().result("  ")
        assertEquals(VoiceInputPhase.Error, controller.state.value.phase)
        assertEquals("No speech was detected.", controller.state.value.error)
    }

    @Test
    fun `denied permission carries a retry or settings action`() = runTest {
        VoiceInputController.resetForTests()
        val harness = Harness()
        harness.permission = VoicePermissionResult(granted = false, canAskAgain = false)
        val controller = controllerFor(harness, backgroundScope)

        controller.start()
        assertEquals(VoiceInputPhase.Error, controller.state.value.phase)
        assertEquals(VoiceInputErrorAction.Settings, controller.state.value.errorAction)
    }

    @Test
    fun `the five-minute cap stops capture`() = runTest {
        VoiceInputController.resetForTests()
        val harness = Harness()
        val controller = controllerFor(harness, backgroundScope)

        controller.start()
        advanceTimeBy(VOICE_RECORDING_LIMIT_SECONDS * 1000L + 1)
        assertEquals(VoiceInputPhase.Transcribing, controller.state.value.phase)
        assertTrue(harness.engines.single().stopped)
    }

    @Test
    fun `elapsed time ticks while recording`() = runTest {
        VoiceInputController.resetForTests()
        val harness = Harness()
        val controller = controllerFor(harness, backgroundScope)

        controller.start()
        advanceTimeBy(65_001)
        assertEquals(65, controller.state.value.elapsedSeconds)
    }
}
