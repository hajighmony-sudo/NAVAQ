package com.navaq.android.ui

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import androidx.core.content.ContextCompat
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import com.navaq.android.ava.AvaCatalogTrack
import com.navaq.android.playback.NavaqPlaybackService

/**
 * Thin Media3 controller for the UI. It talks to the existing NavaqPlaybackService session,
 * so notification, lock-screen and headset controls keep working while the UI is closed.
 */
class PlaybackClient(private val context: Context, private val onChange: () -> Unit) {
    private var future: ListenableFuture<MediaController>? = null
    private var controller: MediaController? = null
    private var released = false

    private val listener = object : Player.Listener {
        override fun onEvents(player: Player, events: Player.Events) = onChange()
    }

    val isConnected: Boolean get() = controller != null
    val isPlaying: Boolean get() = controller?.isPlaying == true
    val shuffle: Boolean get() = controller?.shuffleModeEnabled == true
    val repeatMode: Int get() = controller?.repeatMode ?: Player.REPEAT_MODE_OFF
    val currentId: String? get() = controller?.currentMediaItem?.mediaId?.takeIf { it.isNotBlank() }
    val positionMs: Long get() = controller?.currentPosition?.coerceAtLeast(0L) ?: 0L
    val durationMs: Long get() = controller?.duration?.takeIf { it > 0L } ?: 0L
    val speed: Float get() = controller?.playbackParameters?.speed ?: 1f
    val queueIds: List<String>
        get() {
            val c = controller ?: return emptyList()
            return (0 until c.mediaItemCount).map { c.getMediaItemAt(it).mediaId }
        }

    fun connect() {
        val token = SessionToken(context, ComponentName(context, NavaqPlaybackService::class.java))
        val f = MediaController.Builder(context, token).buildAsync()
        future = f
        f.addListener({
            if (released) return@addListener
            controller = try { f.get() } catch (_: Exception) { null }
            controller?.addListener(listener)
            onChange()
        }, ContextCompat.getMainExecutor(context))
    }

    fun release() {
        released = true
        controller?.removeListener(listener)
        future?.let { MediaController.releaseFuture(it) }
        controller = null
    }

    fun playQueue(tracks: List<AvaCatalogTrack>, startIndex: Int) {
        val c = controller ?: return
        if (tracks.isEmpty()) return
        val items = tracks.map { t ->
            MediaItem.Builder()
                .setMediaId(t.id)
                .setUri(t.sourceValue)
                .setMediaMetadata(
                    MediaMetadata.Builder()
                        .setTitle(t.title)
                        .setArtist(t.artist)
                        .setAlbumTitle(t.album)
                        .build()
                )
                .build()
        }
        c.setMediaItems(items, startIndex.coerceIn(0, items.lastIndex), 0L)
        c.prepare()
        c.play()
    }

    fun jumpTo(id: String): Boolean {
        val c = controller ?: return false
        val idx = queueIds.indexOf(id)
        if (idx < 0) return false
        c.seekTo(idx, 0L)
        c.play()
        return true
    }

    fun toggle() { controller?.let { if (it.isPlaying) it.pause() else it.play() } }
    fun next() { controller?.seekToNextMediaItem() }
    fun previous() {
        controller?.let { if (it.currentPosition > 3000L) it.seekTo(0L) else it.seekToPreviousMediaItem() }
    }
    fun seekTo(ms: Long) { controller?.seekTo(ms) }
    fun pause() { controller?.pause() }
    fun setSpeed(value: Float) { controller?.setPlaybackSpeed(value) }
    fun toggleShuffle() { controller?.let { it.shuffleModeEnabled = !it.shuffleModeEnabled } }
    fun cycleRepeat() {
        controller?.let {
            it.repeatMode = when (it.repeatMode) {
                Player.REPEAT_MODE_OFF -> Player.REPEAT_MODE_ALL
                Player.REPEAT_MODE_ALL -> Player.REPEAT_MODE_ONE
                else -> Player.REPEAT_MODE_OFF
            }
        }
    }

    fun setBeatHaptics(enabled: Boolean) {
        val intent = Intent(context, NavaqPlaybackService::class.java)
            .setAction(NavaqPlaybackService.ACTION_SET_BEAT_HAPTICS)
            .putExtra(NavaqPlaybackService.EXTRA_ENABLED, enabled)
        context.startService(intent)
    }

    /** Sends a Rust-validated equalizer (`EqualizerSettings` JSON) to the playback DSP. */
    fun setEqualizerJson(json: String) {
        val intent = Intent(context, NavaqPlaybackService::class.java)
            .setAction(NavaqPlaybackService.ACTION_SET_EQUALIZER)
            .putExtra(NavaqPlaybackService.EXTRA_EQUALIZER_JSON, json)
        context.startService(intent)
    }

    fun setAdvancedHaptics(enabled: Boolean) {
        context.startService(
            Intent(context, NavaqPlaybackService::class.java)
                .setAction(NavaqPlaybackService.ACTION_SET_ADVANCED_HAPTICS)
                .putExtra(NavaqPlaybackService.EXTRA_ENABLED, enabled)
        )
    }

    /** Applies the simplified room reverb; pass mix 0 to turn it off. */
    fun setRoom(delayMs: Int, feedback: Float, mix: Float) {
        context.startService(
            Intent(context, NavaqPlaybackService::class.java)
                .setAction(NavaqPlaybackService.ACTION_SET_ROOM)
                .putExtra(NavaqPlaybackService.EXTRA_ROOM_DELAY_MS, delayMs)
                .putExtra(NavaqPlaybackService.EXTRA_ROOM_FEEDBACK, feedback)
                .putExtra(NavaqPlaybackService.EXTRA_ROOM_MIX, mix)
        )
    }
}
