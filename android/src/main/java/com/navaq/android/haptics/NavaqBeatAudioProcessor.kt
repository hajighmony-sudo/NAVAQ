package com.navaq.android.haptics

import androidx.media3.common.C
import androidx.media3.common.audio.AudioProcessor
import androidx.media3.common.audio.BaseAudioProcessor
import com.navaq.android.core.NavaqNativeBridge
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.roundToInt
import kotlin.math.sqrt

/**
 * Runs the Rust stateful PCM DSP chain, then observes its output for generic
 * energy onsets. If native DSP cannot be initialized, PCM passes through.
 */
class NavaqBeatAudioProcessor(
    private val onBeat: (Float) -> Unit,
) : BaseAudioProcessor() {
    private var sampleRate = 0
    private var channels = 0
    private var encoding = C.ENCODING_INVALID
    private var samplesPerWindow = 0
    private var samplesInWindow = 0
    private var squareSum = 0.0
    private var noiseFloor: Float? = null
    private var lastBeatAtMs = Long.MIN_VALUE
    private var dspHandle = 0L

    override fun onConfigure(inputAudioFormat: AudioProcessor.AudioFormat): AudioProcessor.AudioFormat {
        if (inputAudioFormat.encoding != C.ENCODING_PCM_FLOAT &&
            inputAudioFormat.encoding != C.ENCODING_PCM_16BIT
        ) {
            return AudioProcessor.AudioFormat.NOT_SET
        }
        releaseDsp()
        sampleRate = inputAudioFormat.sampleRate
        channels = inputAudioFormat.channelCount
        encoding = inputAudioFormat.encoding
        samplesPerWindow = (sampleRate / 50).coerceAtLeast(1) * channels
        dspHandle = NavaqNativeBridge.nativeCreateDsp(sampleRate, channels)
        resetAnalysis()
        return inputAudioFormat
    }

    override fun queueInput(inputBuffer: ByteBuffer) {
        val view = inputBuffer.duplicate().order(ByteOrder.LITTLE_ENDIAN)
        val bytesPerSample = if (encoding == C.ENCODING_PCM_FLOAT) 4 else 2
        val count = view.remaining() / bytesPerSample
        val samples = FloatArray(count)

        when (encoding) {
            C.ENCODING_PCM_FLOAT -> {
                for (i in samples.indices) {
                    val value = view.float
                    samples[i] = if (value.isFinite()) value else 0f
                }
            }
            C.ENCODING_PCM_16BIT -> {
                for (i in samples.indices) samples[i] = view.short.toFloat() / 32768f
            }
        }

        // Native failure is fail-open: preserve playback rather than interrupt audio.
        if (dspHandle != 0L) {
            NavaqNativeBridge.nativeProcessDsp(dspHandle, samples)
        }
        samples.forEach { acceptSample(it.toDouble()) }

        val output = replaceOutputBuffer(count * bytesPerSample).order(ByteOrder.LITTLE_ENDIAN)
        when (encoding) {
            C.ENCODING_PCM_FLOAT -> samples.forEach { output.putFloat(it) }
            C.ENCODING_PCM_16BIT -> samples.forEach {
                output.putShort((it.coerceIn(-1f, 1f) * 32768f).roundToInt().coerceIn(-32768, 32767).toShort())
            }
        }
        inputBuffer.position(inputBuffer.limit())
        output.flip()
    }

    /** Apply a validated Rust EQ configuration; false means it was rejected. */
    fun setEqualizerJson(equalizerJson: String): Boolean =
        dspHandle != 0L && NavaqNativeBridge.nativeSetDspEqualizer(dspHandle, equalizerJson)

    private fun acceptSample(sample: Double) {
        squareSum += sample * sample
        samplesInWindow++
        if (samplesInWindow < samplesPerWindow) return

        val rms = sqrt(squareSum / samplesInWindow).toFloat()
        val floor = noiseFloor ?: rms.coerceAtLeast(0.0001f)
        val strength = rms / floor.coerceAtLeast(0.0001f)
        val now = System.nanoTime() / 1_000_000L
        val isOnset = noiseFloor != null &&
            rms >= MINIMUM_RMS &&
            strength >= SENSITIVITY &&
            (lastBeatAtMs == Long.MIN_VALUE || now - lastBeatAtMs >= REFRACTORY_MS)

        if (isOnset) {
            lastBeatAtMs = now
            onBeat(((strength - SENSITIVITY) / STRENGTH_RANGE).coerceIn(0.15f, 1f))
        }

        val target = if (isOnset) minOf(floor, rms) else rms
        val adaptation = if (target < floor) 0.08f else 0.015f
        noiseFloor = (floor + adaptation * (target - floor)).coerceIn(0.0001f, 1f)
        samplesInWindow = 0
        squareSum = 0.0
    }

    override fun onFlush(streamMetadata: AudioProcessor.StreamMetadata) {
        resetAnalysis()
    }

    override fun onReset() {
        releaseDsp()
        sampleRate = 0
        channels = 0
        encoding = C.ENCODING_INVALID
        samplesPerWindow = 0
        resetAnalysis()
    }

    private fun releaseDsp() {
        if (dspHandle != 0L) {
            NavaqNativeBridge.nativeDestroyDsp(dspHandle)
            dspHandle = 0L
        }
    }

    private fun resetAnalysis() {
        samplesInWindow = 0
        squareSum = 0.0
        noiseFloor = null
        lastBeatAtMs = Long.MIN_VALUE
    }

    private companion object {
        const val MINIMUM_RMS = 0.025f
        const val SENSITIVITY = 2.2f
        const val STRENGTH_RANGE = 5f
        const val REFRACTORY_MS = 120L
    }
}
