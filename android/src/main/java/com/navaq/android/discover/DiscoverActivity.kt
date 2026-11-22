package com.navaq.android.discover

import android.app.Activity
import android.app.DownloadManager
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.net.Uri
import android.os.Bundle
import android.os.Environment
import android.view.View
import android.webkit.DownloadListener
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import com.navaq.android.core.NavaqNativeBridge
import org.json.JSONObject
import java.net.InetAddress
import java.net.URL
import java.net.URLEncoder
import java.util.concurrent.Executors

/**
 * Native Discover host: Wikimedia Commons audio search, embedded web browser,
 * system-managed direct downloads and a local list of completed downloads.
 *
 * Network and filesystem work stays on Android. Rust remains the source of
 * truth for direct HTTP(S) URL validation through NavaqNativeBridge.
 */
class DiscoverActivity : Activity() {
    private val executor = Executors.newSingleThreadExecutor()
    private lateinit var content: LinearLayout
    private lateinit var status: TextView
    private lateinit var searchInput: EditText
    private lateinit var browserInput: EditText
    private lateinit var downloadUrlInput: EditText
    private lateinit var loadMoreButton: Button
    private var nextSearchOffset = 0
    private var activeSearchQuery = ""
    private var searchGeneration = 0
    private lateinit var resultsContainer: LinearLayout
    private lateinit var libraryContainer: LinearLayout
    private lateinit var browser: WebView
    private lateinit var progress: ProgressBar
    private lateinit var downloadManager: DownloadManager

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.statusBarColor = BG
        window.navigationBarColor = BG
        downloadManager = getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager
        setContentView(buildScreen())
        showSearch()
    }

    override fun onDestroy() {
        if (::browser.isInitialized) browser.destroy()
        executor.shutdown()
        super.onDestroy()
    }

    private fun buildScreen(): View {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(BG)
            setPadding(dp(18), dp(22), dp(18), dp(18))
        }
        root.addView(label("NAVAQ  /  DISCOVER", 12, MUTED, true))
        root.addView(label("Discover", 34, Color.WHITE, true).apply {
            setPadding(0, dp(4), 0, dp(4))
        })
        root.addView(label("Find audio, browse the web and keep music offline.", 14, SUBTLE, false)
            .apply { setPadding(0, 0, 0, dp(16)) })

        val tabs = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        tabs.addView(tabButton("Search") { showSearch() }, weightParams())
        tabs.addView(tabButton("Browser") { showBrowser() }, weightParams())
        tabs.addView(tabButton("Offline") { showLibrary() }, weightParams())
        root.addView(tabs, params(bottom = 12))

        status = label("Ready", 13, SUBTLE, false)
        root.addView(status, params(bottom = 8))
        content = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        val scroll = ScrollView(this).apply {
            isFillViewport = true
            addView(content)
        }
        root.addView(scroll, LinearLayout.LayoutParams(-1, 0, 1f))
        return root
    }

    private fun showSearch() {
        content.removeAllViews()
        searchInput = EditText(this).apply {
            hint = "Search public audio on Wikimedia Commons"
            setSingleLine(true)
            setTextColor(Color.WHITE)
            setHintTextColor(MUTED)
            setPadding(dp(14), dp(12), dp(14), dp(12))
            background = fieldBackground()
        }
        content.addView(searchInput, params(bottom = 8))
        content.addView(actionButton("Search audio") { searchAudio(searchInput.text.toString()) },
            params(bottom = 12))
        progress = ProgressBar(this).apply { visibility = View.GONE }
        content.addView(progress, params(bottom = 8))
        resultsContainer = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        content.addView(resultsContainer)
        loadMoreButton = actionButton("Load more results") {
            searchAudio(activeSearchQuery, append = true)
        }.apply { visibility = View.GONE }
        content.addView(loadMoreButton, params(bottom = 8))
        status.text = "Search uses Wikimedia Commons public audio files. Use Browser for the wider web."
    }

    private fun searchAudio(rawQuery: String, append: Boolean = false) {
        val query = if (append) activeSearchQuery else rawQuery.trim()
        if (query.isEmpty()) {
            toast("Enter a search term first.")
            return
        }
        if (query.length > 180) {
            toast("Search text is too long.")
            return
        }
        if (!append) {
            activeSearchQuery = query
            nextSearchOffset = 0
            searchGeneration += 1
            resultsContainer.removeAllViews()
        }
        val requestGeneration = searchGeneration
        val requestOffset = nextSearchOffset
        loadMoreButton.visibility = View.GONE
        progress.visibility = View.VISIBLE
        status.text = if (append) "Loading more audio…" else "Searching audio…"
        executor.execute {
            try {
                val endpoint = "https://commons.wikimedia.org/w/api.php?action=query&generator=search&gsrsearch=" +
                    URLEncoder.encode("filetype:audio $query", Charsets.UTF_8.name()) +
                    "&gsrnamespace=6&gsrlimit=25&gsroffset=$requestOffset&prop=imageinfo&iiprop=url%7Cmime%7Csize&format=json"
                val connection = URL(endpoint).openConnection()
                connection.connectTimeout = 12_000
                connection.readTimeout = 18_000
                connection.setRequestProperty("User-Agent", "NAVAQ/0.1 (Discover audio search)")
                val body = connection.getInputStream().bufferedReader().use { it.readText() }
                val response = JSONObject(body)
                val pages = response.optJSONObject("query")?.optJSONObject("pages")
                val continuation = response.optJSONObject("continue")
                val hasMore = continuation?.has("gsroffset") == true
                val followingOffset = continuation?.optInt("gsroffset", 0) ?: 0
                val found = mutableListOf<AudioHit>()
                if (pages != null) {
                    val keys = pages.keys()
                    while (keys.hasNext()) {
                        val page = pages.getJSONObject(keys.next())
                        val info = page.optJSONArray("imageinfo")?.optJSONObject(0) ?: continue
                        val url = info.optString("url")
                        val mime = info.optString("mime")
                        if (url.startsWith("https://") && mime.startsWith("audio/")) {
                            found += AudioHit(
                                title = page.optString("title").removePrefix("File:"),
                                pageUrl = "https://commons.wikimedia.org/wiki/" +
                                    URLEncoder.encode(page.optString("title").replace(' ', '_'), "UTF-8"),
                                directUrl = url,
                                mime = mime,
                                size = info.optLong("size", -1L)
                            )
                        }
                    }
                }
                runOnUiThread {
                    if (requestGeneration != searchGeneration) return@runOnUiThread
                    progress.visibility = View.GONE
                    if (!append) resultsContainer.removeAllViews()
                    if (found.isEmpty() && !append) {
                        status.text = "No audio files found. Try another phrase or use Browser."
                    } else {
                        status.text = if (append) "More results loaded • ${found.size} added" else "${found.size} audio result(s)"
                        found.forEach { addAudioHit(it) }
                    }
                    nextSearchOffset = followingOffset
                    loadMoreButton.visibility = if (hasMore && followingOffset > requestOffset) View.VISIBLE else View.GONE
                }
            } catch (error: Exception) {
                runOnUiThread {
                    if (requestGeneration != searchGeneration) return@runOnUiThread
                    progress.visibility = View.GONE
                    loadMoreButton.visibility = if (append && nextSearchOffset > 0) View.VISIBLE else View.GONE
                    status.text = "Search failed: ${error.message ?: "network error"}"
                }
            }
        }
    }

    private fun addAudioHit(hit: AudioHit) {
        val card = card()
        card.addView(label(hit.title, 16, Color.WHITE, true))
        card.addView(label(hit.mime + "  •  " + formatSize(hit.size), 12, MUTED, false)
            .apply { setPadding(0, dp(6), 0, dp(10)) })
        val actions = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        actions.addView(actionButton("Download") { enqueueDownload(hit.directUrl, hit.title) },
            LinearLayout.LayoutParams(0, dp(46), 1f).apply { marginEnd = dp(8) })
        actions.addView(actionButton("Details") { openBrowser(hit.pageUrl) },
            LinearLayout.LayoutParams(0, dp(46), 1f))
        card.addView(actions)
        resultsContainer.addView(card, params(bottom = 10))
    }

    private fun showBrowser() {
        content.removeAllViews()
        browserInput = EditText(this).apply {
            hint = "Enter a website or search the web"
            setSingleLine(true)
            setTextColor(Color.WHITE)
            setHintTextColor(MUTED)
            setPadding(dp(14), dp(12), dp(14), dp(12))
            background = fieldBackground()
        }
        content.addView(browserInput, params(bottom = 8))
        val controls = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        controls.addView(actionButton("Go") { navigateBrowser(browserInput.text.toString()) },
            LinearLayout.LayoutParams(0, dp(46), 1f).apply { marginEnd = dp(8) })
        controls.addView(actionButton("Copy page URL") {
            val current = browser.url
            if (!current.isNullOrBlank()) {
                val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
                clipboard.setPrimaryClip(android.content.ClipData.newPlainText("NAVAQ page URL", current))
                toast("Page URL copied. Paste a direct media URL below to download it.")
            }
        }, LinearLayout.LayoutParams(0, dp(46), 1f))
        content.addView(controls, params(bottom = 8))
        downloadUrlInput = EditText(this).apply {
            hint = "Paste a direct HTTP(S) media URL to download"
            setSingleLine(true)
            setTextColor(Color.WHITE)
            setHintTextColor(MUTED)
            setPadding(dp(14), dp(12), dp(14), dp(12))
            background = fieldBackground()
        }
        content.addView(downloadUrlInput, params(bottom = 8))
        content.addView(actionButton("Download direct URL") {
            enqueueDownload(downloadUrlInput.text.toString(), "web-audio")
        }, params(bottom = 8))
        val navigation = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        navigation.addView(actionButton("Back") { if (::browser.isInitialized && browser.canGoBack()) browser.goBack() }, weightParams())
        navigation.addView(actionButton("Forward") { if (::browser.isInitialized && browser.canGoForward()) browser.goForward() }, weightParams())
        navigation.addView(actionButton("Reload") { if (::browser.isInitialized) browser.reload() }, weightParams())
        content.addView(navigation, params(bottom = 8))

        if (!::browser.isInitialized) {
            browser = WebView(this).apply {
                settings.javaScriptEnabled = true
                settings.domStorageEnabled = true
                settings.allowFileAccess = false
                settings.allowContentAccess = false
                settings.allowFileAccessFromFileURLs = false
                settings.allowUniversalAccessFromFileURLs = false
                settings.setSupportMultipleWindows(false)
                if (android.os.Build.VERSION.SDK_INT >= 26) settings.safeBrowsingEnabled = true
                webChromeClient = WebChromeClient()
                webViewClient = object : WebViewClient() {
                    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                        val scheme = request.url.scheme?.lowercase()
                        return scheme != "http" && scheme != "https"
                    }

                    override fun onPageFinished(view: WebView, url: String) {
                        super.onPageFinished(view, url)
                        if (::browserInput.isInitialized) browserInput.setText(url)
                    }
                }
                setDownloadListener(DownloadListener { url, _, contentDisposition, mimeType, _ ->
                    if (::downloadUrlInput.isInitialized) downloadUrlInput.setText(url)
                    enqueueDownload(url, contentDisposition ?: mimeType ?: "web-download")
                })
            }
        }
        content.addView(browser, LinearLayout.LayoutParams(-1, dp(560)))
        status.text = "Embedded browser • only HTTP(S) pages are opened."
        if (browser.url.isNullOrBlank()) navigateBrowser("https://duckduckgo.com/")
    }

    private fun navigateBrowser(raw: String) {
        val value = raw.trim()
        if (value.isEmpty()) return
        val target = if (value.startsWith("http://") || value.startsWith("https://")) {
            value
        } else if (value.contains('.') && !value.contains(' ')) {
            "https://$value"
        } else {
            "https://duckduckgo.com/?q=" + URLEncoder.encode(value, "UTF-8")
        }
        val uri = Uri.parse(target)
        if (uri.scheme !in setOf("http", "https")) {
            toast("Only HTTP and HTTPS links are supported.")
            return
        }
        browserInput.setText(target)
        browser.loadUrl(target)
    }

    private fun openBrowser(url: String) {
        showBrowser()
        browserInput.setText(url)
        browser.loadUrl(url)
    }

    private fun showLibrary() {
        content.removeAllViews()
        content.addView(actionButton("Refresh offline library") { showLibrary() },
            params(bottom = 10))
        libraryContainer = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        content.addView(libraryContainer)
        refreshLibrary()
        status.text = "Completed downloads are saved in Music/NAVAQ and remain available offline."
    }

    private fun refreshLibrary() {
        libraryContainer.removeAllViews()
        val ids = getSharedPreferences(PREFS, MODE_PRIVATE)
            .getStringSet(KEY_DOWNLOAD_IDS, emptySet()).orEmpty()
        var visible = 0
        for (rawId in ids) {
            val id = rawId.toLongOrNull() ?: continue
            val cursor = downloadManager.query(DownloadManager.Query().setFilterById(id))
            cursor.use {
                if (!it.moveToFirst()) return@use
                val title = it.getString(it.getColumnIndexOrThrow(DownloadManager.COLUMN_TITLE))
                val statusCode = it.getInt(it.getColumnIndexOrThrow(DownloadManager.COLUMN_STATUS))
                val reason = it.getInt(it.getColumnIndexOrThrow(DownloadManager.COLUMN_REASON))
                val card = card()
                card.addView(label(title ?: "Audio download", 15, Color.WHITE, true))
                val statusLabel = when (statusCode) {
                    DownloadManager.STATUS_SUCCESSFUL -> "Available offline"
                    DownloadManager.STATUS_FAILED -> "Failed (code $reason)"
                    DownloadManager.STATUS_RUNNING -> "Downloading…"
                    DownloadManager.STATUS_PAUSED -> "Paused by system"
                    DownloadManager.STATUS_PENDING -> "Waiting in queue"
                    else -> "Download status unavailable"
                }
                val downloaded = it.getLong(it.getColumnIndexOrThrow(
                    DownloadManager.COLUMN_BYTES_DOWNLOADED_SO_FAR
                ))
                val total = it.getLong(it.getColumnIndexOrThrow(
                    DownloadManager.COLUMN_TOTAL_SIZE_BYTES
                ))
                val progressLabel = if (total > 0L) {
                    " • ${downloaded * 100L / total}%"
                } else ""
                card.addView(label(statusLabel + progressLabel, 12, MUTED, false).apply {
                    setPadding(0, dp(6), 0, dp(8))
                })
                val actions = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
                if (statusCode == DownloadManager.STATUS_SUCCESSFUL) {
                    actions.addView(actionButton("Open") {
                        val uri = downloadManager.getUriForDownloadedFile(id)
                        if (uri != null) {
                            try {
                                startActivity(Intent(Intent.ACTION_VIEW).setDataAndType(uri, itMime(title))
                                    .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION))
                            } catch (_: Exception) {
                                toast("No compatible audio app was found.")
                            }
                        }
                    }, LinearLayout.LayoutParams(0, dp(44), 1f).apply { marginEnd = dp(8) })
                }
                actions.addView(actionButton(
                    if (statusCode == DownloadManager.STATUS_RUNNING ||
                        statusCode == DownloadManager.STATUS_PENDING ||
                        statusCode == DownloadManager.STATUS_PAUSED
                    ) "Cancel" else "Remove"
                ) {
                    downloadManager.remove(id)
                    forgetDownload(id)
                    refreshLibrary()
                }, LinearLayout.LayoutParams(0, dp(44), 1f))
                card.addView(actions)
                libraryContainer.addView(card, params(bottom = 10))
                visible++
            }
        }
        if (visible == 0) {
            libraryContainer.addView(label(
                "Your offline library is empty. Search audio or download a direct link from Browser.",
                14, SUBTLE, false
            ))
        }
    }

    private fun enqueueDownload(rawUrl: String, suggestedName: String) {
        val url = rawUrl.trim()
        if (url.isEmpty()) {
            toast("There is no direct link to download.")
            return
        }
        if (!url.startsWith("https://") && !url.startsWith("http://")) {
            toast("Only direct HTTP(S) links can be downloaded.")
            return
        }
        executor.execute {
            try {
                if (!NavaqNativeBridge.nativeValidateDownloadUrl(url)) {
                    runOnUiThread { toast("This link is invalid or points to a local/private destination.") }
                    return@execute
                }
                val host = Uri.parse(url).host ?: throw IllegalArgumentException("Missing host")
                val addresses = InetAddress.getAllByName(host)
                if (addresses.isEmpty() || addresses.any { isPrivateAddress(it) }) {
                    runOnUiThread { toast("The download host resolves to a local/private address.") }
                    return@execute
                }
                val request = DownloadManager.Request(Uri.parse(url))
                    .setTitle(safeFileName(suggestedName))
                    .setDescription("NAVAQ Discover")
                    .setMimeType(guessMime(url))
                    .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)
                    .setAllowedOverMetered(true)
                    .setAllowedOverRoaming(false)
                    .setDestinationInExternalFilesDir(
                        this,
                        Environment.DIRECTORY_MUSIC,
                        "NAVAQ/${uniqueFileName(suggestedName)}"
                    )
                val id = downloadManager.enqueue(request)
                rememberDownload(id)
                runOnUiThread {
                    status.text = "Download added to Android's download queue."
                    toast("Download queued.")
                }
            } catch (error: Exception) {
                runOnUiThread { toast("Download could not be queued: ${error.message ?: "error"}") }
            }
        }
    }

    private fun rememberDownload(id: Long) {
        val prefs = getSharedPreferences(PREFS, MODE_PRIVATE)
        val ids = prefs.getStringSet(KEY_DOWNLOAD_IDS, emptySet()).orEmpty().toMutableSet()
        ids.add(id.toString())
        prefs.edit().putStringSet(KEY_DOWNLOAD_IDS, ids).apply()
    }

    private fun forgetDownload(id: Long) {
        val prefs = getSharedPreferences(PREFS, MODE_PRIVATE)
        val ids = prefs.getStringSet(KEY_DOWNLOAD_IDS, emptySet()).orEmpty().toMutableSet()
        ids.remove(id.toString())
        prefs.edit().putStringSet(KEY_DOWNLOAD_IDS, ids).apply()
    }

    private fun isPrivateAddress(address: InetAddress): Boolean =
        address.isAnyLocalAddress || address.isLoopbackAddress ||
            address.isLinkLocalAddress || address.isSiteLocalAddress ||
            address.hostAddress?.startsWith("fc", true) == true ||
            address.hostAddress?.startsWith("fd", true) == true

    private fun safeFileName(value: String): String {
        val cleaned = value.substringAfterLast('/').substringBefore('?')
            .replace(Regex("[^A-Za-z0-9._ -]"), "_")
            .trim().take(90).trim('.')
        val name = cleaned.ifBlank { "navaq-audio" }
        return if (name.contains('.')) name else "$name.${extensionForMime(guessMime(value))}"
    }

    private fun uniqueFileName(value: String): String {
        val safe = safeFileName(value)
        val dot = safe.lastIndexOf('.')
        val suffix = "-${System.currentTimeMillis()}"
        return if (dot > 0) safe.substring(0, dot) + suffix + safe.substring(dot)
        else safe + suffix
    }

    private fun guessMime(url: String): String {
        val path = Uri.parse(url).path.orEmpty().lowercase()
        return when {
            path.endsWith(".mp3") -> "audio/mpeg"
            path.endsWith(".m4a") || path.endsWith(".mp4") -> "audio/mp4"
            path.endsWith(".flac") -> "audio/flac"
            path.endsWith(".ogg") || path.endsWith(".oga") -> "audio/ogg"
            path.endsWith(".wav") -> "audio/wav"
            path.endsWith(".opus") -> "audio/opus"
            else -> "application/octet-stream"
        }
    }

    private fun extensionForMime(mime: String): String = when (mime) {
        "audio/mpeg" -> "mp3"
        "audio/mp4" -> "m4a"
        "audio/flac" -> "flac"
        "audio/ogg" -> "ogg"
        "audio/wav" -> "wav"
        "audio/opus" -> "opus"
        else -> "bin"
    }

    private fun itMime(title: String): String = when {
        title.endsWith(".mp3", true) -> "audio/mpeg"
        title.endsWith(".m4a", true) -> "audio/mp4"
        title.endsWith(".flac", true) -> "audio/flac"
        title.endsWith(".ogg", true) || title.endsWith(".oga", true) -> "audio/ogg"
        title.endsWith(".wav", true) -> "audio/wav"
        else -> "audio/*"
    }

    private fun formatSize(size: Long): String = if (size < 0) "Size unknown"
        else if (size < 1_000_000) "${size / 1_000} KB"
        else "${size / 1_000_000} MB"

    private fun card() = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(14), dp(14), dp(14), dp(14))
        background = GradientDrawable().apply {
            setColor(CARD)
            cornerRadius = dp(18).toFloat()
            setStroke(dp(1), STROKE)
        }
    }

    private fun label(value: String, size: Int, color: Int, bold: Boolean) =
        TextView(this).apply {
            text = value
            textSize = size.toFloat()
            setTextColor(color)
            typeface = if (bold) Typeface.DEFAULT_BOLD else Typeface.DEFAULT
        }

    private fun tabButton(title: String, action: () -> Unit) = Button(this).apply {
        text = title
        isAllCaps = false
        setTextColor(Color.WHITE)
        setOnClickListener { action() }
    }

    private fun actionButton(title: String, action: () -> Unit) = Button(this).apply {
        text = title
        isAllCaps = false
        textSize = 14f
        setTextColor(Color.WHITE)
        background = GradientDrawable().apply {
            setColor(ACCENT)
            cornerRadius = dp(12).toFloat()
        }
        setOnClickListener { action() }
    }

    private fun fieldBackground() = GradientDrawable().apply {
        setColor(CARD)
        cornerRadius = dp(12).toFloat()
        setStroke(dp(1), STROKE)
    }

    private fun params(bottom: Int = 0) = LinearLayout.LayoutParams(-1, -2).apply {
        bottomMargin = dp(bottom)
    }

    private fun weightParams() = LinearLayout.LayoutParams(0, dp(46), 1f)

    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()
    private fun toast(message: String) = Toast.makeText(this, message, Toast.LENGTH_SHORT).show()

    private data class AudioHit(
        val title: String,
        val pageUrl: String,
        val directUrl: String,
        val mime: String,
        val size: Long
    )

    companion object {
        private const val BG = 0xFF0A0F1D.toInt()
        private const val CARD = 0xFF141D30.toInt()
        private const val STROKE = 0xFF273650.toInt()
        private const val ACCENT = 0xFF176B72.toInt()
        private const val MUTED = 0xFF8FA4C7.toInt()
        private const val SUBTLE = 0xFFB9C7DF.toInt()
        private const val PREFS = "navaq_discover"
        private const val KEY_DOWNLOAD_IDS = "download_manager_ids"
    }
}
