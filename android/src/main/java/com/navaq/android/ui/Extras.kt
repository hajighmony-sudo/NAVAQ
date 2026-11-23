package com.navaq.android.ui

import android.content.Context
import android.net.Uri
import android.provider.MediaStore
import com.navaq.android.ava.AvaCatalogTrack
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/** Listening statistics (play counts, last played, listening time), kept locally. */
class PlayStats(context: Context) {
    private val prefs = context.applicationContext.getSharedPreferences("navaq_stats", Context.MODE_PRIVATE)
    private val map = LinkedHashMap<String, LongArray>() // id -> [plays, lastPlayedMs, totalListenMs]
    private var curId: String? = null
    private var curMs = 0L
    private var counted = false
    private var dirty = 0

    init {
        try {
            val o = JSONObject(prefs.getString("m", "{}") ?: "{}")
            o.keys().forEach { k ->
                val a = o.getJSONArray(k)
                map[k] = longArrayOf(a.getLong(0), a.getLong(1), a.getLong(2))
            }
        } catch (_: Exception) {
        }
    }

    /** Call regularly (e.g. every 500 ms) with the current track and elapsed time. A play counts after 30 s. */
    fun tick(id: String?, playing: Boolean, dtMs: Long) {
        if (id == null || id.startsWith("room:")) return
        if (id != curId) { curId = id; curMs = 0L; counted = false }
        if (!playing) return
        val e = map.getOrPut(id) { LongArray(3) }
        e[2] += dtMs
        curMs += dtMs
        if (!counted && curMs >= 30_000L) {
            counted = true
            e[0] += 1
            e[1] = System.currentTimeMillis()
        }
        if (++dirty >= 20) flush()
    }

    fun flush() {
        dirty = 0
        val o = JSONObject()
        map.forEach { (k, v) -> o.put(k, JSONArray(v.toList())) }
        prefs.edit().putString("m", o.toString()).apply()
    }

    fun plays(id: String) = map[id]?.get(0) ?: 0L
    fun recent(n: Int): List<String> = map.filter { it.value[0] > 0 }.entries.sortedByDescending { it.value[1] }.take(n).map { it.key }
    fun top(n: Int): List<String> = map.filter { it.value[0] > 0 }.entries.sortedByDescending { it.value[0] }.take(n).map { it.key }
    fun totalMs() = map.values.sumOf { it[2] }
    fun totalPlays() = map.values.sumOf { it[0] }
    fun distinct() = map.count { it.value[0] > 0 }

    fun clear() {
        map.clear(); curId = null; curMs = 0L; counted = false
        prefs.edit().clear().apply()
    }
}

/** Tracks the user hid from the library. */
class HiddenStore(context: Context) {
    private val prefs = context.applicationContext.getSharedPreferences("navaq_hidden", Context.MODE_PRIVATE)
    fun all(): Set<String> = prefs.getStringSet("ids", emptySet()).orEmpty().toSet()
    fun hide(id: String) = prefs.edit().putStringSet("ids", all() + id).apply()
    fun show(id: String) = prefs.edit().putStringSet("ids", all() - id).apply()
}

/** Last playing session, so the user can continue where they stopped. */
class LastSession(context: Context) {
    private val prefs = context.applicationContext.getSharedPreferences("navaq_last", Context.MODE_PRIVATE)
    fun save(queue: List<String>, id: String, positionMs: Long) {
        prefs.edit().putString("queue", queue.joinToString("\n")).putString("id", id).putLong("pos", positionMs).apply()
    }
    fun queue(): List<String> = prefs.getString("queue", "").orEmpty().split('\n').filter { it.isNotBlank() }
    fun id(): String? = prefs.getString("id", null)
    fun position(): Long = prefs.getLong("pos", 0L)
}

/** Synced (LRC) or plain lyrics: a pasted text saved per track, or a `.lrc` file next to the audio file. */
object Lyrics {
    class Line(val ms: Long, val text: String)

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("navaq_lyrics", Context.MODE_PRIVATE)

    fun save(ctx: Context, id: String, text: String) = prefs(ctx).edit().putString(id, text).apply()
    fun remove(ctx: Context, id: String) = prefs(ctx).edit().remove(id).apply()

    fun load(ctx: Context, track: AvaCatalogTrack): List<Line> {
        val raw = prefs(ctx).getString(track.id, null) ?: sidecar(ctx, track) ?: return emptyList()
        return parse(raw)
    }

    private fun sidecar(ctx: Context, t: AvaCatalogTrack): String? = try {
        val uri = Uri.parse(t.sourceValue)
        val path = when (uri.scheme) {
            "file" -> uri.path
            "content" -> ctx.contentResolver.query(uri, arrayOf(MediaStore.MediaColumns.DATA), null, null, null)?.use {
                if (it.moveToFirst()) it.getString(0) else null
            }
            else -> null
        }
        val f = path?.let { File(it.substringBeforeLast('.') + ".lrc") }
        if (f != null && f.isFile && f.canRead() && f.length() < 512_000) f.readText() else null
    } catch (_: Exception) {
        null
    }

    fun parse(raw: String): List<Line> {
        val stamp = Regex("\\[(\\d{1,3}):(\\d{2})(?:[.:](\\d{1,3}))?]")
        var offset = 0L
        val synced = ArrayList<Line>()
        val plain = ArrayList<Line>()
        for (line in raw.lines()) {
            val trimmed = line.trim()
            if (trimmed.isEmpty()) continue
            Regex("^\\[offset:\\s*(-?\\d+)]", RegexOption.IGNORE_CASE).find(trimmed)?.let { offset = it.groupValues[1].toLong(); continue }
            val stamps = stamp.findAll(trimmed).toList()
            if (stamps.isEmpty()) {
                if (!trimmed.startsWith("[") ) plain += Line(-1L, trimmed)
                continue
            }
            val text = trimmed.substring(stamps.last().range.last + 1).trim()
            for (m in stamps) {
                val frac = m.groupValues[3]
                val msFrac = if (frac.isEmpty()) 0L else frac.padEnd(3, '0').take(3).toLong()
                synced += Line(m.groupValues[1].toLong() * 60_000L + m.groupValues[2].toLong() * 1000L + msFrac, text)
            }
        }
        return if (synced.isNotEmpty()) synced.map { Line((it.ms + offset).coerceAtLeast(0L), it.text) }.sortedBy { it.ms } else plain
    }
}

/** Full local backup/restore of the app's own preferences (theme, favourites, playlists, stats, lyrics, ...). */
object BackupKit {
    private val FILES = listOf(
        "navaq_ui_settings", "navaq_favorites", "navaq_playlists", "navaq_ui_runtime", "navaq_created",
        "navaq_stats", "navaq_hidden", "navaq_lyrics", "navaq_playback_preferences"
    )

    fun export(ctx: Context): String {
        val root = JSONObject().put("app", "NAVAQ").put("version", 1)
        val all = JSONObject()
        for (name in FILES) {
            val o = JSONObject()
            for ((k, v) in ctx.getSharedPreferences(name, Context.MODE_PRIVATE).all) {
                val e = JSONObject()
                when (v) {
                    is String -> e.put("t", "s").put("v", v)
                    is Int -> e.put("t", "i").put("v", v)
                    is Long -> e.put("t", "l").put("v", v)
                    is Float -> e.put("t", "f").put("v", v.toDouble())
                    is Boolean -> e.put("t", "b").put("v", v)
                    is Set<*> -> e.put("t", "set").put("v", JSONArray(v.map { it.toString() }))
                    else -> continue
                }
                o.put(k, e)
            }
            all.put(name, o)
        }
        return root.put("prefs", all).toString()
    }

    /** Returns false (and changes nothing) when the text is not a NAVAQ backup. */
    fun import(ctx: Context, text: String): Boolean {
        val root = try { JSONObject(text) } catch (_: Exception) { return false }
        if (root.optString("app") != "NAVAQ") return false
        val all = root.optJSONObject("prefs") ?: return false
        for (name in FILES) {
            val o = all.optJSONObject(name) ?: continue
            val edit = ctx.getSharedPreferences(name, Context.MODE_PRIVATE).edit().clear()
            o.keys().forEach { k ->
                val e = o.optJSONObject(k) ?: return@forEach
                when (e.optString("t")) {
                    "s" -> edit.putString(k, e.optString("v"))
                    "i" -> edit.putInt(k, e.optInt("v"))
                    "l" -> edit.putLong(k, e.optLong("v"))
                    "f" -> edit.putFloat(k, e.optDouble("v").toFloat())
                    "b" -> edit.putBoolean(k, e.optBoolean("v"))
                    "set" -> {
                        val a = e.optJSONArray("v") ?: JSONArray()
                        edit.putStringSet(k, (0 until a.length()).map { a.getString(it) }.toSet())
                    }
                }
            }
            edit.apply()
        }
        return true
    }
}
