package com.navaq.android.ava

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Handler
import android.os.Looper
import android.os.Process
import android.os.SystemClock
import androidx.annotation.MainThread
import androidx.core.content.ContextCompat
import com.k2fsa.sherpa.onnx.FeatureConfig
import com.k2fsa.sherpa.onnx.OfflineModelConfig
import com.k2fsa.sherpa.onnx.OfflineRecognizer
import com.k2fsa.sherpa.onnx.OfflineRecognizerConfig
import com.k2fsa.sherpa.onnx.OfflineWhisperModelConfig
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.sqrt

/**
 * Fully offline Persian/English speech input.
 *
 * Audio is captured with AudioRecord and decoded locally by the multilingual
 * Whisper model through Sherpa-ONNX. The only network operation is the user's
 * one-time model download in AvaSpeechModelManager; microphone audio and
 * recognized text are never sent to a cloud recognizer.
 */
class AvaSpeechInput(
    context: Context,
    private val onTranscript: (String) -> Unit,
    private val onError: (Int) -> Unit,
    private val onListeningChanged: (Boolean) -> Unit = {}
) {
    private val appContext = context.applicationContext
    private val mainHandler = Handler(Looper.getMainLooper())

    @Volatile
    private var currentSession: CaptureSession? = null

    private class CaptureSession {
        val cancelled = AtomicBoolean(false)

        @Volatile
        var recorder: AudioRecord? = null

        @Volatile
        var thread: Thread? = null
    }

    @MainThread
    fun startListening(languageTag: String, preferOffline: Boolean) {
        check(
            ContextCompat.checkSelfPermission(
                appContext,
                Manifest.permission.RECORD_AUDIO
            ) == PackageManager.PERMISSION_GRANTED
        ) {
            "AVA requires microphone permission before listening"
        }
        check(preferOffline) { "AVA speech recognition is configured for offline use only" }
        check(AvaSpeechModelManager(appContext).isInstalled()) {
            "Install the offline AVA speech model from AVA settings first"
        }

        val previous = currentSession
        stopListening()
        previous?.thread?.let { oldThread ->
            if (oldThread !== Thread.currentThread()) {
                try {
                    oldThread.join(RESTART_JOIN_TIMEOUT_MS)
                } catch (_: InterruptedException) {
                    Thread.currentThread().interrupt()
                }
            }
        }

        val session = CaptureSession()
        currentSession = session
        session.thread = Thread(
            { captureAndRecognize(session, languageTag) },
            "ava-offline-speech"
        ).apply { start() }
    }

    private fun captureAndRecognize(session: CaptureSession, languageTag: String) {
        Process.setThreadPriority(Process.THREAD_PRIORITY_AUDIO)
        var recorder: AudioRecord? = null
        var recognizer: OfflineRecognizer? = null

        try {
            val model = AvaSpeechModelManager(appContext)
            val language = when {
                languageTag.startsWith("fa", ignoreCase = true) -> "fa"
                languageTag.startsWith("en", ignoreCase = true) -> "en"
                else -> ""
            }

            val modelConfig = OfflineModelConfig(
                whisper = OfflineWhisperModelConfig(
                    encoder = model.modelPath("tiny-encoder.int8.onnx"),
                    decoder = model.modelPath("tiny-decoder.int8.onnx"),
                    // Empty language enables Whisper's built-in language detection.
                    language = language,
                    task = "transcribe"
                ),
                tokens = model.modelPath("tiny-tokens.txt"),
                numThreads = RECOGNIZER_THREADS,
                provider = "cpu",
                modelType = "whisper"
            )
            recognizer = OfflineRecognizer(
                config = OfflineRecognizerConfig(
                    featConfig = FeatureConfig(sampleRate = SAMPLE_RATE, featureDim = 80),
                    modelConfig = modelConfig
                )
            )
            if (session.cancelled.get()) return

            val minBuffer = AudioRecord.getMinBufferSize(
                SAMPLE_RATE,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT
            )
            check(minBuffer > 0) {
                "The microphone does not support 16 kHz mono recording"
            }

            recorder = AudioRecord(
                MediaRecorder.AudioSource.VOICE_RECOGNITION,
                SAMPLE_RATE,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT,
                maxOf(minBuffer, BUFFER_BYTES)
            )
            check(recorder.state == AudioRecord.STATE_INITIALIZED) {
                "Could not initialize the microphone"
            }

            session.recorder = recorder
            if (session.cancelled.get()) return

            recorder.startRecording()
            postListening(session, true)

            val pcm = ShortArray(READ_SAMPLES)
            val collected = FloatAccumulator(SAMPLE_RATE * MAX_CAPTURE_SECONDS)
            val preRoll = ShortArray(PRE_ROLL_SAMPLES)
            var preRollWriteIndex = 0
            var preRollCount = 0
            var speechDetected = false
            var silenceSamples = 0
            val startedAt = SystemClock.elapsedRealtime()

            while (
                !session.cancelled.get() &&
                collected.size < SAMPLE_RATE * MAX_CAPTURE_SECONDS
            ) {
                val count = recorder.read(pcm, 0, pcm.size, AudioRecord.READ_BLOCKING)
                if (count <= 0) {
                    if (
                        count == AudioRecord.ERROR_INVALID_OPERATION ||
                        count == AudioRecord.ERROR_BAD_VALUE
                    ) {
                        throw IllegalStateException("Microphone capture failed ($count)")
                    }
                    continue
                }

                var energy = 0.0
                for (index in 0 until count) {
                    val normalized = pcm[index] / 32768.0
                    energy += normalized * normalized
                }
                val rms = sqrt(energy / count)

                if (!speechDetected && rms >= SPEECH_RMS_THRESHOLD) {
                    speechDetected = true
                    // Preserve a short lead-in so the first syllable is not clipped.
                    val firstPreRoll = (preRollWriteIndex - preRollCount + PRE_ROLL_SAMPLES) %
                        PRE_ROLL_SAMPLES
                    for (index in 0 until preRollCount) {
                        val sample = preRoll[(firstPreRoll + index) % PRE_ROLL_SAMPLES]
                        collected.add(sample / 32768.0f)
                    }
                    for (index in 0 until count) {
                        collected.add(pcm[index] / 32768.0f)
                    }
                } else if (speechDetected) {
                    for (index in 0 until count) {
                        collected.add(pcm[index] / 32768.0f)
                    }
                    if (rms < SPEECH_RMS_THRESHOLD) {
                        silenceSamples += count
                    } else {
                        silenceSamples = 0
                    }
                } else {
                    for (index in 0 until count) {
                        preRoll[preRollWriteIndex] = pcm[index]
                        preRollWriteIndex = (preRollWriteIndex + 1) % PRE_ROLL_SAMPLES
                        if (preRollCount < PRE_ROLL_SAMPLES) preRollCount++
                    }
                }

                if (
                    !speechDetected &&
                    SystemClock.elapsedRealtime() - startedAt >= NO_SPEECH_TIMEOUT_MS
                ) {
                    throw AvaSpeechException(ERROR_NO_SPEECH, "No speech was detected")
                }
                if (
                    speechDetected &&
                    silenceSamples >= SAMPLE_RATE * END_SILENCE_SECONDS
                ) {
                    break
                }
            }

            if (session.cancelled.get()) return
            check(speechDetected && collected.size >= MIN_SPEECH_SAMPLES) {
                "No usable speech was captured"
            }

            if (recorder.recordingState == AudioRecord.RECORDSTATE_RECORDING) {
                recorder.stop()
            }
            postListening(session, false)

            val stream = recognizer.createStream()
            try {
                if (language.isEmpty()) {
                    // Whisper detects the spoken language, constrained to AVA's
                    // supported languages to reduce short-utterance misclassification.
                    stream.setOption("language_candidates", "fa,en")
                }
                stream.acceptWaveform(collected.toArray(), SAMPLE_RATE)
                recognizer.decode(stream)

                val result = recognizer.getResult(stream)
                val transcript = result.text.trim()
                if (transcript.isEmpty()) {
                    throw AvaSpeechException(
                        ERROR_NO_TRANSCRIPT,
                        "AVA could not recognize the speech"
                    )
                }
                mainHandler.post {
                    if (!session.cancelled.get()) onTranscript(transcript)
                }
            } finally {
                stream.release()
            }
        } catch (error: Throwable) {
            if (!session.cancelled.get()) {
                val code = (error as? AvaSpeechException)?.code ?: ERROR_RECOGNITION
                mainHandler.post {
                    if (!session.cancelled.get()) onError(code)
                }
            }
        } finally {
            try {
                recorder?.let {
                    if (it.recordingState == AudioRecord.RECORDSTATE_RECORDING) it.stop()
                    it.release()
                }
            } catch (_: Exception) {
                // The capture thread owns final recorder cleanup.
            }
            session.recorder = null
            recognizer?.release()
            if (currentSession === session) {
                currentSession = null
                mainHandler.post { onListeningChanged(false) }
            }
        }
    }

    @MainThread
    fun stopListening() {
        val session = currentSession ?: return
        currentSession = null
        session.cancelled.set(true)
        try {
            session.recorder?.let {
                if (it.recordingState == AudioRecord.RECORDSTATE_RECORDING) it.stop()
            }
        } catch (_: Exception) {
            // The capture thread performs final resource cleanup.
        }
        mainHandler.post { onListeningChanged(false) }
    }

    @MainThread
    fun release() = stopListening()

    private fun postListening(session: CaptureSession, listening: Boolean) {
        mainHandler.post {
            if (currentSession === session && !session.cancelled.get()) {
                onListeningChanged(listening)
            }
        }
    }

    private class FloatAccumulator(initialCapacity: Int) {
        private var values = FloatArray(initialCapacity.coerceAtLeast(1024))

        var size: Int = 0
            private set

        fun add(value: Float) {
            if (size == values.size) values = values.copyOf(values.size * 2)
            values[size++] = value
        }

        fun toArray(): FloatArray = values.copyOf(size)
    }

    private class AvaSpeechException(val code: Int, message: String) : Exception(message)

    companion object {
        private const val SAMPLE_RATE = 16_000
        private const val READ_SAMPLES = 1_024
        private const val BUFFER_BYTES = 8_192
        private const val PRE_ROLL_SAMPLES = SAMPLE_RATE / 5
        private const val MAX_CAPTURE_SECONDS = 20
        private const val END_SILENCE_SECONDS = 1
        private const val NO_SPEECH_TIMEOUT_MS = 10_000L
        private const val RESTART_JOIN_TIMEOUT_MS = 500L
        private const val RECOGNIZER_THREADS = 2
        private const val MIN_SPEECH_SAMPLES = 1_600
        private const val SPEECH_RMS_THRESHOLD = 0.008
        const val ERROR_NO_SPEECH = 1001
        const val ERROR_NO_TRANSCRIPT = 1002
        const val ERROR_RECOGNITION = 1003
    }
}
