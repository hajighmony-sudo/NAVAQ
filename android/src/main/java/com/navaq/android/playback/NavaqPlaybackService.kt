package com.navaq.android.playback

import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService

/**
 * Owns the native Android audio engine for NAVAQ.
 *
 * Media3 publishes the media notification and exposes transport controls to the
 * lock screen, Bluetooth/headset devices, and compatible system surfaces.
 */
@UnstableApi
class NavaqPlaybackService : MediaSessionService() {
    private var mediaSession: MediaSession? = null
    private var player: ExoPlayer? = null

    override fun onCreate() {
        super.onCreate()

        val audioPlayer = ExoPlayer.Builder(this)
            .setHandleAudioBecomingNoisy(true)
            .build()
            .apply {
                // Let Android's audio-focus system pause/duck playback on interruptions.
                setAudioAttributes(AudioAttributes.DEFAULT, true)
                repeatMode = androidx.media3.common.Player.REPEAT_MODE_OFF
            }

        player = audioPlayer
        mediaSession = MediaSession.Builder(this, audioPlayer).build()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? {
        return mediaSession
    }

    override fun onTaskRemoved(rootIntent: android.content.Intent?) {
        val activePlayer = player
        if (activePlayer == null || !activePlayer.playWhenReady || activePlayer.mediaItemCount == 0) {
            stopSelf()
        }
        // Keep the service alive while audio is actively playing.
    }

    override fun onDestroy() {
        mediaSession?.run {
            player.release()
            release()
        }
        mediaSession = null
        player = null
        super.onDestroy()
    }
}
