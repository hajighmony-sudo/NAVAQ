package com.navaq.android.haptics

import android.content.Context
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import java.util.concurrent.atomic.AtomicLong

/**
 * Small Android actuator adapter for beat/onset events produced by an audio
 * analysis layer. It is deliberately opt-in and rate-limited: callers should
 * send detected musical events, never raw audio samples.
 */
class NavaqHaptics(context: Context) {
    enum class Trigger {
        BASS, SNARE, HI_HAT, VOCAL, BEAT
    }

    private val appContext = context.applicationContext
    private val vibrator: Vibrator? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        appContext.getSystemService(VibratorManager::class.java)?.defaultVibrator
    } else {
        @Suppress("DEPRECATION")
        appContext.getSystemService(Vibrator::class.java)
    }
    private val lastPulseAt = AtomicLong(0L)

    @Volatile
    var enabled: Boolean = false
        private set

    /** Explicit user preference. Disabled by default and never enabled implicitly. */
    fun setEnabled(value: Boolean) {
        enabled = value && vibrator?.hasVibrator() == true
        if (!enabled) vibrator?.cancel()
    }

    /**
     * Map a classified audio event to a short haptic pulse.
     * The audio analyzer / Rust host must call this; no microphone permission
     * or background audio capture is used by this adapter.
     */
    fun onAudioEvent(trigger: Trigger, strength: Float) {
        if (!enabled || !strength.isFinite()) return
        val now = System.nanoTime() / 1_000_000L
        val previous = lastPulseAt.get()
        if (now - previous < MIN_INTERVAL_MS || !lastPulseAt.compareAndSet(previous, now)) return

        val base = when (trigger) {
            Trigger.BASS -> 92
            Trigger.SNARE -> 52
            Trigger.HI_HAT -> 20
            Trigger.VOCAL -> 45
            Trigger.BEAT -> 34
        }
        val duration = base.coerceIn(MIN_DURATION_MS, MAX_DURATION_MS)
        val scaled = (strength.coerceIn(0f, 1f) * MAX_AMPLITUDE).toInt()
        val amplitude = scaled.coerceIn(MIN_AMPLITUDE, MAX_AMPLITUDE)
        val target = vibrator ?: return

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            target.vibrate(VibrationEffect.createOneShot(duration.toLong(), amplitude))
        } else {
            @Suppress("DEPRECATION")
            target.vibrate(duration.toLong())
        }
    }

    fun release() {
        vibrator?.cancel()
        enabled = false
    }

    companion object {
        private const val MIN_INTERVAL_MS = 70L
        private const val MIN_DURATION_MS = 15
        private const val MAX_DURATION_MS = 110
        private const val MIN_AMPLITUDE = 1
        private const val MAX_AMPLITUDE = 180
    }
}
