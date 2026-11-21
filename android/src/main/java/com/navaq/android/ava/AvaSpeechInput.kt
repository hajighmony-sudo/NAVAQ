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
import kotlin.math.sqrt

/**
 * Fully offline Persian/English speech input.
 *
 * Audio is captured from the microphone with AudioRecord, segmented locally
 * using a short silence timeout, and decoded by the multilingual Whisper model
 * through Sherpa-ONNX. No audio or transcript is sent to a cloud recognizer.
 */
class AvaSpeechInput(
    context: Context,
    private val onTranscript: (String) -> Unit,
    private val onError: (Int) -> Unit,
    private val onListeningChanged: (Boolean) -> Unit = {}
) {
    private val appContext = context.applicationContext
    private val mainHandler = Handler(Looper.getMainLooper())

    @Volatile private var stopRequested = false
    @Volatile private var activeRecorder: AudioRecord? = null
    private var captureThread: Thread? = null

    @MainThread
    fun startListening(
        languageTag: String,
        preferOffline: Boolean
    ) {
        check(ContextCompat.checkSelfPermission(appContext, Manifest.permission.RECORD_AUDIO) ==
            PackageManager.PERMISSION_GRANTED) {
            "AVA requires microphone permission before listening"
        }
        check(preferOffline) { "AVA speech recognition is configured for offline use only" }
        check(AvaSpeechModelManager(appContext).isInstalled()) {
            "Install the offline AVA speech model from AVA settings first"
        }

        stopListening()
        stopRequested = false
        captureThread = Thread({ captureAndRecognize(languageTag) }, "ava-offline-speech").apply {
            start()
        }
    }

    private fun captureAndRecognize(languageTag: String) {
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
                    language = language,
                    task = "transcribe"
                ),
                tokens = model.modelPath("tiny-tokens.txt"),
                numThreads = 2,
                provider = "cpu",
                modelType = "whisper"
            )
            recognizer = OfflineRecognizer(
                config = OfflineRecognizerConfig(
                    featConfig = FeatureConfig(sampleRate = SAMPLE_RATE, featureDim = 80),
                    modelConfig = modelConfig
                )
            )

            val minBuffer = AudioRecord.getMinBufferSize(
                SAMPLE_RATE,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT
            )
            check(minBuffer > 0) { "The microphone does not support 16 kHz mono recording" }
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
            activeRecorder = recorder
            recorder.startRecording()
            postListening(true)

            val pcm = ShortArray(READ_SAMPLES)
            val collected = FloatAccumulator(SAMPLE_RATE * MAX_CAPTURE_SECONDS)
            var speechDetected = false
            var silenceSamples = 0
            val startedAt = SystemClock.elapsedRealtime()

            while (!stopRequested && collected.size < SAMPLE_RATE * MAX_CAPTURE_SECONDS) {
                val count = recorder.read(pcm, 0, pcm.size, AudioRecord.READ_BLOCKING)
                if (count <= 0) {
                    if (count == AudioRecord.ERROR_INVALID_OPERATION ||
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
                if (rms >= SPEECH_RMS_THRESHOLD) {
                    speechDetected = true
                    silenceSamples = 0
                } else if (speechDetected) {
                    silenceSamples += count
                }

                if (speechDetected) {
                    for (index in 0 until count) {
                        collected.add(pcm[index] / 32768.0f)
                    }
                }

                if (!speechDetected &&
                    SystemClock.elapsedRealtime() - startedAt >= NO_SPEECH_TIMEOUT_MS
                ) {
                    throw AvaSpeechException(ERROR_NO_SPEECH, "No speech was detected")
                }
                if (speechDetected && silenceSamples >= SAMPLE_RATE * END_SILENCE_SECONDS) {
                    break
                }
            }

            if (stopRequested) return
            check(speechDetected && collected.size >= MIN_SPEECH_SAMPLES) {
                "No usable speech was captured"
            }

            recorder.stop()
            postListening(false)
            val stream = recognizer.createStream()
            try {
                stream.acceptWaveform(collected.toArray(), SAMPLE_RATE)
                recognizer.decode(stream)
                val transcript = recognizer.getResult(stream).text.trim()
                if (transcript.isEmpty()) {
                    throw AvaSpeechException(ERROR_NO_TRANSCRIPT, "AVA could not recognize the speech")
                }
                mainHandler.post { if (!stopRequested) onTranscript(transcript) }
            } finally {
                stream.release()
            }
        } catch (error: Throwable) {
            if (!stopRequested) {
                val code = (error as? AvaSpeechException)?.code ?: ERROR_RECOGNITION
                mainHandler.post { if (!stopRequested) onError(code) }
            }
        } finally {
            try {
                recorder?.let {
                    if (it.recordingState == AudioRecord.RECORDSTATE_RECORDING) it.stop()
                    it.release()
                }
            } catch (_: Exception) {
                // Recorder may already have been stopped by release().
            }
            activeRecorder = null
            recognizer?.release()
            postListening(false)
        }
    }

    @MainThread
    fun stopListening() {
        stopRequested = true
        try {
            activeRecorder?.let {
                if (it.recordingState == AudioRecord.RECORDSTATE_RECORDING) it.stop()
            }
        } catch (_: Exception) {
            // The capture thread performs final resource cleanup.
        }
        postListening(false)
    }

    @MainThread
    fun release() = stopListening()

    private fun postListening(listening: Boolean) {
        mainHandler.post { onListeningChanged(listening) }
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
        private const val MAX_CAPTURE_SECONDS = 20
        private const val END_SILENCE_SECONDS = 1
        private const val NO_SPEECH_TIMEOUT_MS = 10_000L
        private const val MIN_SPEECH_SAMPLES = 1_600
        private const val SPEECH_RMS_THRESHOLD = 0.008
        const val ERROR_NO_SPEECH = 1001
        const val ERROR_NO_TRANSCRIPT = 1002
        const val ERROR_RECOGNITION = 1003
    }
}
