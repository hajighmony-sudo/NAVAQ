package com.navaq.android.haptics

import androidx.media3.common.C
import androidx.media3.common.audio.AudioProcessor
import androidx.media3.common.audio.BaseAudioProcessor
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.sqrt

/**
 * Observes decoded PCM on Media3's audio-processing path and forwards generic
 * energy onsets. The input bytes are copied unchanged to the output buffer.
 *
 * This is deliberately not an instrument classifier: a transient may be a beat,
 * percussion hit, or another sudden change in energy.
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

    override fun onConfigure(inputAudioFormat: AudioProcessor.AudioFormat): AudioProcessor.AudioFormat {
        if (inputAudioFormat.encoding != C.ENCODING_PCM_FLOAT &&
            inputAudioFormat.encoding != C.ENCODING_PCM_16BIT
        ) {
            return AudioProcessor.AudioFormat.NOT_SET
        }
        sampleRate = inputAudioFormat.sampleRate
        channels = inputAudioFormat.channelCount
        encoding = inputAudioFormat.encoding
        samplesPerWindow = (sampleRate / 50).coerceAtLeast(1) * channels
        return inputAudioFormat
    }

    override fun queueInput(inputBuffer: ByteBuffer) {
        val view = inputBuffer.duplicate().order(ByteOrder.LITTLE_ENDIAN)
        when (encoding) {
            C.ENCODING_PCM_FLOAT -> {
                while (view.remaining() >= Float.SIZE_BYTES) {
                    val sample = view.float
                    acceptSample(if (sample.isFinite()) sample.toDouble() else 0.0)
                }
            }
            C.ENCODING_PCM_16BIT -> {
                while (view.remaining() >= Short.SIZE_BYTES) {
                    acceptSample(view.short.toDouble() / Short.MAX_VALUE.toDouble())
                }
            }
        }

        val output = replaceOutputBuffer(inputBuffer.remaining())
        output.put(inputBuffer)
        output.flip()
    }

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
        sampleRate = 0
        channels = 0
        encoding = C.ENCODING_INVALID
        samplesPerWindow = 0
        resetAnalysis()
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
