package com.navaq.android.ui

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.AbsListView
import android.widget.BaseAdapter
import android.widget.FrameLayout
import android.widget.GridView
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.ListView
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.SeekBar
import android.widget.TextView
import android.window.OnBackInvokedDispatcher
import androidx.core.view.WindowCompat
import androidx.media3.common.Player
import com.navaq.android.ava.AvaActivity
import com.navaq.android.ava.AvaCatalogTrack
import com.navaq.android.ava.AvaMusicCatalog
import com.navaq.android.ava.AvaPermissions
import com.navaq.android.ava.AvaSettings
import com.navaq.android.ava.AvaSettingsActivity
import com.navaq.android.discover.DiscoverActivity
import java.text.Collator
import java.util.Locale
import java.util.concurrent.Executors

/**
 * NAVAQ's native glass music UI: library, search, mini-player, full player and a fully
 * user-customisable theme/interface settings area. Playback goes through the existing
 * Media3 session of [com.navaq.android.playback.NavaqPlaybackService].
 */
class NavaqHomeActivity : Activity() {
    private lateinit var store: UiSettingsStore
    private lateinit var favs: FavoriteStore
    private lateinit var s: UiSettings
    private lateinit var root: FrameLayout
    private lateinit var wall: WallpaperView
    private lateinit var content: FrameLayout
    private lateinit var nav: LinearLayout
    private lateinit var mini: FrameLayout
    private lateinit var client: PlaybackClient
    private val handler = Handler(Looper.getMainLooper())
    private val executor = Executors.newSingleThreadExecutor()
    private var insetTop = 0
    private var insetBottom = 0

    private var allTracks: List<AvaCatalogTrack> = emptyList()
    private var byId: Map<String, AvaCatalogTrack> = emptyMap()
    private var permissionNeeded = false
    private var screen = "library"
    private var tab = "tracks"
    private var detail: Pair<String, String>? = null
    private var query = ""
    private var settingsQuery = ""
    private val stack = ArrayList<String>()
    private var settingsScroll: ScrollView? = null
    private var keepScroll = 0
    private var currentAdapter: BaseAdapter? = null
    private var miniProgress: ProgressBar? = null
    private var lastMiniId: String? = null

    private var playerLayer: FrameLayout? = null
    private var sheetLayer: FrameLayout? = null
    private var playerQueue = false
    private var playerShownId: String? = null
    private var seeking = false
    private var pSeek: SeekBar? = null
    private var pCur: TextView? = null
    private var pDur: TextView? = null
    private var pPlay: IconView? = null
    private var pShuffle: IconView? = null
    private var pRepeat: IconView? = null
    private var pHeart: IconView? = null
    private var pCover: View? = null
    private var pBars: BarsView? = null
    private var sleepRunnable: Runnable? = null

    private val tick = object : Runnable {
        override fun run() {
            updateProgress()
            handler.postDelayed(this, 500L)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        store = UiSettingsStore(this)
        favs = FavoriteStore(this)
        s = store.load()
        GlassUi.update(this, s)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        window.statusBarColor = Color.TRANSPARENT
        window.navigationBarColor = Color.TRANSPARENT

        root = FrameLayout(this)
        root.layoutDirection = View.LAYOUT_DIRECTION_RTL
        wall = WallpaperView(this)
        root.addView(wall, fill())
        content = FrameLayout(this)
        root.addView(content, fill())
        mini = FrameLayout(this)
        root.addView(mini)
        nav = LinearLayout(this)
        root.addView(nav)
        setContentView(root)
        root.setOnApplyWindowInsetsListener { _, insets ->
            @Suppress("DEPRECATION")
            run {
                val top = insets.stableInsetTop
                val bottom = insets.stableInsetBottom
                val changed = top != insetTop || bottom != insetBottom
                insetTop = top
                insetBottom = bottom
                if (changed) rebuildAll()
            }
            insets
        }
        if (Build.VERSION.SDK_INT >= 33) {
            onBackInvokedDispatcher.registerOnBackInvokedCallback(OnBackInvokedDispatcher.PRIORITY_DEFAULT) {
                if (!handleBack()) finish()
            }
        }
        client = PlaybackClient(this) { onPlayback() }
        client.connect()
        loadTracks()
        rebuildAll()
        handler.post(tick)
    }

    override fun onResume() {
        super.onResume()
        if (allTracks.isEmpty() && AvaPermissions.hasAudioLibraryAccess(this)) loadTracks()
    }

    override fun onDestroy() {
        handler.removeCallbacksAndMessages(null)
        client.release()
        executor.shutdownNow()
        super.onDestroy()
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) loadTracks()
    }

    @Deprecated("Kept for API < 33")
    override fun onBackPressed() {
        if (!handleBack()) super.onBackPressed()
    }

    private fun handleBack(): Boolean {
        when {
            sheetLayer != null -> dismissSheet()
            playerLayer != null -> closePlayer()
            screen == "settings" && stack.isNotEmpty() -> { stack.removeAt(stack.lastIndex); buildScreen() }
            detail != null -> { detail = null; buildScreen() }
            else -> return false
        }
        return true
    }

    private fun fill() = FrameLayout.LayoutParams(-1, -1)

    // ------------------------------------------------------------------ settings changes

    private fun mutate(rebuild: Boolean = true, change: (UiSettings) -> UiSettings) {
        if (rebuild) keepScroll = settingsScroll?.scrollY ?: 0
        s = change(s).sanitized()
        store.save(s)
        GlassUi.update(this, s)
        wall.configure()
        if (rebuild) rebuildAll() else refreshTree(root)
    }

    private fun rebuildAll() {
        GlassUi.update(this, s)
        root.setBackgroundColor(T.bg)
        wall.configure()
        val c = WindowCompat.getInsetsController(window, root)
        c.isAppearanceLightStatusBars = !T.dark
        c.isAppearanceLightNavigationBars = !T.dark
        buildNav()
        updateMini(force = true)
        buildScreen()
        if (playerLayer != null) buildPlayer()
    }

    // ------------------------------------------------------------------ data

    private fun loadTracks() {
        if (!AvaPermissions.hasAudioLibraryAccess(this)) {
            permissionNeeded = true
            allTracks = emptyList()
            byId = emptyMap()
            return
        }
        permissionNeeded = false
        executor.execute {
            val list = try {
                AvaMusicCatalog(this).search(null, AvaSettings())
            } catch (_: Exception) {
                emptyList()
            }
            runOnUiThread {
                allTracks = list
                byId = list.associateBy { it.id }
                buildScreen()
                updateMini(force = true)
            }
        }
    }

    private fun visibleTracks(): List<AvaCatalogTrack> {
        val min = s.minTrackSeconds * 1000L
        val collator = Collator.getInstance(Locale("fa"))
        val list = allTracks.filter { (it.durationMs ?: Long.MAX_VALUE) >= min }
        return when (s.sort) {
            SortOrder.TITLE -> list.sortedWith { a, b -> collator.compare(a.title, b.title) }
            SortOrder.ARTIST -> list.sortedWith { a, b -> collator.compare(a.artist.orEmpty(), b.artist.orEmpty()) }
            SortOrder.DURATION -> list.sortedByDescending { it.durationMs ?: 0L }
        }
    }

    // ------------------------------------------------------------------ chrome

    private fun navTotal(): Int =
        if (s.navStyle == NavStyle.FLOATING) T.dp(64) + insetBottom + T.dp(14) else T.dp(74) + insetBottom

    private fun buildNav() {
        nav.removeAllViews()
        nav.orientation = LinearLayout.HORIZONTAL
        nav.gravity = Gravity.CENTER
        if (s.navStyle == NavStyle.FLOATING) {
            nav.glass(1f)
            nav.setPadding(T.dp(6), T.dp(6), T.dp(6), T.dp(6))
            nav.layoutParams = FrameLayout.LayoutParams(-1, T.dp(64), Gravity.BOTTOM).apply {
                setMargins(T.dp(14), 0, T.dp(14), insetBottom + T.dp(14))
            }
        } else {
            nav.glass(1f, topOnly = true)
            nav.setPadding(T.dp(6), T.dp(6), T.dp(6), insetBottom)
            nav.layoutParams = FrameLayout.LayoutParams(-1, T.dp(74) + insetBottom, Gravity.BOTTOM)
        }
        val items = listOf(
            Triple("library", "کتابخانه", Icon.LIBRARY),
            Triple("search", "جستجو", Icon.SEARCH),
            Triple("settings", "تنظیمات", Icon.SLIDERS)
        )
        items.forEach { (id, text, icon) ->
            val on = screen == id
            val tint = if (on) T.accent else T.fg2
            nav.addView(LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                gravity = Gravity.CENTER
                background = GradientDrawable().apply {
                    cornerRadius = T.radiusPx * 0.75f
                    setColor(if (on) (T.accent and 0x00FFFFFF) or (0x38 shl 24) else Color.TRANSPARENT)
                }
                addView(IconView(this@NavaqHomeActivity, icon, tint), LinearLayout.LayoutParams(T.dp(22), T.dp(22)))
                addView(label(this@NavaqHomeActivity, text, 11f, on, tint).apply { setPadding(0, T.dp(2), 0, 0) })
                pressable {
                    if (screen != id) {
                        screen = id
                        detail = null
                        buildNav()
                        buildScreen()
                    }
                }
            }, LinearLayout.LayoutParams(0, -1, 1f).apply { setMargins(T.dp(2), 0, T.dp(2), 0) })
        }
    }

    private fun btn(icon: Icon, box: Int, glyph: Int, tint: Int = T.fg, onClick: () -> Unit): FrameLayout =
        FrameLayout(this).apply {
            addView(IconView(this@NavaqHomeActivity, icon, tint), FrameLayout.LayoutParams(T.dp(glyph), T.dp(glyph), Gravity.CENTER))
            layoutParams = LinearLayout.LayoutParams(T.dp(box), T.dp(box))
            pressable(onClick)
        }

    private fun iconOf(b: View): IconView = (b as FrameLayout).getChildAt(0) as IconView

    private fun updateMini(force: Boolean = false) {
        val id = client.currentId
        val track = id?.let { byId[it] }
        if (!force && id == lastMiniId && miniProgress != null) {
            (mini.getChildAt(0) as? LinearLayout)?.let { row ->
                val playBtn = row.getChildAt(2) as? FrameLayout
                playBtn?.let { iconOf(it).icon = if (client.isPlaying) Icon.PAUSE else Icon.PLAY }
            }
            return
        }
        lastMiniId = id
        mini.removeAllViews()
        if (track == null) { mini.visibility = View.GONE; miniProgress = null; return }
        mini.visibility = View.VISIBLE
        mini.glass(0.9f)
        mini.layoutParams = FrameLayout.LayoutParams(-1, T.dp(64), Gravity.BOTTOM).apply {
            setMargins(T.dp(14), 0, T.dp(14), navTotal() + T.dp(8))
        }
        val row = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(T.dp(9), T.dp(9), T.dp(8), T.dp(9))
        }
        row.addView(CoverView(this).apply { bind(track.id, track.sourceValue, track.title) }, LinearLayout.LayoutParams(T.dp(46), T.dp(46)))
        row.addView(LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(label(this@NavaqHomeActivity, track.title, 14f, true, T.fg, 1))
            addView(label(this@NavaqHomeActivity, track.artist ?: "هنرمند نامشخص", 12f, false, T.fg2, 1).apply { setPadding(0, T.dp(2), 0, 0) })
        }, LinearLayout.LayoutParams(0, -2, 1f).apply { marginStart = T.dp(10); marginEnd = T.dp(6) })
        row.addView(btn(if (client.isPlaying) Icon.PAUSE else Icon.PLAY, 44, 24) { client.toggle() })
        row.addView(btn(Icon.NEXT, 44, 22) { client.next() })
        mini.addView(row, FrameLayout.LayoutParams(-1, -1))
        // Row children: cover(0) text(1) play(2) next(3)
        val bar = ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal).apply {
            max = 1000
            progressTintList = android.content.res.ColorStateList.valueOf(T.accent)
            progressBackgroundTintList = android.content.res.ColorStateList.valueOf(Color.TRANSPARENT)
        }
        miniProgress = bar
        mini.addView(bar, FrameLayout.LayoutParams(-1, T.dp(3), Gravity.BOTTOM).apply { setMargins(T.dp(14), 0, T.dp(14), T.dp(2)) })
        mini.pressable { openPlayer() }
    }

    // ------------------------------------------------------------------ screens

    private fun column(): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(0, insetTop + T.dp(8), 0, 0)
    }

    private fun header(title: String, back: (() -> Unit)?, vararg actions: View): View =
        LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(T.dp(if (back != null) 8 else 20), T.dp(4), T.dp(12), T.dp(4))
            if (back != null) addView(btn(Icon.BACK, 44, 24) { back() })
            addView(label(this@NavaqHomeActivity, title, if (back != null) 22f else 30f, true, T.fg, 1), LinearLayout.LayoutParams(0, -2, 1f))
            actions.forEach { addView(it) }
        }

    private fun buildScreen() {
        content.removeAllViews()
        currentAdapter = null
        settingsScroll = null
        val v = when (screen) {
            "search" -> searchScreen()
            "settings" -> settingsScreen()
            else -> libraryScreen()
        }
        content.addView(v, fill())
    }

    private fun libraryScreen(): View {
        val col = column()
        val d = detail
        if (d != null) {
            col.addView(header(d.second, { detail = null; buildScreen() }))
            val list = visibleTracks().filter { if (d.first == "album") it.album == d.second else it.artist == d.second }
            col.addView(trackPane(list), LinearLayout.LayoutParams(-1, 0, 1f))
            return col
        }
        col.addView(header("موسیقی", null,
            btn(Icon.SEARCH, 44, 22) { screen = "search"; buildNav(); buildScreen() },
            btn(Icon.SLIDERS, 44, 22) { screen = "settings"; buildNav(); buildScreen() }))
        val tabs = mutableListOf("tracks" to "آهنگ‌ها", "albums" to "آلبوم‌ها")
        if (s.showArtistsTab) tabs += "artists" to "هنرمندان"
        tabs += "fav" to "علاقه‌مندی‌ها"
        if (tabs.none { it.first == tab }) tab = "tracks"
        col.addView(HorizontalScrollView(this).apply {
            isHorizontalScrollBarEnabled = false
            addView(LinearLayout(this@NavaqHomeActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                setPadding(T.dp(20), T.dp(8), T.dp(20), T.dp(12))
                tabs.forEach { (id, text) ->
                    val on = tab == id
                    addView(label(this@NavaqHomeActivity, text, 14f, on, if (on) T.accentFg else T.fg).apply {
                        setPadding(T.dp(16), T.dp(8), T.dp(16), T.dp(8))
                        background = GradientDrawable().apply {
                            cornerRadius = T.dpf(40f)
                            setColor(if (on) T.accent else Color.argb(if (T.dark) 40 else 130, 128, 134, 160))
                        }
                        pressable { tab = id; buildScreen() }
                    }, LinearLayout.LayoutParams(-2, -2).apply { marginEnd = T.dp(8) })
                }
            })
        })
        val pane: View = when {
            permissionNeeded -> permissionCard()
            tab == "albums" -> {
                val groups = visibleTracks().filter { !it.album.isNullOrBlank() }.groupBy { it.album!! }
                listPane(groups.map { (name, tr) ->
                    Cell("album:$name", name, tr.first().artist ?: "", tr.first().sourceValue, name, null) {
                        detail = "album" to name; buildScreen()
                    }
                }, grid = true)
            }
            tab == "artists" -> {
                val groups = visibleTracks().filter { !it.artist.isNullOrBlank() }.groupBy { it.artist!! }
                listPane(groups.map { (name, tr) ->
                    Cell("artist:$name", name, "${fa(tr.size)} آهنگ", tr.first().sourceValue, name, null) {
                        detail = "artist" to name; buildScreen()
                    }
                }, grid = false)
            }
            tab == "fav" -> trackPane(visibleTracks().filter { favs.isFavorite(it.id) })
            else -> trackPane(visibleTracks())
        }
        col.addView(pane, LinearLayout.LayoutParams(-1, 0, 1f))
        return col
    }

    private fun permissionCard(): View = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        gravity = Gravity.CENTER
        glass()
        setPadding(T.dp(20), T.dp(24), T.dp(20), T.dp(24))
        addView(label(this@NavaqHomeActivity, "برای نمایش موسیقی‌های گوشی به دسترسی نیاز است", 15f, true).apply { gravity = Gravity.CENTER })
        addView(label(this@NavaqHomeActivity, "اجازه دادن", 14f, true, T.accentFg).apply {
            setPadding(T.dp(22), T.dp(10), T.dp(22), T.dp(10))
            background = GradientDrawable().apply { cornerRadius = T.dpf(40f); setColor(T.accent) }
            pressable { requestPermissions(arrayOf(AvaPermissions.requiredAudioPermission()), 11) }
        }, LinearLayout.LayoutParams(-2, -2).apply { topMargin = T.dp(16) })
        layoutParams = LinearLayout.LayoutParams(-1, -2).apply { setMargins(T.dp(16), T.dp(24), T.dp(16), 0) }
    }.let { card -> FrameLayout(this).apply { addView(card, FrameLayout.LayoutParams(-1, -2).apply { setMargins(T.dp(16), T.dp(24), T.dp(16), 0) }) } }

    private fun playFrom(list: List<AvaCatalogTrack>, index: Int) {
        if (!client.isConnected) { toast("پخش‌کننده هنوز آماده نیست"); return }
        client.playQueue(list, index)
    }

    private fun trackPane(list: List<AvaCatalogTrack>): View {
        if (list.isEmpty()) return emptyState(if (tab == "fav") "هنوز آهنگی را نپسندیده‌اید" else "آهنگی پیدا نشد")
        val col = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        col.addView(LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            setPadding(T.dp(16), 0, T.dp(16), T.dp(8))
            fun action(text: String, icon: Icon, primary: Boolean, run: () -> Unit) {
                addView(LinearLayout(this@NavaqHomeActivity).apply {
                    orientation = LinearLayout.HORIZONTAL
                    gravity = Gravity.CENTER
                    setPadding(0, T.dp(12), 0, T.dp(12))
                    if (primary) background = GradientDrawable().apply { cornerRadius = T.radiusPx * 0.8f; setColor(T.accent) } else glass(0.8f)
                    val tint = if (primary) T.accentFg else T.fg
                    addView(IconView(this@NavaqHomeActivity, icon, tint), LinearLayout.LayoutParams(T.dp(20), T.dp(20)))
                    addView(label(this@NavaqHomeActivity, text, 14f, true, tint).apply { setPadding(T.dp(8), 0, 0, 0) })
                    pressable(run)
                }, LinearLayout.LayoutParams(0, -2, 1f).apply { setMargins(T.dp(4), 0, T.dp(4), 0) })
            }
            action("پخش همه", Icon.PLAY, true) { playFrom(list, 0) }
            action("درهم", Icon.SHUFFLE, false) { playFrom(list.shuffled(), 0) }
        })
        val cells = list.mapIndexed { i, t ->
            Cell(t.id, t.title, listOfNotNull(t.artist, if (s.showDuration) t.durationMs?.let { fmtTime(it) } else null).joinToString(" · "), t.sourceValue, t.title, t) {
                playFrom(list, i)
            }
        }
        col.addView(listPane(cells, s.listStyle == ListStyle.GRID), LinearLayout.LayoutParams(-1, 0, 1f))
        return col
    }

    private fun emptyState(text: String): View = FrameLayout(this).apply {
        addView(label(this@NavaqHomeActivity, text, 15f, false, T.fg2).apply { gravity = Gravity.CENTER }, FrameLayout.LayoutParams(-1, -2, Gravity.CENTER))
    }

    private class Cell(
        val key: String, val title: String, val sub: String, val uri: String?, val seed: String,
        val track: AvaCatalogTrack?, val onClick: () -> Unit
    )

    private class Holder(
        val root: View, val cover: CoverView, val title: TextView, val sub: TextView,
        val bars: BarsView?, val heart: IconView?, val heartBox: View?
    )

    private class SquareLayout(context: Context) : FrameLayout(context) {
        override fun onMeasure(widthSpec: Int, heightSpec: Int) = super.onMeasure(widthSpec, widthSpec)
    }

    private fun makeHolder(grid: Boolean): Holder {
        val cover = CoverView(this)
        if (grid) {
            val title = label(this, "", 14f, true, T.fg, 1)
            val sub = label(this, "", 12f, false, T.fg2, 1)
            val rootV = LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                addView(SquareLayout(this@NavaqHomeActivity).apply { addView(cover, fill()) }, LinearLayout.LayoutParams(-1, -2))
                addView(title, LinearLayout.LayoutParams(-1, -2).apply { topMargin = T.dp(8) })
                addView(sub, LinearLayout.LayoutParams(-1, -2).apply { topMargin = T.dp(2) })
            }
            return Holder(rootV, cover, title, sub, null, null, null)
        }
        val title = label(this, "", 15f, true, T.fg, 1)
        val sub = label(this, "", 12.5f, false, T.fg2, 1)
        val bars = BarsView(this)
        val heart = IconView(this, Icon.HEART, T.fg2)
        val heartBox = FrameLayout(this).apply {
            addView(heart, FrameLayout.LayoutParams(T.dp(22), T.dp(22), Gravity.CENTER))
        }
        val rootV = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(T.dp(6), T.dp(T.rowPadDp), T.dp(2), T.dp(T.rowPadDp))
            addView(cover, LinearLayout.LayoutParams(T.dp(52), T.dp(52)))
            addView(LinearLayout(this@NavaqHomeActivity).apply {
                orientation = LinearLayout.VERTICAL
                addView(title)
                addView(sub, LinearLayout.LayoutParams(-2, -2).apply { topMargin = T.dp(3) })
            }, LinearLayout.LayoutParams(0, -2, 1f).apply { marginStart = T.dp(12); marginEnd = T.dp(8) })
            addView(bars, LinearLayout.LayoutParams(T.dp(18), T.dp(16)).apply { marginEnd = T.dp(6) })
            addView(heartBox, LinearLayout.LayoutParams(T.dp(44), T.dp(44)))
        }
        return Holder(rootV, cover, title, sub, bars, heart, heartBox)
    }

    private fun listPane(cells: List<Cell>, grid: Boolean, primary: Boolean = true): View {
        if (cells.isEmpty()) return emptyState("موردی پیدا نشد")
        val adapter = object : BaseAdapter() {
            override fun getCount() = cells.size
            override fun getItem(position: Int) = cells[position]
            override fun getItemId(position: Int) = position.toLong()
            override fun getView(position: Int, convertView: View?, parent: ViewGroup?): View {
                val h = (convertView?.tag as? Holder) ?: makeHolder(grid).also { it.root.tag = it }
                val c = cells[position]
                h.title.text = c.title
                h.sub.text = c.sub
                h.cover.bind(c.key, c.uri, c.seed)
                val current = c.track != null && c.track.id == client.currentId
                h.title.setTextColor(if (current) T.accent else T.fg)
                h.bars?.apply { visibility = if (current) View.VISIBLE else View.GONE; playing = client.isPlaying }
                if (!grid) {
                    h.root.background = if (current) GradientDrawable().apply {
                        cornerRadius = T.radiusPx * 0.6f
                        setColor(Color.argb(if (T.dark) 40 else 120, 128, 134, 160))
                    } else null
                    if (c.track != null) {
                        h.heartBox?.visibility = View.VISIBLE
                        val fav = favs.isFavorite(c.track.id)
                        h.heart?.icon = if (fav) Icon.HEART_FILL else Icon.HEART
                        h.heart?.tint = if (fav) T.accent else T.fg2
                        h.heartBox?.pressable {
                            val now = favs.toggle(c.track.id)
                            toast(if (now) "به علاقه‌مندی‌ها افزوده شد" else "از علاقه‌مندی‌ها برداشته شد")
                            notifyDataSetChanged()
                            pHeart?.let { ph -> if (client.currentId == c.track.id) { ph.icon = if (now) Icon.HEART_FILL else Icon.HEART; ph.tint = if (now) T.accent else T.fg } }
                        }
                    } else {
                        h.heartBox?.visibility = View.GONE
                    }
                }
                return h.root
            }
        }
        if (primary) currentAdapter = adapter
        val pad = navTotal() + T.dp(84)
        val list: AbsListView = if (grid) GridView(this).apply {
            numColumns = 2
            horizontalSpacing = T.dp(14)
            verticalSpacing = T.dp(14)
            this.adapter = adapter
        } else ListView(this).apply {
            divider = null
            dividerHeight = 0
            this.adapter = adapter
        }
        list.apply {
            clipToPadding = false
            setPadding(T.dp(16), 0, T.dp(16), pad)
            selector = ColorDrawable(Color.TRANSPARENT)
            isVerticalScrollBarEnabled = false
            setOnItemClickListener { _, _, position, _ -> cells[position].onClick() }
        }
        return list
    }

    private fun searchScreen(): View {
        val col = column()
        col.addView(header("جستجو", null))
        val results = FrameLayout(this)
        fun refresh() {
            results.removeAllViews()
            val needle = query.trim().replace('ي', 'ی').replace('ك', 'ک').lowercase(Locale.ROOT)
            if (needle.isEmpty()) {
                results.addView(emptyState("نام آهنگ، هنرمند یا آلبوم را بنویسید"))
                return
            }
            val found = visibleTracks().filter {
                listOfNotNull(it.title, it.artist, it.album).joinToString(" ").replace('ي', 'ی').replace('ك', 'ک')
                    .lowercase(Locale.ROOT).contains(needle)
            }
            results.addView(
                if (found.isEmpty()) emptyState("نتیجه‌ای پیدا نشد")
                else listPane(found.mapIndexed { i, t ->
                    Cell(t.id, t.title, listOfNotNull(t.artist, t.album).joinToString(" · "), t.sourceValue, t.title, t) { playFrom(found, i) }
                }, false)
            )
        }
        col.addView(SettingsKit.searchBox(this, "آهنگ، هنرمند یا آلبوم", query) { query = it; refresh() },
            LinearLayout.LayoutParams(-1, -2).apply { setMargins(T.dp(16), T.dp(4), T.dp(16), T.dp(8)) })
        refresh()
        col.addView(results, LinearLayout.LayoutParams(-1, 0, 1f))
        return col
    }

    // ------------------------------------------------------------------ settings

    private sealed class Item {
        class Sec(val text: String) : Item()
        class Row(val title: String, val keywords: String, val view: () -> View) : Item()
    }

    private fun runtimePrefs() = getSharedPreferences("navaq_ui_runtime", MODE_PRIVATE)

    private fun pageTitle(id: String) = when (id) {
        "look" -> "تم و رنگ"
        "glass" -> "شیشه و جلوه‌ها"
        "layout" -> "چیدمان و رابط"
        "playback" -> "پخش"
        "library" -> "کتابخانه"
        "backup" -> "پشتیبان و بازنشانی"
        "about" -> "درباره"
        else -> "تنظیمات"
    }

    private fun toast(text: String) = android.widget.Toast.makeText(this, text, android.widget.Toast.LENGTH_SHORT).show()

    private fun applyPreset(p: UiSettings) = mutate {
        it.copy(
            theme = p.theme, accent = p.accent, wallpaper = p.wallpaper, blurPx = p.blurPx,
            glassOpacityPercent = p.glassOpacityPercent, cornerRadiusDp = p.cornerRadiusDp, edgeGlow = p.edgeGlow
        )
    }

    private fun slider(title: String, min: Int, max: Int, value: Int, unit: String, live: Boolean, change: (UiSettings, Int) -> UiSettings): Item.Row =
        Item.Row(title, title) {
            SettingsKit.sliderRow(this, title, min, max, value, { fa(it) + unit },
                { v -> if (live) mutate(false) { change(it, v) } },
                { v -> mutate(true) { change(it, v) } })
        }

    private fun toggle(title: String, desc: String?, value: Boolean, change: (UiSettings, Boolean) -> UiSettings): Item.Row =
        Item.Row(title, desc.orEmpty()) {
            SettingsKit.toggleRow(this, title, desc, value) { v -> mutate(true) { change(it, v) } }
        }

    private fun <E : Enum<E>> choice(title: String, options: List<Pair<E, String>>, value: E, change: (UiSettings, E) -> UiSettings): Item.Row =
        Item.Row(title, options.joinToString(" ") { it.second }) {
            SettingsKit.choiceRow(this, title, options.map { it.first.name to it.second }, value.name) { picked ->
                val e = options.first { it.first.name == picked }.first
                mutate(true) { change(it, e) }
            }
        }

    private fun pageItems(id: String): List<Item> = when (id) {
        "look" -> listOf(
            Item.Sec("قالب‌های آماده"),
            Item.Row("قالب", "شفق شکوفه نیمه‌شب مینیمال") {
                LinearLayout(this).apply {
                    setPadding(0, T.dp(12), 0, T.dp(12))
                    addView(SettingsKit.chips(this@NavaqHomeActivity, listOf(
                        "a" to "شفق شیشه‌ای", "b" to "شکوفه", "c" to "نیمه‌شب", "d" to "مینیمال"), "") { k ->
                        applyPreset(when (k) { "a" -> UiSettings.AuroraGlass; "b" -> UiSettings.Sakura; "c" -> UiSettings.Midnight; else -> UiSettings.Minimal })
                    }, LinearLayout.LayoutParams(-1, -2))
                }
            },
            Item.Sec("حالت نمایش"),
            choice("تم", listOf(ThemeMode.SYSTEM to "خودکار", ThemeMode.LIGHT to "روشن", ThemeMode.DARK to "تاریک"), s.theme) { u, v -> u.copy(theme = v) },
            Item.Sec("رنگ"),
            Item.Row("رنگ تأکید", "accent color hex") {
                SettingsKit.swatchRow(this, s.accent, { hex -> mutate { it.copy(accent = hex) } }, { toast("رنگ باید به شکل #rrggbb باشد") })
            },
            choice("تصویر زمینه", listOf(
                Wallpaper.AURORA to "شفق", Wallpaper.SUNSET to "غروب", Wallpaper.OCEAN to "اقیانوس",
                Wallpaper.FOREST to "جنگل", Wallpaper.MONO to "خاکستری", Wallpaper.ALBUM to "هم‌رنگ آهنگ"), s.wallpaper) { u, v -> u.copy(wallpaper = v) }
        )
        "glass" -> listOf(
            Item.Sec("شیشه"),
            slider("محو‌شدگی زمینه", 0, 60, s.blurPx, " px", true) { u, v -> u.copy(blurPx = v) },
            slider("شفافیت شیشه", 4, 85, s.glassOpacityPercent, "٪", true) { u, v -> u.copy(glassOpacityPercent = v) },
            slider("گردی گوشه‌ها", 6, 38, s.cornerRadiusDp, " dp", true) { u, v -> u.copy(cornerRadiusDp = v) },
            toggle("درخشش لبه‌ها", "هاله‌ای به رنگ تأکید دور پنل‌ها", s.edgeGlow) { u, v -> u.copy(edgeGlow = v) },
            Item.Sec("حرکت"),
            toggle("انیمیشن‌ها", "حرکت زمینه و نوارهای پخش", s.animations) { u, v -> u.copy(animations = v) },
            toggle("نوار موج در پلیر", "نمایش نشانگر پخش زیر کاور", s.visualizer) { u, v -> u.copy(visualizer = v) },
            toggle("چرخش کاور دایره‌ای", "فقط با شکل کاور «دایره»", s.spinCircularCover) { u, v -> u.copy(spinCircularCover = v) }
        )
        "layout" -> listOf(
            Item.Sec("نوشته و فاصله"),
            slider("اندازه متن", 85, 130, s.fontScalePercent, "٪", false) { u, v -> u.copy(fontScalePercent = v) },
            choice("تراکم فهرست", listOf(Density.COMPACT to "فشرده", Density.COZY to "متعادل", Density.ROOMY to "گشاد"), s.density) { u, v -> u.copy(density = v) },
            Item.Sec("کاور و فهرست"),
            choice("شکل کاور", listOf(CoverShape.SQUARE to "مربع", CoverShape.ROUNDED to "گرد", CoverShape.CIRCLE to "دایره"), s.coverShape) { u, v -> u.copy(coverShape = v) },
            choice("نمایش آهنگ‌ها", listOf(ListStyle.LIST to "فهرست", ListStyle.GRID to "شبکه"), s.listStyle) { u, v -> u.copy(listStyle = v) },
            toggle("نمایش مدت آهنگ", null, s.showDuration) { u, v -> u.copy(showDuration = v) },
            Item.Sec("ناوبری"),
            choice("نوار پایین", listOf(NavStyle.FLOATING to "شناور", NavStyle.DOCKED to "چسبیده"), s.navStyle) { u, v -> u.copy(navStyle = v) },
            toggle("تب هنرمندان", null, s.showArtistsTab) { u, v -> u.copy(showArtistsTab = v) }
        )
        "playback" -> listOf(
            Item.Sec("حس موسیقی"),
            Item.Row("لرزش همراه ضرب", "haptic vibration beat") {
                SettingsKit.toggleRow(this, "لرزش همراه ضرب", "ضربه‌های موسیقی را با لرزش حس کنید", runtimePrefs().getBoolean("haptics", false)) { v ->
                    runtimePrefs().edit().putBoolean("haptics", v).apply()
                    client.setBeatHaptics(v)
                }
            },
            Item.Sec("دستیار و کشف"),
            Item.Row("دستیار صوتی AVA", "ava voice assistant") {
                SettingsKit.navRow(this, Icon.MIC, "دستیار صوتی AVA", "فرمان صوتی آفلاین فارسی و انگلیسی") { startActivity(Intent(this, AvaActivity::class.java)) }
            },
            Item.Row("تنظیمات AVA", "ava settings") {
                SettingsKit.navRow(this, Icon.SLIDERS, "تنظیمات AVA", "زبان، منبع موسیقی و حالت پاسخ") { startActivity(Intent(this, AvaSettingsActivity::class.java)) }
            },
            Item.Row("کشف موسیقی", "discover download") {
                SettingsKit.navRow(this, Icon.DISC, "کشف موسیقی", "جستجو و دانلود قانونی") { startActivity(Intent(this, DiscoverActivity::class.java)) }
            },
            Item.Row("راهنما", "") { SettingsKit.noteRow(this, "سرعت پخش، درهم، تکرار و تایمر خواب از داخل پلیر قابل تنظیم هستند.") }
        )
        "library" -> listOf(
            Item.Sec("فهرست"),
            choice("مرتب‌سازی", listOf(SortOrder.TITLE to "نام", SortOrder.ARTIST to "هنرمند", SortOrder.DURATION to "مدت"), s.sort) { u, v -> u.copy(sort = v) },
            slider("حداقل طول آهنگ", 0, 120, s.minTrackSeconds, " ثانیه", false) { u, v -> u.copy(minTrackSeconds = v) },
            Item.Row("اسکن دوباره", "rescan") {
                SettingsKit.actionRow(this, Icon.RESET, "اسکن دوباره کتابخانه", "خواندن دوباره موسیقی‌های گوشی") { loadTracks(); toast("در حال بروزرسانی کتابخانه") }
            }
        )
        "backup" -> listOf(
            Item.Row("بازنشانی", "reset default") {
                SettingsKit.actionRow(this, Icon.RESET, "بازگشت به پیش‌فرض", "تنظیمات ظاهری به حالت اولیه برمی‌گردد") { mutate { UiSettings() } }
            }
        )
        "about" -> listOf(
            Item.Row("درباره", "") {
                SettingsKit.noteRow(this, "NAVAQ — پخش‌کننده‌ی شیشه‌ای بر پایه‌ی هسته‌ی Rust. تنظیمات ظاهری با ماژول ui_settings هسته هم‌ساختار است.")
            }
        )
        else -> listOf(
            Item.Row("تم و رنگ", "theme color wallpaper") { SettingsKit.navRow(this, Icon.PALETTE, "تم و رنگ", "حالت روشن و تاریک، رنگ، تصویر زمینه، قالب‌ها") { open("look") } },
            Item.Row("شیشه و جلوه‌ها", "glass blur") { SettingsKit.navRow(this, Icon.GLASS, "شیشه و جلوه‌ها", "محو‌شدگی، شفافیت، گوشه‌ها، انیمیشن") { open("glass") } },
            Item.Row("چیدمان و رابط", "layout font density") { SettingsKit.navRow(this, Icon.LAYOUT, "چیدمان و رابط", "اندازه متن، تراکم، کاور، ناوبری") { open("layout") } },
            Item.Row("پخش", "playback haptic ava") { SettingsKit.navRow(this, Icon.PLAY, "پخش", "لرزش، دستیار AVA، کشف موسیقی") { open("playback") } },
            Item.Row("کتابخانه", "library sort") { SettingsKit.navRow(this, Icon.LIBRARY, "کتابخانه", "مرتب‌سازی و حداقل طول") { open("library") } },
            Item.Row("پشتیبان و بازنشانی", "reset") { SettingsKit.navRow(this, Icon.RESET, "بازنشانی", "بازگشت به پیش‌فرض") { open("backup") } },
            Item.Row("درباره", "about") { SettingsKit.navRow(this, Icon.INFO, "درباره", "NAVAQ Core") { open("about") } }
        )
    }

    private fun open(page: String) { stack.add(page); buildScreen() }

    private fun renderItems(items: List<Item>): LinearLayout {
        val out = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        val pending = ArrayList<View>()
        fun flush() {
            if (pending.isNotEmpty()) out.addView(SettingsKit.card(this, pending.toList()))
            pending.clear()
        }
        items.forEach { item ->
            when (item) {
                is Item.Sec -> { flush(); out.addView(SettingsKit.section(this, item.text)) }
                is Item.Row -> pending.add(item.view())
            }
        }
        flush()
        return out
    }

    private fun settingsScreen(): View {
        val id = stack.lastOrNull() ?: "root"
        val col = column()
        col.addView(header(pageTitle(id), if (stack.isNotEmpty()) ({ stack.removeAt(stack.lastIndex); buildScreen() }) else null))
        val body = FrameLayout(this)
        val scroll = ScrollView(this).apply {
            isVerticalScrollBarEnabled = false
            clipToPadding = false
            setPadding(T.dp(16), 0, T.dp(16), navTotal() + T.dp(84))
        }
        fun fillBody() {
            scroll.removeAllViews()
            val q = settingsQuery.trim()
            if (id == "root" && q.isNotEmpty()) {
                val hits = listOf("look", "glass", "layout", "playback", "library", "backup")
                    .flatMap { pageItems(it) }.filterIsInstance<Item.Row>()
                    .filter { (it.title + " " + it.keywords).contains(q, ignoreCase = true) }
                scroll.addView(if (hits.isEmpty()) emptyState("تنظیماتی با این نام پیدا نشد") else renderItems(hits))
            } else {
                scroll.addView(renderItems(pageItems(id)))
            }
        }
        val keep = keepScroll
        keepScroll = 0
        if (id == "root") {
            col.addView(SettingsKit.searchBox(this, "جستجو در تنظیمات", settingsQuery) { settingsQuery = it; fillBody() },
                LinearLayout.LayoutParams(-1, -2).apply { setMargins(T.dp(16), T.dp(4), T.dp(16), T.dp(8)) })
        }
        fillBody()
        body.addView(scroll, fill())
        col.addView(body, LinearLayout.LayoutParams(-1, 0, 1f))
        settingsScroll = scroll
        if (keep > 0) scroll.post { scroll.scrollTo(0, keep) }
        return col
    }

    // ------------------------------------------------------------------ player

    private fun openPlayer() {
        if (playerLayer != null || client.currentId == null) return
        val layer = FrameLayout(this)
        layer.isClickable = true
        root.addView(layer, fill())
        playerLayer = layer
        buildPlayer()
        layer.translationY = root.height.toFloat()
        layer.animate().translationY(0f).setDuration(if (s.animations) 380L else 0L).start()
    }

    private fun closePlayer() {
        val layer = playerLayer ?: return
        dismissSheet()
        playerLayer = null
        pSeek = null; pCur = null; pDur = null; pPlay = null; pShuffle = null; pRepeat = null; pHeart = null; pCover = null; pBars = null
        layer.animate().translationY(root.height.toFloat()).setDuration(if (s.animations) 300L else 0L)
            .withEndAction { root.removeView(layer) }.start()
    }

    private fun buildPlayer() {
        val layer = playerLayer ?: return
        val track = client.currentId?.let { byId[it] }
        if (track == null) { closePlayer(); return }
        playerShownId = track.id
        layer.removeAllViews()
        val hue = hueOf(track.title)
        val top = if (T.dark) Color.HSVToColor(floatArrayOf(hue, 0.7f, 0.45f)) else Color.HSVToColor(floatArrayOf(hue, 0.35f, 0.96f))
        layer.background = GradientDrawable(GradientDrawable.Orientation.TOP_BOTTOM, intArrayOf(top, T.bg))
        val col = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(T.dp(22), insetTop + T.dp(10), T.dp(22), insetBottom + T.dp(18))
        }
        layer.addView(col, fill())

        col.addView(LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(btn(Icon.DOWN, 44, 24) { closePlayer() })
            addView(label(this@NavaqHomeActivity, "در حال پخش", 14f, true, T.fg2).apply { gravity = Gravity.CENTER }, LinearLayout.LayoutParams(0, -2, 1f))
            addView(btn(if (playerQueue) Icon.MUSIC else Icon.QUEUE, 44, 22) { playerQueue = !playerQueue; buildPlayer() })
        })

        val stage = FrameLayout(this)
        col.addView(stage, LinearLayout.LayoutParams(-1, 0, 1f))
        if (playerQueue) {
            val ids = client.queueIds
            val cells = ids.mapNotNull { byId[it] }.map { t ->
                Cell(t.id, t.title, t.artist ?: "", t.sourceValue, t.title, t) { client.jumpTo(t.id) }
            }
            stage.addView(listPane(cells, false, primary = false).also { it.setPadding(0, 0, 0, 0) }, fill())
        } else {
            val size = minOf((root.width * 0.76f).toInt(), (root.height * 0.38f).toInt()).coerceAtLeast(T.dp(180))
            val cover = CoverView(this).apply { bind(track.id, track.sourceValue, track.title) }
            pCover = cover
            stage.addView(cover, FrameLayout.LayoutParams(size, size, Gravity.CENTER))
            if (s.visualizer) {
                val bars = BarsView(this).apply { playing = client.isPlaying }
                pBars = bars
                stage.addView(bars, FrameLayout.LayoutParams(T.dp(36), T.dp(28), Gravity.BOTTOM or Gravity.CENTER_HORIZONTAL))
            }
            applyCoverMotion()
        }

        col.addView(LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(LinearLayout(this@NavaqHomeActivity).apply {
                orientation = LinearLayout.VERTICAL
                addView(label(this@NavaqHomeActivity, track.title, 22f, true, T.fg, 1))
                addView(label(this@NavaqHomeActivity, listOfNotNull(track.artist, track.album).joinToString(" · "), 14f, false, T.fg2, 1).apply { setPadding(0, T.dp(4), 0, 0) })
            }, LinearLayout.LayoutParams(0, -2, 1f))
            val fav = favs.isFavorite(track.id)
            val heart = btn(if (fav) Icon.HEART_FILL else Icon.HEART, 48, 26, if (fav) T.accent else T.fg) {
                val now = favs.toggle(track.id)
                pHeart?.let { it.icon = if (now) Icon.HEART_FILL else Icon.HEART; it.tint = if (now) T.accent else T.fg }
                currentAdapter?.notifyDataSetChanged()
            }
            pHeart = iconOf(heart)
            addView(heart)
        }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = T.dp(14) })

        val seek = SeekBar(this).apply {
            layoutDirection = View.LAYOUT_DIRECTION_LTR
            max = (client.durationMs / 1000L).toInt().coerceAtLeast(1)
            progress = (client.positionMs / 1000L).toInt()
            progressTintList = android.content.res.ColorStateList.valueOf(T.accent)
            thumbTintList = android.content.res.ColorStateList.valueOf(T.accent)
            progressBackgroundTintList = android.content.res.ColorStateList.valueOf(Color.argb(80, 128, 134, 160))
            setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                override fun onProgressChanged(seekBar: SeekBar?, progress: Int, fromUser: Boolean) {
                    if (fromUser) pCur?.text = fmtTime(progress * 1000L)
                }
                override fun onStartTrackingTouch(seekBar: SeekBar?) { seeking = true }
                override fun onStopTrackingTouch(seekBar: SeekBar?) {
                    seeking = false
                    client.seekTo((seekBar?.progress ?: 0) * 1000L)
                }
            })
        }
        pSeek = seek
        col.addView(seek, LinearLayout.LayoutParams(-1, -2).apply { topMargin = T.dp(10) })
        val cur = label(this, fmtTime(client.positionMs), 12f, false, T.fg2)
        val dur = label(this, fmtTime(client.durationMs), 12f, false, T.fg2)
        pCur = cur; pDur = dur
        col.addView(LinearLayout(this).apply {
            layoutDirection = View.LAYOUT_DIRECTION_LTR
            addView(cur, LinearLayout.LayoutParams(0, -2, 1f))
            addView(dur)
        }, LinearLayout.LayoutParams(-1, -2).apply { setMargins(T.dp(4), 0, T.dp(4), 0) })

        val shuffle = btn(Icon.SHUFFLE, 48, 24, if (client.shuffle) T.accent else T.fg) { client.toggleShuffle() }
        val repeat = btn(repeatIcon(), 48, 24, if (client.repeatMode != Player.REPEAT_MODE_OFF) T.accent else T.fg) { client.cycleRepeat() }
        pShuffle = iconOf(shuffle); pRepeat = iconOf(repeat)
        val play = FrameLayout(this).apply {
            background = GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(T.accent) }
            val ic = IconView(this@NavaqHomeActivity, if (client.isPlaying) Icon.PAUSE else Icon.PLAY, T.accentFg)
            pPlay = ic
            addView(ic, FrameLayout.LayoutParams(T.dp(32), T.dp(32), Gravity.CENTER))
            layoutParams = LinearLayout.LayoutParams(T.dp(72), T.dp(72))
            pressable { client.toggle() }
        }
        col.addView(LinearLayout(this).apply {
            layoutDirection = View.LAYOUT_DIRECTION_LTR
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(shuffle, LinearLayout.LayoutParams(0, T.dp(72), 1f).also { it.gravity = Gravity.CENTER })
            addView(btn(Icon.PREV, 56, 28) { client.previous() })
            addView(play, LinearLayout.LayoutParams(T.dp(72), T.dp(72)).apply { setMargins(T.dp(8), 0, T.dp(8), 0) })
            addView(btn(Icon.NEXT, 56, 28) { client.next() })
            addView(repeat, LinearLayout.LayoutParams(0, T.dp(72), 1f).also { it.gravity = Gravity.CENTER })
        }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = T.dp(10) })

        col.addView(LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
            glass(0.8f)
            setPadding(T.dp(8), T.dp(4), T.dp(8), T.dp(4))
            addView(btn(Icon.SPEED, 52, 22) { speedSheet() }, LinearLayout.LayoutParams(0, T.dp(52), 1f))
            addView(btn(Icon.TIMER, 52, 22, if (sleepRunnable != null) T.accent else T.fg) { sleepSheet() }, LinearLayout.LayoutParams(0, T.dp(52), 1f))
            addView(btn(Icon.QUEUE, 52, 22, if (playerQueue) T.accent else T.fg) { playerQueue = !playerQueue; buildPlayer() }, LinearLayout.LayoutParams(0, T.dp(52), 1f))
        }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = T.dp(6) })
    }

    private fun repeatIcon() = if (client.repeatMode == Player.REPEAT_MODE_ONE) Icon.REPEAT_ONE else Icon.REPEAT

    private fun applyCoverMotion() {
        val cover = pCover ?: return
        val spin = s.spinCircularCover && s.coverShape == CoverShape.CIRCLE && s.animations
        cover.animate().cancel()
        cover.animate().scaleX(if (client.isPlaying) 1f else 0.9f).scaleY(if (client.isPlaying) 1f else 0.9f)
            .setDuration(if (s.animations) 350L else 0L).start()
        if (spin && client.isPlaying) {
            val a = android.animation.ObjectAnimator.ofFloat(cover, View.ROTATION, cover.rotation, cover.rotation + 360f)
            a.duration = 14_000L
            a.interpolator = android.view.animation.LinearInterpolator()
            a.repeatCount = android.animation.ValueAnimator.INFINITE
            a.start()
            cover.tag = a
        } else {
            (cover.tag as? android.animation.ObjectAnimator)?.cancel()
        }
    }

    private fun onPlayback() {
        updateMini()
        val id = client.currentId
        if (id != null) byId[id]?.let { wall.setAlbumHue(hueOf(it.title)) }
        if (playerLayer != null) {
            if (id != playerShownId) buildPlayer() else {
                pPlay?.icon = if (client.isPlaying) Icon.PAUSE else Icon.PLAY
                pShuffle?.tint = if (client.shuffle) T.accent else T.fg
                pRepeat?.icon = repeatIcon()
                pRepeat?.tint = if (client.repeatMode != Player.REPEAT_MODE_OFF) T.accent else T.fg
                pBars?.playing = client.isPlaying
                applyCoverMotion()
            }
        }
        currentAdapter?.notifyDataSetChanged()
    }

    private fun updateProgress() {
        val dur = client.durationMs
        miniProgress?.progress = if (dur > 0) (client.positionMs * 1000L / dur).toInt() else 0
        if (playerLayer != null && !seeking) {
            pSeek?.let {
                it.max = (dur / 1000L).toInt().coerceAtLeast(1)
                it.progress = (client.positionMs / 1000L).toInt()
            }
            pCur?.text = fmtTime(client.positionMs)
            pDur?.text = fmtTime(dur)
        }
    }

    // ------------------------------------------------------------------ sheets

    private fun showSheet(title: String, options: List<Pair<String, () -> Unit>>) {
        dismissSheet()
        val layer = FrameLayout(this)
        layer.setBackgroundColor(Color.argb(110, 0, 0, 0))
        layer.isClickable = true
        layer.setOnClickListener { dismissSheet() }
        val panel = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            glass()
            setPadding(T.dp(18), T.dp(18), T.dp(18), T.dp(18))
            isClickable = true
            addView(label(this@NavaqHomeActivity, title, 17f, true))
            addView(FlowLayout(this@NavaqHomeActivity, T.dp(8)).apply {
                options.forEach { (text, run) ->
                    addView(label(this@NavaqHomeActivity, text, 14f, false).apply {
                        setPadding(T.dp(14), T.dp(9), T.dp(14), T.dp(9))
                        background = GradientDrawable().apply { cornerRadius = T.dpf(40f); setColor(Color.argb(70, 128, 134, 160)) }
                        pressable { run(); dismissSheet() }
                    })
                }
            }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = T.dp(14) })
        }
        layer.addView(panel, FrameLayout.LayoutParams(-1, -2, Gravity.BOTTOM).apply { setMargins(T.dp(10), 0, T.dp(10), insetBottom + T.dp(10)) })
        root.addView(layer, fill())
        sheetLayer = layer
    }

    private fun dismissSheet() {
        sheetLayer?.let { root.removeView(it) }
        sheetLayer = null
    }

    private fun speedSheet() = showSheet("سرعت پخش", listOf(0.5f, 0.75f, 1f, 1.25f, 1.5f, 2f).map { v ->
        "${fa(v.toString().removeSuffix(".0"))}×" to { client.setSpeed(v) }
    })

    private fun sleepSheet() {
        val opts = mutableListOf<Pair<String, () -> Unit>>()
        listOf(15, 30, 45, 60).forEach { m ->
            opts += "${fa(m)} دقیقه" to { setSleep(m) }
        }
        opts += "خاموش" to { setSleep(0) }
        showSheet("تایمر خواب", opts)
    }

    private fun setSleep(minutes: Int) {
        sleepRunnable?.let { handler.removeCallbacks(it) }
        sleepRunnable = null
        if (minutes > 0) {
            val r = Runnable { client.pause(); sleepRunnable = null; toast("تایمر خواب: پخش متوقف شد") }
            sleepRunnable = r
            handler.postDelayed(r, minutes * 60_000L)
            toast("تایمر خواب: ${fa(minutes)} دقیقه")
        } else {
            toast("تایمر خواب خاموش شد")
        }
        if (playerLayer != null) buildPlayer()
    }
}
