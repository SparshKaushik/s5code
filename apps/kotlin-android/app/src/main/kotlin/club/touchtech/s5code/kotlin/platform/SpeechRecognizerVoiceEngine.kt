package club.touchtech.s5code.kotlin.platform

import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import java.util.Locale

/**
 * `SpeechRecognizer` behind the controller's engine interface.
 *
 * Availability is decided at factory time: a device with no speech service
 * simply hides the mic. `stopListening()` is the confirm path — the engine
 * delivers `onResults` with the final hypothesis, which is also why cancel uses
 * `cancel()` rather than `stopListening()` and ignores every callback that may
 * still arrive afterwards.
 */
class SpeechRecognizerVoiceEngine private constructor(
    private val recognizer: SpeechRecognizer,
    private val listener: VoiceEngineListener,
    private val intent: Intent,
) : VoiceEngine {

    override val locale: String = Locale.getDefault().toLanguageTag()

    override fun start() = recognizer.startListening(intent)

    override fun stop() = recognizer.stopListening()

    override fun cancel() = recognizer.cancel()

    override fun destroy() = recognizer.destroy()

    private val recognitionListener =
        object : RecognitionListener {
            override fun onReadyForSpeech(params: Bundle?) = Unit

            override fun onBeginningOfSpeech() = Unit

            override fun onRmsChanged(rmsdB: Float) = Unit

            override fun onBufferReceived(buffer: ByteArray?) = Unit

            override fun onEndOfSpeech() = Unit

            override fun onError(error: Int) {
                when (error) {
                    // Client cancellation is not a failure; the controller that
                    // called cancel() already moved on.
                    SpeechRecognizer.ERROR_CLIENT -> return
                    SpeechRecognizer.ERROR_NO_MATCH,
                    SpeechRecognizer.ERROR_SPEECH_TIMEOUT ->
                        // An empty transcript lands as "No speech was detected."
                        listener.onVoiceResult("")
                    SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS ->
                        listener.onVoiceError("Microphone access is required for voice input.")
                    SpeechRecognizer.ERROR_RECOGNIZER_BUSY ->
                        listener.onVoiceError("Voice transcription is still finishing. Try again shortly.")
                    SpeechRecognizer.ERROR_NETWORK,
                    SpeechRecognizer.ERROR_NETWORK_TIMEOUT ->
                        listener.onVoiceError("Voice transcription needs a network connection.")
                    else ->
                        listener.onVoiceError("Could not transcribe this recording.")
                }
            }

            override fun onResults(results: Bundle?) {
                val transcript =
                    results
                        ?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)
                        ?.firstOrNull()
                        .orEmpty()
                listener.onVoiceResult(transcript)
            }

            override fun onPartialResults(partialResults: Bundle?) {
                val partial =
                    partialResults
                        ?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)
                        ?.firstOrNull()
                        .orEmpty()
                if (partial.isNotBlank()) listener.onVoicePartial(partial)
            }

            override fun onEvent(eventType: Int, params: Bundle?) = Unit
        }

    companion object {
        /** Null on devices with no speech service installed or usable. */
        fun create(context: Context, listener: VoiceEngineListener): SpeechRecognizerVoiceEngine? {
            @Suppress("DEPRECATION")
            if (!SpeechRecognizer.isRecognitionAvailable(context)) return null
            val engine =
                SpeechRecognizerVoiceEngine(
                    recognizer = SpeechRecognizer.createSpeechRecognizer(context),
                    listener = listener,
                    intent =
                        Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH)
                            .putExtra(
                                RecognizerIntent.EXTRA_LANGUAGE_MODEL,
                                RecognizerIntent.LANGUAGE_MODEL_FREE_FORM,
                            )
                            .putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true),
                )
            engine.recognizer.setRecognitionListener(engine.recognitionListener)
            return engine
        }
    }
}
