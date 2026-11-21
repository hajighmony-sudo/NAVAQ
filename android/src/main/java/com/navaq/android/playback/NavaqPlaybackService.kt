package com.navaq.android.playback

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import androidx.media3.common.AudioAttributes
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.audio.AudioSink
import androidx.media3.exoplayer.audio.DefaultAudioSink
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import com.navaq.android.core.NavaqNativeBridge
import com.navaq.android.haptics.NavaqBeatAudioProcessor
import com.navaq.android.haptics.NavaqHaptics
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/**
 * Android host for NAVAQ. Rust owns queue/state decisions; Media3 owns decoding
 * and the operating-system playback session.
 */
@UnstableApi
class NavaqPlaybackService : MediaSessionService() {
    private val mainHandler = Handler(Looper.getMainLooper())
    private var mediaSession: MediaSession? = null
    private var player: ExoPlayer? = null
    private var nativeHandle: Long = 0L
    private var beatHaptics: NavaqHaptics? = null
    private var beatProcessor: NavaqBeatAudioProcessor? = null

    private val positionReporter = object : Runnable {
        override fun run() {
            val activePlayer = player ?: return
            val trackId = activePlayer.currentMediaItem?.mediaId
            if (!trackId.isNullOrBlank() && activePlayer.isPlaying) {
                dispatchBackendEvent(
                    JSONObject()
                        .put("Position", JSONObject()
                            .put("track_id", trackId)
                            .put("position_ms", activePlayer.currentPosition.coerceAtLeast(0L)))
                )
            }
            mainHandler.postDelayed(this, 1_000L)
        }
    }

    private val playerListener = object : Player.Listener {
        override fun onPlaybackStateChanged(playbackState: Int) {
            val activePlayer = player ?: return
            val trackId = activePlayer.currentMediaItem?.mediaId ?: return
            when (playbackState) {
                Player.STATE_READY -> {
                    val duration = activePlayer.duration
                    val payload = JSONObject()
                        .put("track_id", trackId)
                        .put("duration_ms", if (duration >= 0L) duration else JSONObject.NULL)
                    dispatchBackendEvent(JSONObject().put("Prepared", payload))
                }
                Player.STATE_ENDED -> {
                    dispatchBackendEvent(
                        JSONObject().put("Ended", JSONObject().put("track_id", trackId))
                    )
                }
            }
        }

        override fun onIsLoadingChanged(isLoading: Boolean) {
            val trackId = player?.currentMediaItem?.mediaId ?: return
            dispatchBackendEvent(
                JSONObject().put(
                    "Buffering",
                    JSONObject().put("track_id", trackId).put("is_buffering", isLoading)
                )
            )
        }

        override fun onPlayerError(error: androidx.media3.common.PlaybackException) {
            val trackId = player?.currentMediaItem?.mediaId ?: return
            dispatchBackendEvent(
                JSONObject().put(
                    "Failed",
                    JSONObject()
                        .put("track_id", trackId)
                        .put("code", error.errorCodeName)
                        .put("message", error.message ?: "Media3 playback failed")
                )
            )
        }
    }

    override fun onCreate() {
        super.onCreate()
        nativeHandle = NavaqNativeBridge.nativeCreate()

        val haptics = NavaqHaptics(this)
        beatHaptics = haptics
        val onsetProcessor = NavaqBeatAudioProcessor { strength ->
            // Vibrator calls are dispatched away from Media3's audio thread.
            mainHandler.post {
                haptics.onAudioEvent(NavaqHaptics.Trigger.BEAT, strength)
            }
        }
        beatProcessor = onsetProcessor
        val renderersFactory = object : DefaultRenderersFactory(this) {
            override fun buildAudioSink(
                context: Context,
                enableFloatOutput: Boolean,
                enableAudioOutputPlaybackParameters: Boolean,
            ): AudioSink {
                return DefaultAudioSink.Builder(context)
                    .setEnableFloatOutput(enableFloatOutput)
                    .setEnableAudioOutputPlaybackParameters(enableAudioOutputPlaybackParameters)
                    .setAudioProcessors(arrayOf(onsetProcessor))
                    .build()
            }
        }

        val audioPlayer = ExoPlayer.Builder(this, renderersFactory)
            .build()
            .apply {
                setHandleAudioBecomingNoisy(true)
                // Media3 requests/releases Android audio focus and handles ducking.
                setAudioAttributes(AudioAttributes.DEFAULT, true)
                addListener(playerListener)
            }

        player = audioPlayer
        mediaSession = MediaSession.Builder(this, audioPlayer).build()
        mainHandler.post(positionReporter)
    }

    /**
     * User preference for beat-reactive haptics. Disabled by default; a settings
     * screen in the app can call this method after explicit user action.
     */
    fun setBeatHapticsEnabled(enabled: Boolean) {
        beatHaptics?.setEnabled(enabled)
    }

    fun dispatchCoreCommand(commandJson: String): String {
        check(nativeHandle != 0L) { "Rust core is not initialized" }
        val output = NavaqNativeBridge.nativeHandleCommand(nativeHandle, commandJson)
        applyCoreOutput(output)
        return output
    }

    private fun dispatchBackendEvent(event: JSONObject) {
        if (nativeHandle == 0L) return
        val output = NavaqNativeBridge.nativeHandleBackendEvent(nativeHandle, event.toString())
        applyCoreOutput(output)
    }

    private fun applyCoreOutput(outputJson: String) {
        val output = JSONObject(outputJson)
        if (output.has("error")) return
        val actions = output.optJSONArray("backend_actions") ?: JSONArray()
        for (index in 0 until actions.length()) {
            val action = actions.opt(index)
            when (action) {
                is String -> when (action) {
                    "Play" -> player?.play()
                    "Pause" -> player?.pause()
                    "Stop" -> player?.stop()
                }
                is JSONObject -> {
                    when {
                        action.has("Prepare") -> prepareTrack(action.getJSONObject("Prepare"))
                        action.has("SeekTo") -> player?.seekTo(action.getLong("SeekTo"))
                        action.has("SetVolume") -> player?.volume =
                            action.getDouble("SetVolume").toFloat().coerceIn(0f, 1f)
                        action.has("SetSpeed") -> {
                            val speed = action.getDouble("SetSpeed").toFloat()
                            player?.setPlaybackSpeed(speed)
                        }
                        // Equalizer DSP and crossfade rendering require dedicated
                        // Media3 audio processors; they are not silently simulated here.
                    }
                }
            }
        }
    }

    private fun prepareTrack(track: JSONObject) {
        val source = track.optJSONObject("source") ?: return
        val value = source.optString("value")
        if (value.isBlank()) return
        val uri = when (source.optString("kind")) {
            "local_file" -> Uri.fromFile(File(value))
            "content_uri", "remote_url" -> Uri.parse(value)
            else -> return
        }
        val title = track.optString("title").takeIf { it.isNotBlank() && it != "null" }
        val artist = track.optString("artist").takeIf { it.isNotBlank() && it != "null" }
        val album = track.optString("album").takeIf { it.isNotBlank() && it != "null" }
        val artworkValue = sequenceOf("artwork_uri", "artwork_url", "cover_uri", "image_url")
            .map { track.optString(it) }
            .firstOrNull { it.isNotBlank() && it != "null" }
        val artworkUri = artworkValue
            ?.let(Uri::parse)
            ?.takeIf { uri ->
                uri.scheme in setOf("content", "file", "https", "http", "android.resource")
            }

        // Publish complete, normalized metadata through the standard Android
        // MediaSession contract. System UI and OEM surfaces (including Xiaomi
        // HyperOS media surfaces and Samsung Now Bar on supported builds) consume
        // this session; their exact presentation remains controlled by the OS.
        val displayTitle = title ?: track.optString("id").takeIf { it.isNotBlank() && it != "null" }
        val metadataBuilder = MediaMetadata.Builder()
            .setTitle(displayTitle)
            .setDisplayTitle(displayTitle)
            .setSubtitle(artist ?: album)
            .setArtist(artist)
            .setAlbumTitle(album)
            .setIsPlayable(true)
        if (artworkUri != null) {
            metadataBuilder.setArtworkUri(artworkUri)
        }
        val metadata = metadataBuilder.build()
        val item = MediaItem.Builder()
            .setMediaId(track.optString("id"))
            .setUri(uri)
            .setMediaMetadata(metadata)
            .build()
        player?.setMediaItem(item)
        player?.prepare()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? =
        mediaSession

    override fun onTaskRemoved(rootIntent: Intent?) {
        val activePlayer = player
        if (activePlayer == null || !activePlayer.playWhenReady || activePlayer.mediaItemCount == 0) {
            stopSelf()
        }
    }

    override fun onDestroy() {
        mainHandler.removeCallbacks(positionReporter)
        player?.removeListener(playerListener)
        beatHaptics?.release()
        beatHaptics = null
        beatProcessor = null
        mediaSession?.release()
        player?.release()
        mediaSession = null
        player = null
        if (nativeHandle != 0L) {
            NavaqNativeBridge.nativeDestroy(nativeHandle)
            nativeHandle = 0L
        }
        super.onDestroy()
    }
}
