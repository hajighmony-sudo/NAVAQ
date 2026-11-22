package com.navaq.android.net

import android.content.Context
import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaMuxer
import android.net.ConnectivityManager
import com.navaq.android.core.NavaqNativeBridge
import com.navaq.android.notifications.NavaqNotificationCenter
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.net.HttpURLConnection
import java.net.InetAddress
import java.net.URL
import java.nio.ByteBuffer
import java.util.concurrent.Executors

/**
 * "NAVAQ Downloader": paste a link to a media file, an HLS (.m3u8) stream or a web page that embeds
 * audio/video, and it downloads the media with pause/resume, redirects handled safely, and an optional
 * "audio only" extraction. It does not decipher protected streams, DRM or encrypted HLS, and it does not
 * implement site-specific extractors (e.g. YouTube).
 */
object LinkDownloader {
    const val QUEUED = "queued"
    const val RUNNING = "running"
    const val PAUSED = "paused"
    const val DONE = "done"
    const val FAILED = "failed"

    class Cand(val url: String, val label: String, val hls: Boolean, val ext: String)
    class Probe(val cands: List<Cand>, val title: String?, val error: String?)

    class Job(
        val id: String, val url: String, var title: String, var path: String,
        val hls: Boolean, val audioOnly: Boolean, val wifiOnly: Boolean
    ) {
        @Volatile var state = QUEUED
        @Volatile var done = 0L
        @Volatile var total = 0L
        @Volatile var error: String? = null
        @Volatile var stop: String? = null
        @Volatile var speedBps: Long = 0L
        @Volatile var etaSeconds: Long = -1L
        @Volatile var workerActive: Boolean = false
        val percent: Int get() = if (total > 0) (done * 100 / total).toInt().coerceIn(0, 100) else 0
    }

    private val pool = Executors.newFixedThreadPool(3)
    private val jobs = ArrayList<Job>()
    private var loaded = false
    private val MEDIA = setOf("mp3", "m4a", "mp4", "m4v", "webm", "ogg", "oga", "flac", "wav", "opus", "aac", "ts", "mkv", "mov", "avi", "3gp")
    private val ALLOWED_HOSTS = setOf("commons.wikimedia.org", "upload.wikimedia.org")

    // ---- persistence ----------------------------------------------------------------------

    @Synchronized
    private fun load(ctx: Context) {
        if (loaded) return
        loaded = true
        try {
            val arr = JSONArray(ctx.getSharedPreferences("navaq_linkdl", Context.MODE_PRIVATE).getString("jobs", "[]"))
            for (i in 0 until arr.length()) {
                val o = arr.getJSONObject(i)
                val j = Job(o.getString("id"), o.getString("url"), o.getString("title"), o.getString("path"),
                    o.getBoolean("hls"), o.getBoolean("audio"), o.getBoolean("wifi"))
                j.state = o.getString("state").let { if (it == RUNNING || it == QUEUED) PAUSED else it }
                j.done = if (j.hls && j.state != DONE) 0L else o.optLong("done")
                j.total = o.optLong("total")
                j.error = o.optString("error").ifEmpty { null }
                jobs += j
            }
        } catch (_: Exception) {
        }
    }

    @Synchronized
    private fun save(ctx: Context) {
        val arr = JSONArray()
        jobs.forEach {
            arr.put(JSONObject().put("id", it.id).put("url", it.url).put("title", it.title).put("path", it.path)
                .put("hls", it.hls).put("audio", it.audioOnly).put("wifi", it.wifiOnly).put("state", it.state)
                .put("done", it.done).put("total", it.total).put("error", it.error ?: ""))
        }
        ctx.getSharedPreferences("navaq_linkdl", Context.MODE_PRIVATE).edit().putString("jobs", arr.toString()).apply()
    }

    @Synchronized
    fun list(ctx: Context): List<Job> { load(ctx); return jobs.reversed() }

    // ---- network helpers -----------------------------------------------------------------

    private fun isPrivate(a: InetAddress) =
        a.isAnyLocalAddress || a.isLoopbackAddress || a.isLinkLocalAddress || a.isSiteLocalAddress ||
            a.hostAddress?.startsWith("fc", true) == true || a.hostAddress?.startsWith("fd", true) == true

    private fun checkUrl(url: String) {
        if (!url.startsWith("https://", ignoreCase = true)) {
            throw IOException("برای دانلود امن فقط لینک HTTPS پذیرفته می‌شود")
        }
        if (!NavaqNativeBridge.nativeValidateDownloadUrl(url)) {
            throw IOException("این لینک معتبر نیست یا به آدرس محلی اشاره می‌کند")
        }
        val parsed = URL(url)
        val host = parsed.host.lowercase()
        if (parsed.userInfo != null || host !in ALLOWED_HOSTS) {
            throw IOException("دانلود در این نسخه فقط از رسانه‌های عمومی Wikimedia Commons پشتیبانی می‌شود")
        }
        if (InetAddress.getAllByName(host).any(::isPrivate)) {
            throw IOException("این میزبان به آدرس محلی اشاره می‌کند")
        }
    }

    private fun open(start: String, from: Long = 0L): HttpURLConnection {
        var url = start
        repeat(6) {
            checkUrl(url)
            val c = URL(url).openConnection() as HttpURLConnection
            c.instanceFollowRedirects = false
            c.connectTimeout = 12_000
            c.readTimeout = 25_000
            c.setRequestProperty("User-Agent", "Mozilla/5.0 (Linux; Android 14) NAVAQ")
            if (from > 0) c.setRequestProperty("Range", "bytes=$from-")
            val code = c.responseCode
            if (code in 300..399) {
                val loc = c.getHeaderField("Location") ?: throw IOException("تغییر مسیر نامعتبر")
                val next = URL(URL(url), loc).toString()
                c.disconnect()
                if (url.startsWith("https://") && next.startsWith("http://")) throw IOException("تغییر مسیر ناامن (https به http)")
                url = next
                return@repeat
            }
            if (code !in 200..299) { c.disconnect(); throw IOException("پاسخ سرور: $code") }
            return c
        }
        throw IOException("تغییر مسیرهای بیش از حد")
    }

    private fun readText(c: HttpURLConnection, max: Int): String {
        val out = java.io.ByteArrayOutputStream()
        c.inputStream.use { ins ->
            val buf = ByteArray(16 * 1024)
            while (out.size() < max) {
                val n = ins.read(buf)
                if (n < 0) break
                out.write(buf, 0, n)
            }
        }
        return out.toString("UTF-8")
    }

    private fun extOf(url: String): String =
        (url.substringBefore('?').substringBefore('#').substringAfterLast('.', "")).lowercase()

    private fun extFromType(ct: String, url: String): String {
        val e = extOf(url)
        if (e in MEDIA) return e
        return when {
            ct.contains("mpeg") && ct.startsWith("audio") -> "mp3"
            ct.contains("mp4") && ct.startsWith("audio") -> "m4a"
            ct.contains("mp4") -> "mp4"
            ct.contains("webm") -> "webm"
            ct.contains("ogg") -> "ogg"
            ct.contains("flac") -> "flac"
            ct.contains("wav") -> "wav"
            else -> "mp4"
        }
    }

    // ---- probing -------------------------------------------------------------------------

    /** Blocking: figure out what a link points to. Call off the main thread. */
    fun probe(rawUrl: String): Probe {
        val url = rawUrl.trim()
        return try {
            val c = open(url)
            val finalUrl = c.url.toString()
            val ct = (c.contentType ?: "").lowercase()
            val name = URL(finalUrl).path.substringAfterLast('/').ifBlank { URL(finalUrl).host }
            when {
                ct.contains("mpegurl") || extOf(finalUrl) == "m3u8" -> { c.disconnect(); Probe(listOf(Cand(finalUrl, "جریان HLS — $name", true, "ts")), null, null) }
                ct.startsWith("audio/") || ct.startsWith("video/") || (extOf(finalUrl) in MEDIA) -> {
                    c.disconnect()
                    Probe(listOf(Cand(finalUrl, name, false, extFromType(ct, finalUrl))), null, null)
                }
                ct.contains("html") || ct.contains("xml") -> {
                    val html = readText(c, 1_500_000)
                    c.disconnect()
                    parsePage(finalUrl, html)
                }
                else -> { c.disconnect(); Probe(emptyList(), null, "نوع این لینک ($ct) قابل دانلود نیست") }
            }
        } catch (e: Exception) {
            Probe(emptyList(), null, e.message ?: "خطا در خواندن لینک")
        }
    }

    private fun parsePage(base: String, rawHtml: String): Probe {
        val html = rawHtml.replace("\\/", "/").replace("&amp;", "&")
        val title = Regex("<title[^>]*>([^<]{1,150})</title>", RegexOption.IGNORE_CASE).find(html)?.groupValues?.get(1)?.trim()
        val found = LinkedHashSet<String>()
        fun add(ref: String) {
            try {
                val abs = URL(URL(base), ref.trim()).toString()
                if (abs.startsWith("https://") && URL(abs).host.lowercase() == "upload.wikimedia.org") found += abs
            } catch (_: Exception) {
            }
        }
        Regex("<meta[^>]+(?:property|name)=[\"'](?:og:video(?::secure_url|:url)?|og:audio(?::secure_url|:url)?|twitter:player:stream)[\"'][^>]+content=[\"']([^\"']+)[\"']", RegexOption.IGNORE_CASE)
            .findAll(html).forEach { add(it.groupValues[1]) }
        Regex("<(?:video|audio|source)[^>]+src=[\"']([^\"']+)[\"']", RegexOption.IGNORE_CASE).findAll(html).forEach { add(it.groupValues[1]) }
        Regex("https?://[^\"'\\s<>\\\\]+?\\.(?:mp4|m4a|mp3|webm|m3u8|ogg|flac|wav|opus)(?:\\?[^\"'\\s<>\\\\]*)?", RegexOption.IGNORE_CASE)
            .findAll(html).forEach { add(it.value) }
        val cands = found.take(12).map {
            val hls = extOf(it) == "m3u8"
            Cand(it, (if (hls) "HLS" else extOf(it).uppercase().ifBlank { "MEDIA" }) + " — " + (URL(it).path.substringAfterLast('/').ifBlank { URL(it).host }), hls, if (hls) "ts" else extOf(it).ifBlank { "mp4" })
        }
        return if (cands.isEmpty()) Probe(emptyList(), title,
            "فایل صوتی یا تصویری قابل دانلود در این صفحه پیدا نشد. جریان‌های محافظت‌شده و سایت‌هایی مثل یوتیوب پشتیبانی نمی‌شوند.")
        else Probe(cands, title, null)
    }

    // ---- job control ---------------------------------------------------------------------

    fun add(ctx: Context, cand: Cand, title: String, audioOnly: Boolean, wifiOnly: Boolean): Job {
        load(ctx)
        val safe = title.replace(Regex("[^\\p{L}\\p{N}._ -]"), "_").trim().take(80).trim('.').ifBlank { "navaq" }
        val ext = if (cand.hls) "ts" else cand.ext
        val file = File(Downloads.dir(ctx), "$safe-${System.currentTimeMillis() % 100000000}.$ext")
        val job = Job(System.nanoTime().toString(), cand.url, safe, file.absolutePath, cand.hls, audioOnly, wifiOnly)
        synchronized(this) { jobs += job }
        save(ctx)
        start(ctx, job)
        return job
    }

    fun start(ctx: Context, job: Job) {
        val app = ctx.applicationContext
        synchronized(job) {
            if (job.workerActive || job.state == DONE) return
            job.workerActive = true
            job.stop = null
            job.error = null
            job.speedBps = 0L
            job.etaSeconds = -1L
            job.state = QUEUED
        }
        save(app)
        NavaqTransferService.ensure(app)
        pool.execute {
            try { run(app, job) } finally {
                synchronized(job) { job.workerActive = false }
            }
        }
    }

    /** Ask a running job to stop after the current buffer; paused jobs keep their partial file. */
    fun pause(ctx: Context, job: Job) {
        if (job.state == RUNNING || job.state == QUEUED) job.stop = "pause"
        save(ctx)
    }

    /** Cancel and delete everything of this job. */
    fun remove(ctx: Context, job: Job) {
        job.stop = "cancel"
        synchronized(this) { jobs.remove(job) }
        if (job.state != RUNNING) cleanFiles(job, deleteFinal = true)
        save(ctx)
    }

    private fun cleanFiles(job: Job, deleteFinal: Boolean) {
        File(job.path + ".part").delete()
        if (deleteFinal) File(job.path).delete()
    }

    private fun metered(ctx: Context) =
        (ctx.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager).isActiveNetworkMetered

    private fun run(ctx: Context, job: Job) {
        if (job.stop != null) { job.state = PAUSED; save(ctx); return }
        job.state = RUNNING
        save(ctx)
        try {
            if (job.wifiOnly && metered(ctx)) throw IOException("اینترنت همراه فعال است؛ به وای‌فای وصل شوید")
            if (job.hls) hls(ctx, job) else direct(ctx, job)
            if (job.stop != null) {
                if (job.stop == "cancel") { cleanFiles(job, true) } else job.state = PAUSED
            } else {
                finish(ctx, job)
            }
        } catch (e: Exception) {
            if (job.stop == "pause") job.state = PAUSED
            else if (job.stop == "cancel") cleanFiles(job, true)
            else { job.state = FAILED; job.error = e.message ?: "خطا" }
        }
        save(ctx)
    }

    private fun finish(ctx: Context, job: Job) {
        if (job.audioOnly) {
            val out = File(job.path.substringBeforeLast('.') + ".m4a")
            if (out.path != job.path && extractAudio(File(job.path), out)) {
                File(job.path).delete()
                job.path = out.absolutePath
            }
        }
        job.state = DONE
        if (NavaqNotificationCenter.isEnabled(ctx)) NavaqNotificationCenter.downloadComplete(ctx, job.title, true)
    }

    private fun direct(ctx: Context, job: Job) {
        val part = File(job.path + ".part")
        var have = if (part.exists()) part.length() else 0L
        val c = open(job.url, have)
        try {
            var append = true
            if (have > 0 && c.responseCode != 206) { have = 0; append = false }
            else if (have > 0) {
                val rangeStart = Regex("bytes\\s+(\\d+)-", RegexOption.IGNORE_CASE)
                    .find(c.getHeaderField("Content-Range").orEmpty())?.groupValues?.getOrNull(1)?.toLongOrNull()
                if (rangeStart != have) throw IOException("سرور محدودهٔ ادامهٔ دانلود را تأیید نکرد")
            }
            val len = c.contentLengthLong
            job.total = if (len > 0) len + have else 0L
            job.done = have
            c.inputStream.use { ins ->
                FileOutputStream(part, append).use { out ->
                    val buf = ByteArray(64 * 1024)
                    var sampleAt = System.currentTimeMillis()
                    var sampleBytes = job.done
                    var persistedAt = sampleAt
                    var persistedBytes = job.done
                    while (true) {
                        if (job.stop != null) return
                        if (job.wifiOnly && metered(ctx)) {
                            job.stop = "pause"
                            job.error = "فقط وای‌فای فعال است؛ دانلود تا اتصال وای‌فای متوقف شد"
                            return
                        }
                        val n = ins.read(buf)
                        if (n < 0) break
                        out.write(buf, 0, n)
                        job.done += n
                        val now = System.currentTimeMillis()
                        if (now - sampleAt >= 1000L) {
                            val elapsed = (now - sampleAt).coerceAtLeast(1L)
                            job.speedBps = ((job.done - sampleBytes) * 1000L / elapsed).coerceAtLeast(0L)
                            job.etaSeconds = if (job.speedBps > 0L && job.total > job.done) (job.total - job.done) / job.speedBps else -1L
                            sampleAt = now
                            sampleBytes = job.done
                        }
                        if (job.done - persistedBytes >= 1_048_576L || now - persistedAt >= 1500L) {
                            save(ctx)
                            persistedAt = now
                            persistedBytes = job.done
                        }
                    }
                }
            }
        } finally {
            c.disconnect()
        }
        if (!part.renameTo(File(job.path))) throw IOException("ذخیره‌ی فایل ناموفق بود")
    }

    private fun resolve(base: String, ref: String) = URL(URL(base), ref.trim()).toString()

    private fun text(url: String): String {
        val c = open(url)
        try { return readText(c, 2_000_000) } finally { c.disconnect() }
    }

    private fun hls(ctx: Context, job: Job) {
        var base = job.url
        var list = text(base)
        if (list.contains("#EXT-X-STREAM-INF")) {
            var best = -1L
            var uri = ""
            var bw = 0L
            var expectUri = false
            for (line in list.lines()) {
                if (line.startsWith("#EXT-X-STREAM-INF")) {
                    bw = Regex("BANDWIDTH=(\\d+)").find(line)?.groupValues?.get(1)?.toLongOrNull() ?: 0L
                    expectUri = true
                } else if (expectUri && line.isNotBlank() && !line.startsWith("#")) {
                    if (bw > best) { best = bw; uri = line.trim() }
                    expectUri = false
                }
            }
            if (uri.isEmpty()) throw IOException("فهرست پخش خالی است")
            base = resolve(base, uri)
            list = text(base)
        }
        if (Regex("#EXT-X-KEY:.*METHOD=(?!NONE)", RegexOption.IGNORE_CASE).containsMatchIn(list)) throw IOException("این جریان رمزگذاری شده است و پشتیبانی نمی‌شود")
        if (!list.contains("#EXT-X-ENDLIST")) throw IOException("پخش زنده پشتیبانی نمی‌شود")
        val init = Regex("#EXT-X-MAP:.*URI=\"([^\"]+)\"").find(list)?.groupValues?.get(1)
        val all = (if (init != null) listOf(resolve(base, init)) else emptyList()) +
            list.lines().filter { it.isNotBlank() && !it.startsWith("#") }.map { resolve(base, it) }
        if (all.isEmpty()) throw IOException("بخشی برای دانلود پیدا نشد")
        job.total = all.size.toLong()
        val part = File(job.path + ".part")
        var index = job.done.toInt().coerceIn(0, all.size)
        if (index == 0) part.delete()
        FileOutputStream(part, index > 0).use { out ->
            while (index < all.size) {
                if (job.stop != null) return
                if (job.wifiOnly && metered(ctx)) {
                    job.stop = "pause"
                    job.error = "فقط وای‌فای فعال است؛ دانلود تا اتصال وای‌فای متوقف شد"
                    return
                }
                val c = open(all[index])
                try { c.inputStream.use { it.copyTo(out) } } finally { c.disconnect() }
                index++
                job.done = index.toLong()
                save(ctx)
            }
        }
        if (init != null && job.path.endsWith(".ts")) job.path = job.path.removeSuffix(".ts") + ".mp4"
        if (!part.renameTo(File(job.path))) throw IOException("ذخیره‌ی فایل ناموفق بود")
    }

    /** Copies the first audio track into an .m4a without re-encoding; false when the codec cannot be muxed. */
    private fun extractAudio(src: File, dst: File): Boolean {
        val ex = MediaExtractor()
        var mux: MediaMuxer? = null
        return try {
            ex.setDataSource(src.absolutePath)
            val index = (0 until ex.trackCount).firstOrNull { ex.getTrackFormat(it).getString(MediaFormat.KEY_MIME)?.startsWith("audio/") == true }
                ?: return false
            ex.selectTrack(index)
            mux = MediaMuxer(dst.absolutePath, MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4)
            val track = mux.addTrack(ex.getTrackFormat(index))
            mux.start()
            val buf = ByteBuffer.allocate(1 shl 20)
            val info = MediaCodec.BufferInfo()
            while (true) {
                buf.clear()
                val n = ex.readSampleData(buf, 0)
                if (n < 0) break
                val flags = if (ex.sampleFlags and MediaExtractor.SAMPLE_FLAG_SYNC != 0) MediaCodec.BUFFER_FLAG_KEY_FRAME else 0
                info.set(0, n, ex.sampleTime, flags)
                mux.writeSampleData(track, buf, info)
                ex.advance()
            }
            mux.stop()
            true
        } catch (_: Exception) {
            dst.delete()
            false
        } finally {
            try { mux?.release() } catch (_: Exception) { }
            ex.release()
        }
    }
}
