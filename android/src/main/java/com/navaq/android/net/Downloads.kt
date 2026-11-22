package com.navaq.android.net

import android.app.DownloadManager
import android.content.Context
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.os.Environment
import com.navaq.android.ava.AvaCatalogTrack
import com.navaq.android.core.NavaqNativeBridge
import java.io.File
import java.net.InetAddress

/** Direct-URL audio downloads on top of Android's DownloadManager (shares the id list the completion receiver checks). */
object Downloads {
    private const val PREFS = "navaq_discover"
    private const val KEY = "download_manager_ids"
    private val AUDIO = setOf("mp3", "m4a", "mp4", "m4v", "flac", "ogg", "oga", "wav", "opus", "aac", "ts", "webm", "mkv", "mov", "avi", "3gp")

    class Job(
        val id: Long, val title: String, val url: String, val status: Int, val reason: Int,
        val done: Long, val total: Long, val localUri: String?
    ) {
        val percent: Int get() = if (total > 0) (done * 100 / total).toInt().coerceIn(0, 100) else 0
        val running: Boolean get() = status == DownloadManager.STATUS_RUNNING ||
            status == DownloadManager.STATUS_PENDING || status == DownloadManager.STATUS_PAUSED
    }

    fun dir(ctx: Context): File =
        File(ctx.getExternalFilesDir(Environment.DIRECTORY_MUSIC) ?: ctx.filesDir, "NAVAQ").apply { mkdirs() }

    private fun ids(ctx: Context): MutableSet<String> =
        ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getStringSet(KEY, emptySet()).orEmpty().toMutableSet()

    private fun saveIds(ctx: Context, ids: Set<String>) =
        ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putStringSet(KEY, ids).apply()

    private fun isPrivate(a: InetAddress) =
        a.isAnyLocalAddress || a.isLoopbackAddress || a.isLinkLocalAddress || a.isSiteLocalAddress ||
            a.hostAddress?.startsWith("fc", true) == true || a.hostAddress?.startsWith("fd", true) == true

    private fun mimeFor(name: String) = when (name.substringAfterLast('.', "").lowercase()) {
        "mp3" -> "audio/mpeg"
        "m4a", "aac" -> "audio/mp4"
        "mp4", "m4v" -> "video/mp4"
        "webm" -> "video/webm"
        "mkv" -> "video/x-matroska"
        "mov" -> "video/quicktime"
        "avi" -> "video/x-msvideo"
        "3gp" -> "video/3gpp"
        "flac" -> "audio/flac"
        "ogg", "oga" -> "audio/ogg"
        "wav" -> "audio/wav"
        "opus" -> "audio/opus"
        else -> "application/octet-stream"
    }

    private fun fileName(url: String, wanted: String): String {
        val base = wanted.trim().ifBlank { Uri.parse(url).lastPathSegment.orEmpty() }
        val cleaned = base.replace(Regex("[^\\p{L}\\p{N}._ -]"), "_").trim().take(90).trim('.').ifBlank { "navaq-audio" }
        val named = if (cleaned.substringAfterLast('.', "").lowercase() in AUDIO) cleaned else "$cleaned.mp3"
        val dot = named.lastIndexOf('.')
        val stamp = "-" + System.currentTimeMillis() % 100000000
        return named.substring(0, dot) + stamp + named.substring(dot)
    }

    /** Blocking (DNS lookup): call off the main thread. Returns an error message, or null when queued. */
    fun enqueue(ctx: Context, rawUrl: String, wantedName: String, wifiOnly: Boolean): String? {
        val url = rawUrl.trim()
        if (!url.startsWith("https://") && !url.startsWith("http://")) return "فقط لینک مستقیم http یا https پذیرفته می‌شود"
        return try {
            if (!NavaqNativeBridge.nativeValidateDownloadUrl(url)) return "این لینک معتبر نیست یا به آدرس محلی اشاره می‌کند"
            val host = Uri.parse(url).host ?: return "آدرس نامعتبر است"
            if (InetAddress.getAllByName(host).any(::isPrivate)) return "این میزبان به آدرس محلی اشاره می‌کند"
            val name = fileName(url, wantedName)
            val request = DownloadManager.Request(Uri.parse(url))
                .setTitle(name.substringBeforeLast('-'))
                .setDescription("NAVAQ")
                .setMimeType(mimeFor(name))
                .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE)
                .setAllowedOverMetered(!wifiOnly)
                .setAllowedOverRoaming(false)
                .setDestinationInExternalFilesDir(ctx, Environment.DIRECTORY_MUSIC, "NAVAQ/$name")
            val manager = ctx.getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager
            val id = manager.enqueue(request)
            saveIds(ctx, ids(ctx).apply { add(id.toString()) })
            null
        } catch (e: Exception) {
            "صف‌کردن دانلود ناموفق بود: ${e.message ?: "خطا"}"
        }
    }

    fun list(ctx: Context): List<Job> {
        val manager = ctx.getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager
        val known = ids(ctx)
        if (known.isEmpty()) return emptyList()
        val out = ArrayList<Job>()
        val cursor = manager.query(DownloadManager.Query().setFilterById(*known.mapNotNull { it.toLongOrNull() }.toLongArray()))
            ?: return emptyList()
        cursor.use {
            while (it.moveToNext()) {
                fun s(c: String) = it.getString(it.getColumnIndexOrThrow(c))
                fun n(c: String) = it.getLong(it.getColumnIndexOrThrow(c))
                out += Job(
                    n(DownloadManager.COLUMN_ID), s(DownloadManager.COLUMN_TITLE).orEmpty(), s(DownloadManager.COLUMN_URI).orEmpty(),
                    n(DownloadManager.COLUMN_STATUS).toInt(), n(DownloadManager.COLUMN_REASON).toInt(),
                    n(DownloadManager.COLUMN_BYTES_DOWNLOADED_SO_FAR), n(DownloadManager.COLUMN_TOTAL_SIZE_BYTES),
                    s(DownloadManager.COLUMN_LOCAL_URI)
                )
            }
        }
        return out.sortedByDescending { it.id }
    }

    /** Cancels a running job or removes a finished one (the file is deleted too). */
    fun remove(ctx: Context, id: Long) {
        (ctx.getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager).remove(id)
        saveIds(ctx, ids(ctx).apply { remove(id.toString()) })
    }

    /** Audio files already in the downloads folder, as library tracks. */
    fun tracks(ctx: Context): List<AvaCatalogTrack> =
        (dir(ctx).listFiles() ?: emptyArray()).filter { it.isFile && it.extension.lowercase() in AUDIO && it.length() > 0 }
            .sortedByDescending { it.lastModified() }.map { f ->
                var title = f.nameWithoutExtension.substringBeforeLast('-').replace('_', ' ')
                var artist: String? = null
                var dur = 0L
                try {
                    val r = MediaMetadataRetriever()
                    r.setDataSource(f.absolutePath)
                    r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_TITLE)?.takeIf { it.isNotBlank() }?.let { title = it }
                    artist = r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_ARTIST)?.takeIf { it.isNotBlank() }
                    dur = r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull() ?: 0L
                    r.release()
                } catch (_: Exception) {
                }
                AvaCatalogTrack(
                    id = "dl:" + f.name, title = title, artist = artist, album = "دانلودها",
                    durationMs = dur, sourceKind = "local_file", sourceValue = "file://" + f.absolutePath
                )
            }
}
