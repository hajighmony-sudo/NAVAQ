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
import com.navaq.android.ava.AvaCommandInterpreter
import com.navaq.android.ava.AvaMusicCatalog
import com.navaq.android.ava.AvaResponseMode
import com.navaq.android.ava.AvaSpeechOutput
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
    private var avaSpeechOutput: AvaSpeechOutput? = null

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
        haptics.setEnabled(
            getSharedPreferences(PREFERENCES_NAME, MODE_PRIVATE)
                .getBoolean(PREFERENCE_BEAT_HAPTICS, false)
        )
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
        getSharedPreferences(PREFERENCES_NAME, MODE_PRIVATE)
            .edit()
            .putBoolean(PREFERENCE_BEAT_HAPTICS, enabled)
            .apply()
        beatHaptics?.setEnabled(enabled)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_SET_BEAT_HAPTICS ->
                setBeatHapticsEnabled(intent.getBooleanExtra(EXTRA_ENABLED, false))
            ACTION_EXECUTE_AVA -> {
                val transcript = intent.getStringExtra(EXTRA_AVA_TRANSCRIPT).orEmpty()
                if (transcript.isNotBlank()) executeAvaTranscript(transcript)
            }
        }
        return super.onStartCommand(intent, flags, startId)
    }

    /**
     * Execute a recognized AVA transcript through the existing Rust PlayerCore.
     * This reuses the current Media3 player and does not create a second playback
     * engine or modify any existing queue/player capability.
     */
    private fun executeAvaTranscript(transcript: String) {
        val interpreter = AvaCommandInterpreter(this)
        val settings = interpreter.currentSettings()
        if (!settings.enabled) {
            publishAvaResult("AVA is disabled in settings.")
            return
        }

        val decision = try {
            interpreter.interpret(transcript)
        } catch (error: Exception) {
            publishAvaResult("AVA could not interpret this command: ${error.message ?: "unknown error"}")
            return
        }
        if (decision.has("error")) {
            publishAvaResult(decision.optString("error"))
            return
        }

        val intent = decision.optJSONObject("intent")
        if (intent == null) {
            publishAvaResult("AVA returned an invalid command.")
            return
        }

        val status = when (intent.optString("type")) {
            "transport" -> {
                val command = intent.opt("command")
                val commandJson = when (command) {
                    is String -> JSONObject.quote(command)
                    is JSONObject -> command.toString()
                    else -> null
                }
                if (commandJson == null) {
                    "AVA could not understand the playback command."
                } else {
                    val output = dispatchCoreCommand(commandJson)
                    if (JSONObject(output).has("error")) {
                        "There is no active track or queue for that command."
                    } else {
                        when (command) {
                            "Next" -> "Skipped to the next track."
                            "Previous" -> "Returned to the previous track."
                            "Pause" -> "Playback paused."
                            "Resume" -> "Playback resumed."
                            "Stop" -> "Playback stopped."
                            else -> "Playback command completed."
                        }
                    }
                }
            }
            "play_random" -> {
                val catalog = AvaMusicCatalog(this)
                val track = catalog.chooseRandom(settings)
                if (track == null) {
                    if (settings.musicSources != com.navaq.android.ava.AvaMusicSources.IN_APP &&
                        !catalog.hasDeviceMusicPermission()
                    ) {
                        "Allow AVA to access audio files, or choose the in-app music source."
                    } else {
                        "No music was found in the selected music sources."
                    }
                } else {
                    playAvaTracks(listOf(track))
                }
            }
            "search_music" -> {
                val query = intent.optString("query").trim()
                val tracks = AvaMusicCatalog(this).search(query, settings).take(MAX_AVA_SEARCH_RESULTS)
                if (tracks.isEmpty()) {
                    "I couldn't find a matching track for: $query"
                } else {
                    playAvaTracks(tracks)
                }
            }
            "unknown" -> {
                when (decision.optString("route")) {
                    "online" -> "An online AI provider is not configured yet. Basic music commands are ready."
                    else -> "I couldn't recognize that command. Try a music or playback command."
                }
            }
            else -> "AVA does not support this command yet."
        }
        publishAvaResult(status, settings.responseMode)
    }

    private fun playAvaTracks(tracks: List<com.navaq.android.ava.AvaCatalogTrack>): String {
        if (tracks.isEmpty()) return "No playable tracks were found."
        val serializedTracks = JSONArray()
        tracks.forEach { serializedTracks.put(it.toCoreJson()) }
        val queueCommand = JSONObject()
            .put("SetQueue", JSONObject()
                .put("tracks", serializedTracks)
                .put("start_index", 0))
            .toString()
        val queueResult = JSONObject(dispatchCoreCommand(queueCommand))
        if (queueResult.has("error")) {
            return "The music queue could not be prepared: ${queueResult.optString("error")}"
        }
        val playResult = JSONObject(dispatchCoreCommand(JSONObject.quote("Play")))
        return if (playResult.has("error")) {
            "The selected track could not be started: ${playResult.optString("error")}"
        } else {
            if (tracks.size == 1) "Playing ${tracks.first().title}."
            else "Playing the best match. ${tracks.size} matching tracks added to the queue."
        }
    }

    private fun publishAvaResult(message: String, responseMode: AvaResponseMode? = null) {
        val settings = AvaCommandInterpreter(this).currentSettings()
        val result = Intent(ACTION_AVA_RESULT)
            .setPackage(packageName)
            .putExtra(EXTRA_AVA_RESULT, message)
        sendBroadcast(result)
        if ((responseMode ?: settings.responseMode) == AvaResponseMode.VOICE) {
            val output = avaSpeechOutput ?: AvaSpeechOutput(this).also { avaSpeechOutput = it }
            output.speak(message, settings)
        }
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
                        action.has("SetEqualizer") -> {
                            // Rust validates and stores EQ settings; its stateful PCM
                            // pipeline applies them inside the Media3 audio path.
                            beatProcessor?.setEqualizerJson(
                                action.getJSONObject("SetEqualizer").toString()
                            )
                        }
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

    companion object {
        /** Intent action for a settings UI to change the explicit haptics preference. */
        const val ACTION_SET_BEAT_HAPTICS =
            "com.navaq.android.action.SET_BEAT_HAPTICS"
        const val EXTRA_ENABLED = "com.navaq.android.extra.BEAT_HAPTICS_ENABLED"
        const val ACTION_EXECUTE_AVA = "com.navaq.android.action.EXECUTE_AVA"
        const val EXTRA_AVA_TRANSCRIPT = "com.navaq.android.extra.AVA_TRANSCRIPT"
        const val ACTION_AVA_RESULT = "com.navaq.android.action.AVA_RESULT"
        const val EXTRA_AVA_RESULT = "com.navaq.android.extra.AVA_RESULT_TEXT"
        private const val MAX_AVA_SEARCH_RESULTS = 30
        private const val PREFERENCES_NAME = "navaq_playback_preferences"
        private const val PREFERENCE_BEAT_HAPTICS = "beat_haptics_enabled"
    }

    override fun onDestroy() {
        mainHandler.removeCallbacks(positionReporter)
        player?.removeListener(playerListener)
        avaSpeechOutput?.release()
        avaSpeechOutput = null
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
