package com.navaq.android.ava

import android.Manifest
import android.content.ContentUris
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.provider.MediaStore
import androidx.core.content.ContextCompat
import org.json.JSONObject
import java.util.Locale

data class AvaCatalogTrack(
    val id: String,
    val title: String,
    val artist: String? = null,
    val album: String? = null,
    val durationMs: Long? = null,
    val sourceKind: String,
    val sourceValue: String
) {
    fun toCoreJson(): JSONObject = JSONObject()
        .put("id", id)
        .put("title", title)
        .put("artist", artist ?: JSONObject.NULL)
        .put("album", album ?: JSONObject.NULL)
        .put("duration_ms", durationMs ?: JSONObject.NULL)
        .put("source", JSONObject().put("kind", sourceKind).put("value", sourceValue))
}

/** Adapter for NAVAQ's existing in-app library; register it at app startup. */
fun interface AvaInAppMusicProvider {
    fun search(query: String?): List<AvaCatalogTrack>
}

object AvaInAppCatalog {
    @Volatile private var provider: AvaInAppMusicProvider? = null
    fun register(value: AvaInAppMusicProvider?) { provider = value }
    internal fun search(query: String?): List<AvaCatalogTrack> = provider?.search(query).orEmpty()
}

/** Read-only MediaStore adapter; never scans arbitrary filesystem paths. */
class AvaMusicCatalog(private val context: Context) {
    fun hasDeviceMusicPermission(): Boolean {
        val permission = if (Build.VERSION.SDK_INT >= 33) {
            Manifest.permission.READ_MEDIA_AUDIO
        } else {
            Manifest.permission.READ_EXTERNAL_STORAGE
        }
        return ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED
    }

    fun search(query: String?, settings: AvaSettings): List<AvaCatalogTrack> {
        val result = mutableListOf<AvaCatalogTrack>()
        if (settings.musicSources != AvaMusicSources.IN_APP && hasDeviceMusicPermission()) {
            result += searchDevice(query)
        }
        if (settings.musicSources != AvaMusicSources.DEVICE) {
            result += AvaInAppCatalog.search(query).filter { matches(it, query) }
        }
        return result
            .distinctBy { it.id }
            .sortedWith(compareBy<AvaCatalogTrack> { relevance(it, query) }
                .thenBy { it.title.lowercase(Locale.ROOT) }
                .thenBy { it.id })
    }

    fun chooseRandom(settings: AvaSettings): AvaCatalogTrack? {
        val tracks = search(null, settings)
        return tracks.randomOrNull()
    }

    private fun searchDevice(query: String?): List<AvaCatalogTrack> {
        val collection = MediaStore.Audio.Media.EXTERNAL_CONTENT_URI
        val projection = arrayOf(
            MediaStore.Audio.Media._ID,
            MediaStore.Audio.Media.TITLE,
            MediaStore.Audio.Media.ARTIST,
            MediaStore.Audio.Media.ALBUM,
            MediaStore.Audio.Media.DURATION
        )
        val selection = "${MediaStore.Audio.Media.IS_MUSIC} != 0"
        val tracks = mutableListOf<AvaCatalogTrack>()
        try {
            context.contentResolver.query(
                collection, projection, selection, null,
                "${MediaStore.Audio.Media.TITLE} COLLATE NOCASE ASC"
            )?.use { cursor ->
                val idColumn = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media._ID)
                val titleColumn = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.TITLE)
                val artistColumn = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.ARTIST)
                val albumColumn = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.ALBUM)
                val durationColumn = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.DURATION)
                while (cursor.moveToNext()) {
                    val mediaId = cursor.getLong(idColumn)
                    val title = cursor.getString(titleColumn)?.takeIf(String::isNotBlank) ?: continue
                    val artist = cursor.getString(artistColumn)?.takeUnless { it.isNullOrBlank() }
                    val album = cursor.getString(albumColumn)?.takeUnless { it.isNullOrBlank() }
                    val duration = cursor.getLong(durationColumn).takeIf { it > 0L }
                    val uri = ContentUris.withAppendedId(collection, mediaId)
                    val track = AvaCatalogTrack(
                        id = "device:$mediaId",
                        title = title,
                        artist = artist,
                        album = album,
                        durationMs = duration,
                        sourceKind = "content_uri",
                        sourceValue = uri.toString()
                    )
                    if (matches(track, query)) tracks += track
                }
            }
        } catch (_: SecurityException) {
            // Permission may be revoked between the permission check and query.
        } catch (_: IllegalArgumentException) {
            // An OEM MediaStore provider may reject a query on this collection.
        }
        return tracks
    }

    private fun relevance(track: AvaCatalogTrack, query: String?): Int {
        if (query.isNullOrBlank()) return 10
        val needle = normalize(query)
        val title = normalize(track.title)
        val artist = normalize(track.artist.orEmpty())
        val album = normalize(track.album.orEmpty())
        return when {
            title == needle -> 0
            title.startsWith(needle) -> 1
            title.contains(needle) -> 2
            artist == needle -> 3
            artist.contains(needle) -> 4
            album.contains(needle) -> 5
            else -> 6
        }
    }

    private fun matches(track: AvaCatalogTrack, query: String?): Boolean {
        if (query.isNullOrBlank()) return true
        val needle = normalize(query)
        val haystack = normalize(listOfNotNull(track.title, track.artist, track.album).joinToString(" "))
        return needle.split(' ').filter(String::isNotBlank).all(haystack::contains)
    }

    private fun normalize(value: String): String = value.lowercase(Locale.ROOT)
        .replace('ي', 'ی')
        .replace('ك', 'ک')
        .replace(Regex("\\s+"), " ")
        .trim()
}
