package club.touchtech.s5code.kotlin.platform

import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/** `VOICE_RECORDING_LIMIT_SECONDS` in the RN client: five minutes, hard cap. */
const val VOICE_RECORDING_LIMIT_SECONDS = 300

enum class VoiceInputPhase {
    Idle,
    Preparing,
    Recording,
    Transcribing,
    Error,
}

enum class VoiceInputErrorAction {
    Retry,
    Settings,
}

/** `VoiceInputState` in `packages/client-runtime/src/voice-input/controller.ts`. */
data class VoiceInputState(
    val phase: VoiceInputPhase = VoiceInputPhase.Idle,
    val error: String? = null,
    val errorAction: VoiceInputErrorAction? = null,
    /** Latest streaming partial, shown as the status line while recording. */
    val partial: String? = null,
    val elapsedSeconds: Int = 0,
)

val VoiceInputState.active: Boolean
    get() =
        phase == VoiceInputPhase.Preparing ||
            phase == VoiceInputPhase.Recording ||
            phase == VoiceInputPhase.Transcribing

/**
 * The draft as it stood when recording began. [revision] changes on every
 * draft edit, so a transcript that lands after the user typed mid-recording is
 * stale and must not clobber those edits — `VoiceDraftSnapshot` in the RN
 * client.
 */
data class VoiceDraftSnapshot(
    val ownerKey: String,
    val text: String,
    val selectionStart: Int,
    val selectionEnd: Int,
    val revision: Long,
)

sealed interface TranscriptCommitResult {
    data class Commit(val text: String, val cursor: Int) : TranscriptCommitResult

    data object Stale : TranscriptCommitResult

    data object Empty : TranscriptCommitResult
}

private val TRANSCRIPT_LEFT_BOUNDARY = Regex("[A-Za-z0-9.!?,:;)\\]}'\"]")
private val TRANSCRIPT_RIGHT_BOUNDARY = Regex("[A-Za-z0-9(\\[{']")

/**
 * Where a finished transcript lands in the draft — `resolveTranscriptCommit`
 * in the RN client, verbatim.
 *
 * English spacing rules: a transcript that arrives at a caret sitting on a word
 * boundary gets a space inserted so "hello|world" becomes "hello there world"
 * rather than "hellothereworld". Other locales skip it because their spacing
 * conventions differ.
 */
fun resolveTranscriptCommit(
    captured: VoiceDraftSnapshot,
    current: VoiceDraftSnapshot?,
    transcript: String,
    locale: String,
): TranscriptCommitResult {
    if (
        current == null ||
            current.ownerKey != captured.ownerKey ||
            current.text != captured.text ||
            current.revision != captured.revision
    ) {
        return TranscriptCommitResult.Stale
    }

    val replacement = transcript.trim()
    if (replacement.isEmpty()) return TranscriptCommitResult.Empty

    val isEmptySelection = captured.selectionStart == captured.selectionEnd
    val normalizedLocale = locale.replace('_', '-').lowercase()
    val usesEnglishSpacing = normalizedLocale == "en" || normalizedLocale.startsWith("en-")
    var insertion = replacement
    if (isEmptySelection && usesEnglishSpacing) {
        val left = captured.text.getOrNull(captured.selectionStart - 1)
        val right = captured.text.getOrNull(captured.selectionStart)
        val leftNeedsBoundary =
            left != null &&
                TRANSCRIPT_LEFT_BOUNDARY.matches(left.toString()) &&
                (right == null || right.isWhitespace())
        val rightNeedsBoundary =
            right != null &&
                TRANSCRIPT_RIGHT_BOUNDARY.matches(right.toString()) &&
                (left == null || left.isWhitespace())
        insertion =
            (if (leftNeedsBoundary) " " else "") +
                replacement +
                (if (rightNeedsBoundary) " " else "")
    }

    val safeStart = captured.selectionStart.coerceIn(0, captured.text.length)
    val safeEnd = captured.selectionEnd.coerceIn(safeStart, captured.text.length)
    val text =
        captured.text.substring(0, safeStart) + insertion + captured.text.substring(safeEnd)
    return TranscriptCommitResult.Commit(text = text, cursor = safeStart + insertion.length)
}

/**
 * What the composer's toolbar shows for one voice state — a port of
 * `resolveVoiceComposerPresentation` in `voiceInputPresentation.ts`.
 */
data class VoiceComposerPresentation(
    val showsCancel: Boolean,
    val trailingAction: TrailingAction,
    val showsSend: Boolean,
    val statusIsError: Boolean,
    val statusLabel: String?,
    val confirmationEnabled: Boolean,
) {
    enum class TrailingAction {
        Mic,
        Confirm,
    }
}

fun resolveVoiceComposerPresentation(state: VoiceInputState): VoiceComposerPresentation =
    when (state.phase) {
        VoiceInputPhase.Idle ->
            VoiceComposerPresentation(
                showsCancel = false,
                trailingAction = VoiceComposerPresentation.TrailingAction.Mic,
                showsSend = true,
                statusIsError = false,
                statusLabel = null,
                confirmationEnabled = false,
            )
        VoiceInputPhase.Error ->
            VoiceComposerPresentation(
                showsCancel = false,
                trailingAction = VoiceComposerPresentation.TrailingAction.Mic,
                showsSend = true,
                statusIsError = true,
                statusLabel = state.error,
                confirmationEnabled = false,
            )
        VoiceInputPhase.Preparing ->
            VoiceComposerPresentation(
                showsCancel = true,
                trailingAction = VoiceComposerPresentation.TrailingAction.Confirm,
                showsSend = false,
                statusIsError = false,
                statusLabel = "Preparing",
                confirmationEnabled = false,
            )
        VoiceInputPhase.Recording -> {
            val seconds = state.elapsedSeconds.coerceAtLeast(0)
            VoiceComposerPresentation(
                showsCancel = true,
                trailingAction = VoiceComposerPresentation.TrailingAction.Confirm,
                showsSend = false,
                statusIsError = false,
                statusLabel =
                    "Recording ${seconds / 60}:${(seconds % 60).toString().padStart(2, '0')}",
                confirmationEnabled = true,
            )
        }
        VoiceInputPhase.Transcribing ->
            VoiceComposerPresentation(
                showsCancel = true,
                trailingAction = VoiceComposerPresentation.TrailingAction.Confirm,
                showsSend = false,
                statusIsError = false,
                statusLabel = "Transcribing",
                confirmationEnabled = false,
            )
    }

/** Result of the microphone permission request, resolved by the UI layer. */
data class VoicePermissionResult(val granted: Boolean, val canAskAgain: Boolean)

/**
 * The speech engine the controller drives. One call-site implementation —
 * `SpeechRecognizerVoiceEngine` — and fakes in tests; the controller owns the
 * lifecycle (prepare → record → finish) the same way `VoiceInputController`
 * does in the RN client.
 */
interface VoiceEngine {
    /** BCP-47 tag used for transcript spacing rules. */
    val locale: String

    fun start()

    /** Stops capture and asks the engine to deliver its final result. */
    fun stop()

    /** Abandons capture without a result. */
    fun cancel()

    fun destroy()
}

interface VoiceEngineListener {
    fun onVoicePartial(text: String) {}

    fun onVoiceResult(text: String)

    /** A fatal engine error. [message] is already user-readable. */
    fun onVoiceError(message: String)
}

/**
 * One dictation session per composer, one at a time process-wide.
 *
 * Ported from `VoiceInputController` in `packages/client-runtime`: the phases
 * are idle → preparing → recording → transcribing, a new session cancels the
 * old via a process-wide latch, and the transcript commits only when the draft
 * is untouched since recording began (see [resolveTranscriptCommit]).
 *
 * Android differs from RN in one way that simplifies this: `SpeechRecognizer`
 * streams partials and returns its final transcript on the same call, so
 * "transcribing" is the gap between stop and `onVoiceResult`, not a second
 * service call.
 */
class VoiceInputController(
    private val scope: CoroutineScope,
    private val engineFactory: (VoiceEngineListener) -> VoiceEngine?,
    private val requestPermission: suspend () -> VoicePermissionResult,
    private val readDraft: () -> VoiceDraftSnapshot?,
    private val commitDraft: (text: String, cursor: Int) -> Unit,
) {
    private val _state = MutableStateFlow(VoiceInputState())
    val state: StateFlow<VoiceInputState> = _state.asStateFlow()

    private val lock = Any()
    private var operationToken = 0
    private var engine: VoiceEngine? = null
    private var captured: VoiceDraftSnapshot? = null
    private var capJob: Job? = null
    private var tickJob: Job? = null
    private var finishing = false

    private val listener =
        object : VoiceEngineListener {
            override fun onVoicePartial(text: String) {
                synchronized(lock) {
                    if (_state.value.phase != VoiceInputPhase.Recording) return
                }
                _state.value = _state.value.copy(partial = text.ifBlank { null })
            }

            override fun onVoiceResult(text: String) {
                commitResult(text)
            }

            override fun onVoiceError(message: String) {
                synchronized(lock) {
                    if (!_state.value.active && _state.value.phase != VoiceInputPhase.Error) return
                }
                fail(message, VoiceInputErrorAction.Retry)
            }
        }

    suspend fun start() {
        val phase = synchronized(lock) { _state.value.phase }
        if (phase != VoiceInputPhase.Idle && phase != VoiceInputPhase.Error) return
        val initiating = readDraft()
        if (initiating == null) {
            fail("This draft is no longer available.", VoiceInputErrorAction.Retry)
            return
        }
        if (!acquireSession()) {
            fail("Another voice recording is already active.", VoiceInputErrorAction.Retry)
            return
        }
        val token = synchronized(lock) { ++operationToken }
        setState(VoiceInputState(phase = VoiceInputPhase.Preparing))
        try {
            val nextEngine = engineFactory(listener)
            if (nextEngine == null) {
                fail("Voice transcription is not available on this device.", null)
                return
            }
            val permission = requestPermission()
            if (!isCurrent(token)) return
            if (!permission.granted) {
                fail(
                    "Microphone access is required for voice input.",
                    if (permission.canAskAgain) {
                        VoiceInputErrorAction.Retry
                    } else {
                        VoiceInputErrorAction.Settings
                    },
                )
                return
            }
            engine = nextEngine

            val capturedNow = readDraft()
            if (capturedNow == null || capturedNow.ownerKey != initiating.ownerKey) {
                fail("This draft is no longer available.", VoiceInputErrorAction.Retry)
                return
            }
            captured = capturedNow
            nextEngine.start()
            if (!isCurrent(token)) return

            setState(VoiceInputState(phase = VoiceInputPhase.Recording))
            // The cap is real recording time, not user intent: `record`'s
            // forDuration in RN ends the capture, and SpeechRecognizer keeps
            // listening without one.
            capJob =
                scope.launch {
                    delay(VOICE_RECORDING_LIMIT_SECONDS * 1000L)
                    if (isCurrent(token)) stopCapture()
                }
            tickJob =
                scope.launch {
                    while (true) {
                        delay(1_000L)
                        synchronized(lock) {
                            if (_state.value.phase != VoiceInputPhase.Recording) return@launch
                            _state.value =
                                _state.value.copy(elapsedSeconds = _state.value.elapsedSeconds + 1)
                        }
                    }
                }
        } finally {
            if (isCurrent(token)) {
                if (_state.value.phase == VoiceInputPhase.Error) releaseResources()
            } else if (!finishing) {
                releaseResources()
            }
        }
    }

    /** Confirms the recording: the engine stops and delivers its transcript. */
    fun confirm() = stopCapture()

    /**
     * Ends capture without discarding anything. Entering transcribing here —
     * rather than when the engine answers — is what the toolbar shows while the
     * recognizer finishes.
     */
    private fun stopCapture() {
        synchronized(lock) {
            if (_state.value.phase != VoiceInputPhase.Recording || finishing) return
            finishing = true
        }
        setState(VoiceInputState(phase = VoiceInputPhase.Transcribing))
        engine?.let { engine ->
            runCatching { engine.stop() }
                .onFailure {
                    finishing = false
                    fail("Could not finish voice recording.", VoiceInputErrorAction.Retry)
                    releaseResources()
                }
        }
    }

    fun cancel() {
        when (synchronized(lock) { _state.value.phase }) {
            VoiceInputPhase.Idle -> return
            VoiceInputPhase.Error -> setState(VoiceInputState())
            VoiceInputPhase.Preparing,
            VoiceInputPhase.Transcribing -> {
                invalidateOperation()
                setState(VoiceInputState())
                releaseResources()
            }
            VoiceInputPhase.Recording -> discardRecording(null)
        }
    }

    fun dismissError() {
        if (_state.value.phase == VoiceInputPhase.Error) setState(VoiceInputState())
    }

    /** Drops any in-flight session; called when the owning field leaves composition. */
    fun dispose() {
        cancel()
        releaseResources()
    }

    private fun commitResult(transcript: String) {
        val token: Int
        synchronized(lock) {
            val phase = _state.value.phase
            if (phase != VoiceInputPhase.Recording && phase != VoiceInputPhase.Transcribing) {
                finishing = false
                return
            }
            token = operationToken
        }
        try {
            val capturedNow = captured
            if (!isCurrent(token)) return
            if (capturedNow == null) {
                fail("Could not finish voice recording.", VoiceInputErrorAction.Retry)
                return
            }
            val result =
                resolveTranscriptCommit(
                    capturedNow,
                    readDraft(),
                    transcript,
                    engine?.locale ?: "en",
                )
            when (result) {
                TranscriptCommitResult.Stale ->
                    fail(
                        "The draft changed while voice input was running. The transcript was not added.",
                        VoiceInputErrorAction.Retry,
                    )
                TranscriptCommitResult.Empty ->
                    fail("No speech was detected.", VoiceInputErrorAction.Retry)
                is TranscriptCommitResult.Commit -> {
                    commitDraft(result.text, result.cursor)
                    setState(VoiceInputState())
                }
            }
        } finally {
            finishing = false
            releaseResources()
        }
    }

    private fun discardRecording(error: String?) {
        invalidateOperation()
        setState(
            if (error != null) {
                VoiceInputState(
                    phase = VoiceInputPhase.Error,
                    error = error,
                    errorAction = VoiceInputErrorAction.Retry,
                )
            } else {
                VoiceInputState()
            }
        )
        runCatching { engine?.cancel() }
        releaseResources()
    }

    private fun releaseResources() {
        runCatching { engine?.destroy() }
        engine = null
        captured = null
        capJob?.cancel()
        capJob = null
        tickJob?.cancel()
        tickJob = null
        releaseSession()
    }

    private fun invalidateOperation() {
        synchronized(lock) { operationToken += 1 }
    }

    private fun isCurrent(token: Int): Boolean =
        synchronized(lock) { token == operationToken }

    private fun fail(message: String, action: VoiceInputErrorAction?) {
        setState(
            VoiceInputState(phase = VoiceInputPhase.Error, error = message, errorAction = action)
        )
    }

    private fun setState(next: VoiceInputState) {
        _state.value = next
    }

    companion object {
        /** RN's module-level `activeSession`: one recording at a time, process-wide. */
        private val activeSession = AtomicBoolean(false)

        private fun acquireSession(): Boolean = activeSession.compareAndSet(false, true)

        private fun releaseSession() {
            activeSession.set(false)
        }

        internal fun resetForTests() {
            activeSession.set(false)
        }
    }
}
