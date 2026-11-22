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
import com.navaq.android.core.NavaqNativeBridge
import org.json.JSONArray
import org.json.JSONObject
import android.app.DownloadManager
import com.navaq.android.net.Downloads
import com.navaq.android.net.LinkDownloader
import com.navaq.android.net.LanRoom
import java.io.File
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
        if (Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            // Needed for the media notification and lock-screen controls.
            requestPermissions(arrayOf(android.Manifest.permission.POST_NOTIFICATIONS), 12)
        }
        client = PlaybackClient(this) { onPlayback() }
        client.connect()
        loadTracks()
        rebuildAll()
        handler.post(tick)
        LanRoom.listener = { ev -> runOnUiThread { roomEvent(ev) } }
        LanRoom.commandListener = { cmd -> runOnUiThread { roomCommand(cmd) } }
        LanRoom.resolver = { id -> if (id.startsWith("room:")) null else byId[id]?.sourceValue?.let { android.net.Uri.parse(it) } }
        if (LanRoom.host != null) handler.post(roomTicker)
        if (LanRoom.guest != null) handler.post(roomGuestTicker)
    }

    override fun onResume() {
        super.onResume()
        if (allTracks.isEmpty() && AvaPermissions.hasAudioLibraryAccess(this)) loadTracks()
    }

    override fun onDestroy() {
        handler.removeCallbacksAndMessages(null)
        LanRoom.listener = null
        LanRoom.commandListener = null
        LanRoom.resolver = null
        roomDiscovery?.stop()
        if (isFinishing) {
            LanRoom.host?.stop(); LanRoom.host = null
            LanRoom.guest?.leave(); LanRoom.guest = null
        }
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
            allTracks = createdTracks()
            byId = allTracks.associateBy { it.id }
            return
        }
        permissionNeeded = false
        executor.execute {
            val list = try {
                AvaMusicCatalog(this).search(null, AvaSettings())
            } catch (_: Exception) {
                emptyList()
            }
            val extra = createdTracks() + Downloads.tracks(this)
            runOnUiThread {
                allTracks = list + extra
                byId = allTracks.associateBy { it.id }
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
            nav.setPadding(T.dp(7), T.dp(6), T.dp(7), T.dp(6))
            nav.layoutParams = FrameLayout.LayoutParams(-1, T.dp(68), Gravity.BOTTOM).apply {
                setMargins(T.dp(14), 0, T.dp(14), insetBottom + T.dp(14))
            }
        } else {
            nav.glass(1f, topOnly = true)
            nav.setPadding(T.dp(7), T.dp(6), T.dp(7), insetBottom)
            nav.layoutParams = FrameLayout.LayoutParams(-1, T.dp(78) + insetBottom, Gravity.BOTTOM)
        }
        val items = listOf(
            Triple("library", "کتابخانه", Icon.LIBRARY),
            Triple("search", "جستجو", Icon.SEARCH),
            Triple("settings", "تنظیمات", Icon.SLIDERS)
        )
        items.forEach { (id, text, icon) ->
            val on = screen == id
            val tint = if (on) T.neonA else T.fg2
            val tile = LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                gravity = Gravity.CENTER
                isFocusable = true
                contentDescription = text
                background = GradientDrawable(
                    GradientDrawable.Orientation.TL_BR,
                    if (on) intArrayOf(
                        (T.accent and 0x00FFFFFF) or (0x34 shl 24),
                        (T.neonB and 0x00FFFFFF) or (0x20 shl 24)
                    ) else intArrayOf(Color.TRANSPARENT, Color.TRANSPARENT)
                ).apply {
                    cornerRadius = T.radiusPx * 0.72f
                    if (on) setStroke(T.dp(1), (T.accent and 0x00FFFFFF) or (0x76 shl 24))
                }
                val glyph = IconView(this@NavaqHomeActivity, icon, tint)
                addView(glyph, LinearLayout.LayoutParams(T.dp(22), T.dp(22)))
                addView(label(this@NavaqHomeActivity, text, 11f, on, if (on) T.fg else T.fg2).apply {
                    setPadding(0, T.dp(3), 0, 0)
                })
                addView(View(this@NavaqHomeActivity).apply {
                    background = GradientDrawable(
                        GradientDrawable.Orientation.LEFT_RIGHT,
                        intArrayOf(T.neonA, T.neonB, T.neonC)
                    ).apply { cornerRadius = T.dp(3).toFloat() }
                    visibility = if (on) View.VISIBLE else View.INVISIBLE
                }, LinearLayout.LayoutParams(if (on) T.dp(26) else T.dp(10), T.dp(2)).apply {
                    topMargin = T.dp(4)
                })
                pressable {
                    if (screen != id) {
                        screen = id
                        detail = null
                        buildNav()
                        buildScreen()
                    }
                }
            }
            nav.addView(tile, LinearLayout.LayoutParams(0, -1, 1f).apply {
                setMargins(T.dp(3), 0, T.dp(3), 0)
            })
            if (s.animations) {
                tile.alpha = 0f
                tile.scaleX = 0.94f
                tile.scaleY = 0.94f
                tile.post {
                    tile.animate().alpha(1f).scaleX(1f).scaleY(1f)
                        .setDuration(if (on) 240L else 180L).start()
                }
            }
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
        val onRoom = screen == "settings" && stack.lastOrNull() == "room"
        if (onRoom && roomDiscovery == null && LanRoom.host == null && LanRoom.guest == null) {
            roomDiscovery = LanRoom.Discovery(applicationContext) { l ->
                runOnUiThread { roomFound = l; if (roomVisible() && LanRoom.guest == null) buildScreen() }
            }.also { it.start() }
        } else if (!onRoom && roomDiscovery != null) {
            roomDiscovery?.stop(); roomDiscovery = null; roomFound = emptyList()
        }
        content.removeAllViews()
        currentAdapter = null
        settingsScroll = null
        val v = when (screen) {
            "search" -> searchScreen()
            "settings" -> settingsScreen()
            else -> libraryScreen()
        }
        content.addView(v, fill())
        // Short, low-amplitude page transition: keeps the existing screen/state logic intact.
        if (s.animations) {
            v.alpha = 0f
            v.translationY = T.dp(10).toFloat()
            v.post {
                if (v.parent === content) {
                    v.animate().cancel()
                    v.animate().alpha(1f).translationY(0f)
                        .setDuration(220L)
                        .setInterpolator(android.view.animation.DecelerateInterpolator())
                        .start()
                }
            }
        }
        v.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_YES
        v.contentDescription = when (screen) {
            "search" -> "صفحه جستجوی موسیقی"
            "settings" -> "صفحه تنظیمات"
            else -> "کتابخانه موسیقی"
        }
    }

    /** Compact Bento overview: live library counts, with no changes to playback or storage logic. */
    private fun libraryHero(): View {
        val favoriteCount = allTracks.count { favs.isFavorite(it.id) }
        val playlistCount = playlists().size
        fun stat(value: String, caption: String, accent: Int): View =
            LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                gravity = Gravity.CENTER
                glass(0.72f)
                setPadding(T.dp(8), T.dp(9), T.dp(8), T.dp(9))
                addView(label(this@NavaqHomeActivity, value, 18f, true, accent))
                addView(label(this@NavaqHomeActivity, caption, 10.5f, false, T.fg2).apply {
                    setPadding(0, T.dp(3), 0, 0)
                })
            }
        return LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            glass(1.05f)
            setPadding(T.dp(16), T.dp(14), T.dp(16), T.dp(13))
            addView(label(this@NavaqHomeActivity, "NAVAQ  /  YOUR SOUNDSPACE", 10f, true, T.neonA))
            addView(label(this@NavaqHomeActivity, "موسیقی، با امضای تو", 20f, true, T.fg).apply {
                setPadding(0, T.dp(5), 0, T.dp(2))
            })
            addView(label(
                this@NavaqHomeActivity,
                if (client.currentId != null) "پخش ادامه دارد؛ به فضای شنیدنت برگرد." else "کتابخانه‌ات را کشف کن و حال‌وهوای شنیدنت را بساز.",
                11.5f, false, T.fg2, 2
            ))
            addView(LinearLayout(this@NavaqHomeActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER
                addView(stat(fa(allTracks.size), "آهنگ", T.neonA), LinearLayout.LayoutParams(0, -2, 1f).apply {
                    marginEnd = T.dp(6)
                })
                addView(stat(fa(favoriteCount), "پسندیده", T.neonB), LinearLayout.LayoutParams(0, -2, 1f).apply {
                    marginStart = T.dp(3); marginEnd = T.dp(3)
                })
                addView(stat(fa(playlistCount), "پلی‌لیست", T.neonC), LinearLayout.LayoutParams(0, -2, 1f).apply {
                    marginStart = T.dp(6)
                })
            }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = T.dp(12) })
        }.apply {
            layoutParams = LinearLayout.LayoutParams(-1, -2).apply {
                setMargins(T.dp(16), T.dp(3), T.dp(16), T.dp(8))
            }
        }
    }

    private fun libraryScreen(): View {
        val col = column()
        val d = detail
        if (d != null) {
            val actions = if (d.first == "playlist") arrayOf<View>(
                label(this, "حذف", 14f, true, T.accent).apply {
                    setPadding(T.dp(12), T.dp(8), T.dp(12), T.dp(8))
                    pressable { val cur = playlists(); cur.remove(d.second); savePlaylists(cur); detail = null; buildScreen() }
                }
            ) else emptyArray()
            col.addView(header(d.second, { detail = null; buildScreen() }, *actions))
            val list = when (d.first) {
                "playlist" -> playlists()[d.second].orEmpty().mapNotNull { byId[it] }
                "album" -> visibleTracks().filter { it.album == d.second }
                else -> visibleTracks().filter { it.artist == d.second }
            }
            col.addView(trackPane(list), LinearLayout.LayoutParams(-1, 0, 1f))
            return col
        }
        col.addView(header("موسیقی", null,
            btn(Icon.SEARCH, 44, 22) { screen = "search"; buildNav(); buildScreen() },
            btn(Icon.SLIDERS, 44, 22) { screen = "settings"; buildNav(); buildScreen() }))
        col.addView(libraryHero())
        val tabs = mutableListOf("tracks" to "آهنگ‌ها", "albums" to "آلبوم‌ها")
        if (s.showArtistsTab) tabs += "artists" to "هنرمندان"
        tabs += "fav" to "علاقه‌مندی‌ها"
        tabs += "playlists" to "پلی‌لیست‌ها"
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
                        background = GradientDrawable(
                            GradientDrawable.Orientation.TL_BR,
                            if (on) intArrayOf(
                                (T.accent and 0x00FFFFFF) or (0x48 shl 24),
                                (T.neonB and 0x00FFFFFF) or (0x28 shl 24)
                            ) else intArrayOf(
                                Color.argb(if (T.dark) 34 else 90, 128, 134, 160),
                                Color.argb(if (T.dark) 18 else 45, 128, 134, 160)
                            )
                        ).apply {
                            cornerRadius = T.dpf(40f)
                            if (on) setStroke(T.dp(1), (T.accent and 0x00FFFFFF) or (0x90 shl 24))
                        }
                        isFocusable = true
                        contentDescription = if (on) "$text، انتخاب‌شده" else text
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
            tab == "playlists" -> {
                val all = playlists()
                val cells = mutableListOf(
                    Cell("pl:new", "＋ پلی‌لیست جدید", "ساخت پلی‌لیست خالی", null, "new", null) {
                        val cur = playlists(); cur[newPlaylistName(cur)] = mutableListOf(); savePlaylists(cur); buildScreen()
                    }
                )
                all.forEach { (name, ids) ->
                    cells += Cell("pl:$name", name, "${fa(ids.size)} آهنگ", ids.firstOrNull()?.let { byId[it]?.sourceValue }, name, null) {
                        detail = "playlist" to name; buildScreen()
                    }
                }
                listPane(cells, grid = false)
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
        "sound" -> "صدا و اکولایزر"
        "create" -> "ساخت آهنگ"
        "dna" -> "Music DNA"
        "room" -> "گوش دادن مشترک"
        "downloads" -> "دانلودها"
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

    private val eqFreqs = intArrayOf(31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000)
    private val eqPresets = mapOf(
        "flat" to floatArrayOf(0f, 0f, 0f, 0f, 0f, 0f, 0f, 0f, 0f, 0f),
        "bass" to floatArrayOf(6f, 5f, 4f, 2f, 0f, 0f, 0f, 0f, 0f, 0f),
        "treble" to floatArrayOf(0f, 0f, 0f, 0f, 0f, 1f, 2f, 4f, 5f, 6f),
        "vocal" to floatArrayOf(-2f, -2f, -1f, 1f, 3f, 3f, 2f, 1f, 0f, -1f),
        "rock" to floatArrayOf(4f, 3f, 2f, -1f, -2f, -1f, 2f, 3f, 4f, 4f),
        "classical" to floatArrayOf(0f, 0f, 0f, 0f, 0f, 0f, -2f, -2f, -2f, -3f)
    )

    private fun eqGains(): FloatArray {
        val parts = runtimePrefs().getString("eq_gains", null)?.split(",")
        return if (parts != null && parts.size == 10) FloatArray(10) { parts[it].toFloatOrNull() ?: 0f } else FloatArray(10)
    }

    private fun saveEq(gains: FloatArray, enabled: Boolean) {
        runtimePrefs().edit().putString("eq_gains", gains.joinToString(",")).putBoolean("eq_on", enabled).apply()
        // Same shape as the Rust `EqualizerSettings`, which validates it before applying.
        val bands = org.json.JSONArray()
        eqFreqs.forEachIndexed { i, f ->
            bands.put(org.json.JSONObject().put("frequency_hz", f).put("gain_db", gains[i].toDouble().coerceIn(-12.0, 12.0)))
        }
        client.setEqualizerJson(org.json.JSONObject().put("enabled", enabled).put("bands", bands).toString())
    }

    // ------------------------------------------------------------------ shared room (LAN)

    private var roomFound: List<LanRoom.Found> = emptyList()
    private var roomDiscovery: LanRoom.Discovery? = null
    private var roomHostIn = ""
    private var roomCodeIn = ""
    private var roomStatus = ""
    private var roomNow: TextView? = null
    private val roomTicker = object : Runnable {
        override fun run() {
            roomPush()
            if (LanRoom.host != null) handler.postDelayed(this, 1000L)
        }
    }

    private fun roomVisible() = screen == "settings" && stack.lastOrNull() == "room"

    private fun fieldRow(hint: String, value: String, ltr: Boolean, onChange: (String) -> Unit): View =
        LinearLayout(this).apply {
            setPadding(0, T.dp(8), 0, T.dp(8))
            addView(android.widget.EditText(this@NavaqHomeActivity).apply {
                setText(value)
                this.hint = hint
                setHintTextColor(T.fg2); setTextColor(T.fg)
                textSize = 14f * T.fontScale
                if (ltr) textDirection = View.TEXT_DIRECTION_LTR
                isSingleLine = true
                background = GradientDrawable().apply { cornerRadius = T.dpf(14f); setColor(Color.argb(60, 128, 134, 160)) }
                setPadding(T.dp(12), T.dp(10), T.dp(12), T.dp(10))
                addTextChangedListener(object : android.text.TextWatcher {
                    override fun beforeTextChanged(t: CharSequence?, a: Int, b: Int, c: Int) = Unit
                    override fun onTextChanged(t: CharSequence?, a: Int, b: Int, c: Int) = Unit
                    override fun afterTextChanged(e: android.text.Editable?) { onChange(e?.toString().orEmpty()) }
                })
            }, LinearLayout.LayoutParams(-1, -2))
        }

    private fun roomPush() {
        val h = LanRoom.host ?: return
        val id = client.currentId
        val t = id?.let { byId[it] }
        val o = JSONObject()
        if (id != null && t != null && !id.startsWith("room:")) {
            o.put("id", id).put("title", t.title).put("artist", t.artist ?: "")
                .put("dur", client.durationMs).put("pos", client.positionMs)
                .put("playing", client.isPlaying).put("path", "/a/" + android.net.Uri.encode(id))
                .put("ts", android.os.SystemClock.elapsedRealtime())
            val nid = client.nextId
            val nt = nid?.let { byId[it] }
            if (nid != null && nt != null && !nid.startsWith("room:")) {
                o.put("next", JSONObject().put("id", nid).put("title", nt.title).put("artist", nt.artist ?: "")
                    .put("dur", nt.durationMs ?: 0L).put("path", "/a/" + android.net.Uri.encode(nid)))
            }
        }
        h.update(o)
    }

    private fun roomStartHost() {
        if (LanRoom.host != null) return
        LanRoom.guest?.leave(); LanRoom.guest = null
        roomDiscovery?.stop(); roomDiscovery = null
        executor.execute {
            try {
                val h = LanRoom.Host(
                    applicationContext, LanRoom.newCode(), LanRoom.newToken(),
                    { id -> LanRoom.resolver?.invoke(id) }, { cmd -> LanRoom.commandListener?.invoke(cmd) }
                )
                h.allowGuestControl = runtimePrefs().getBoolean("room_guest_control", true)
                h.start(Build.MODEL ?: "NAVAQ")
                LanRoom.host = h
                com.navaq.android.net.NavaqTransferService.ensure(applicationContext)
                runOnUiThread { handler.post(roomTicker); buildScreen() }
            } catch (_: Exception) {
                runOnUiThread { toast("ساخت اتاق ناموفق بود") }
            }
        }
    }

    private fun roomStop() {
        if (LanRoom.guest != null) client.setSpeed(1f)
        LanRoom.host?.stop(); LanRoom.host = null
        LanRoom.guest?.leave(); LanRoom.guest = null
        roomStatus = ""
        buildScreen()
    }

    private fun roomJoin() {
        val addr = roomHostIn.trim()
        val host = addr.substringBeforeLast(':', "")
        val port = addr.substringAfterLast(':', "").toIntOrNull()
        if (host.isEmpty() || port == null || port !in 1..65535) { toast("آدرس را به شکل 192.168.1.5:4000 وارد کنید"); return }
        if (roomCodeIn.trim().length != 6) { toast("کد ۶ رقمی را وارد کنید"); return }
        LanRoom.guest?.leave()
        roomStatus = "در حال اتصال…"
        LanRoom.guest = LanRoom.Guest(host, port, roomCodeIn.trim()) { ev -> LanRoom.listener?.invoke(ev) }.also { it.start() }
        com.navaq.android.net.NavaqTransferService.ensure(applicationContext)
        handler.removeCallbacks(roomGuestTicker)
        handler.post(roomGuestTicker)
        buildScreen()
    }

    private fun roomEvent(ev: String) {
        when (ev) {
            "state" -> { roomSync(); roomNow?.text = roomNowText(); return }
            "connected" -> roomStatus = "متصل شدید؛ هم‌زمان با میزبان گوش می‌دهید"
            "bad_code" -> { roomStatus = "کد اشتباه است"; LanRoom.guest = null }
            "failed" -> { roomStatus = "اتصال برقرار نشد؛ آدرس، کد و وای‌فای را بررسی کنید"; LanRoom.guest = null }
            "closed" -> { roomStatus = "اتاق بسته شد"; LanRoom.guest = null }
        }
        if (roomVisible()) buildScreen()
    }

    private fun roomNowText(): String {
        val st = LanRoom.guest?.lastState
        val title = st?.optString("title").orEmpty()
        return if (title.isEmpty()) "میزبان هنوز آهنگی پخش نکرده است" else "در حال پخش: $title" +
            st?.optString("artist").orEmpty().let { if (it.isEmpty()) "" else " — $it" } +
            "\nاختلاف با میزبان: ${fa(kotlin.math.abs(roomErrMs))} میلی‌ثانیه"
    }

    private var roomSeekAt = 0L
    private var roomErrMs = 0L
    private val roomGuestTicker = object : Runnable {
        override fun run() {
            if (LanRoom.guest != null) {
                roomSync()
                handler.postDelayed(this, 500L)
            }
        }
    }

    private fun roomTrack(g: LanRoom.Guest, o: JSONObject): AvaCatalogTrack {
        val t = AvaCatalogTrack(
            id = "room:" + o.optString("id"), title = o.optString("title"), artist = o.optString("artist").ifBlank { null },
            album = "اتاق مشترک", durationMs = o.optLong("dur"), sourceKind = "remote_url",
            sourceValue = g.urlFor(o.optString("path"))
        )
        byId = byId + (t.id to t)
        return t
    }

    /**
     * Keeps this phone locked to the host: the host's position is projected to "now" using the
     * ping-estimated clock offset; small errors are absorbed by nudging playback speed (+-6%),
     * larger ones by a seek, so every phone plays the same moment of the same track.
     */
    private fun roomSync() {
        val g = LanRoom.guest ?: return
        val st = g.lastState ?: return
        val id = st.optString("id")
        if (id.isEmpty() || !g.connected) return
        val playing = st.optBoolean("playing")
        val clock = android.os.SystemClock.elapsedRealtime()
        val hostNow = clock + g.offsetMs
        val manual = runtimePrefs().getInt("room_delay_ms", 0)
        val target = st.optLong("pos") + (if (playing) hostNow - st.optLong("ts") else 0L) + manual
        val rid = "room:$id"

        if (client.currentId != rid) {
            if (client.shuffle) client.toggleShuffle()
            if (rid in client.queueIds) client.jumpTo(rid) else client.playQueue(listOf(roomTrack(g, st)), 0)
            client.setSpeed(1f)
            client.seekTo(target + if (playing) 200L else 0L)
            if (!playing) client.pause()
            roomSeekAt = clock
            return
        }
        st.optJSONObject("next")?.let { n ->
            val nid = "room:" + n.optString("id")
            if (n.optString("id").isNotEmpty() && nid !in client.queueIds) client.addTrack(roomTrack(g, n))
        }
        if (playing != client.isPlaying) client.toggle()
        if (!playing) {
            if (kotlin.math.abs(client.positionMs - target) > 300L) client.seekTo(target)
            return
        }
        if (clock - roomSeekAt < 1500L) return
        val err = client.positionMs - target
        roomErrMs = err
        when {
            kotlin.math.abs(err) > 400L -> { client.setSpeed(1f); client.seekTo(target + 200L); roomSeekAt = clock }
            kotlin.math.abs(err) > 25L -> client.setSpeed((1f - err / 1500f).coerceIn(0.94f, 1.06f))
            else -> if (client.speed != 1f) client.setSpeed(1f)
        }
        roomNow?.text = roomNowText()
    }

    private fun roomCommand(cmd: JSONObject) {
        when (cmd.optString("k")) {
            "toggle" -> client.toggle()
            "play" -> if (!client.isPlaying) client.toggle()
            "pause" -> if (client.isPlaying) client.toggle()
            "next" -> client.next()
            "prev" -> client.previous()
            "seek" -> client.seekTo(cmd.optLong("pos"))
        }
        handler.postDelayed({ roomPush() }, 250L)
    }

    private fun roomItems(): List<Item> {
        val h = LanRoom.host
        val g = LanRoom.guest
        val out = mutableListOf<Item>()
        out += Item.Sec("میزبان — پخش برای دیگران")
        if (h != null) {
            val ip = LanRoom.localIp()
            out += Item.Row("اطلاعات اتاق", "code address") {
                LinearLayout(this).apply {
                    orientation = LinearLayout.VERTICAL
                    setPadding(0, T.dp(12), 0, T.dp(12))
                    addView(label(this@NavaqHomeActivity, h.code, 34f, true, T.accent).apply { textDirection = View.TEXT_DIRECTION_LTR; letterSpacing = 0.2f })
                    addView(label(this@NavaqHomeActivity, (ip ?: "؟") + ":" + h.port, 15f, false, T.fg).apply { textDirection = View.TEXT_DIRECTION_LTR; setPadding(0, T.dp(6), 0, T.dp(6)) })
                    addView(label(this@NavaqHomeActivity, "دوستانتان باید به همین وای‌فای وصل باشند، اتاق شما را در فهرست «پیوستن» ببینند و کد را وارد کنند.", 12.5f, false, T.fg2))
                }
            }
            out += Item.Row("کپی", "copy") { SettingsKit.actionRow(this, Icon.CHECK, "کپی آدرس و کد", null) { copyText("${ip ?: ""}:${h.port}  کد: ${h.code}") } }
            out += Item.Row("کنترل مهمان‌ها", "guest control") {
                SettingsKit.toggleRow(this, "کنترل مهمان‌ها", "پخش، توقف، بعدی و قبلی برای همه", runtimePrefs().getBoolean("room_guest_control", true)) { v ->
                    runtimePrefs().edit().putBoolean("room_guest_control", v).apply(); LanRoom.host?.allowGuestControl = v
                }
            }
            out += Item.Row("بستن", "stop") { SettingsKit.actionRow(this, Icon.RESET, "بستن اتاق", "مهمان‌ها قطع می‌شوند") { roomStop() } }
        } else if (g == null) {
            out += Item.Row("ساخت اتاق", "host start") { SettingsKit.actionRow(this, Icon.MUSIC, "ساخت اتاق", "آهنگ در حال پخش شما برای مهمان‌ها همگام می‌شود") { roomStartHost() } }
        } else {
            out += Item.Row("توضیح", "") { SettingsKit.noteRow(this, "برای ساخت اتاق ابتدا از اتاق فعلی خارج شوید.") }
        }

        out += Item.Sec("پیوستن به اتاق")
        if (h != null) {
            out += Item.Row("توضیح", "") { SettingsKit.noteRow(this, "هم‌زمان نمی‌توانید میزبان و مهمان باشید.") }
        } else if (g != null) {
            out += Item.Row("وضعیت", "status") { SettingsKit.noteRow(this, roomStatus) }
            out += Item.Row("در حال پخش", "now") {
                label(this, roomNowText(), 14f, true, T.fg).apply { setPadding(0, T.dp(10), 0, T.dp(10)); roomNow = this }
            }
            out += Item.Row("کنترل", "controls") {
                LinearLayout(this).apply {
                    gravity = Gravity.CENTER
                    addView(btn(Icon.PREV, 52, 22) { g.send(JSONObject().put("k", "prev")) }, LinearLayout.LayoutParams(T.dp(64), T.dp(52)))
                    addView(btn(Icon.PLAY, 52, 22) { g.send(JSONObject().put("k", "toggle")) }, LinearLayout.LayoutParams(T.dp(64), T.dp(52)))
                    addView(btn(Icon.NEXT, 52, 22) { g.send(JSONObject().put("k", "next")) }, LinearLayout.LayoutParams(T.dp(64), T.dp(52)))
                }
            }
            out += prefSlider("جبران تاخیر صدا", "room_delay_ms", -500, 500, 0, { fa(it) + " ms" })
            out += Item.Row("راهنما", "") { SettingsKit.noteRow(this, "اگر با بلندگوی بلوتوثی گوش می‌دهید و صدا دیرتر از دیگران می‌رسد، این مقدار را کم یا زیاد کنید.") }
            out += Item.Row("خروج", "leave") { SettingsKit.actionRow(this, Icon.RESET, "خروج از اتاق", null) { roomStop() } }
        } else {
            if (roomFound.isEmpty()) {
                out += Item.Row("جستجو", "scan") { SettingsKit.noteRow(this, "در حال جستجوی اتاق‌های روی این شبکه… اگر اتاقی پیدا نشد، آدرس را دستی وارد کنید.") }
            }
            roomFound.forEach { f ->
                out += Item.Row(f.name, "room") {
                    SettingsKit.actionRow(this, Icon.MUSIC, f.name, "${f.host}:${f.port}") { roomHostIn = "${f.host}:${f.port}"; buildScreen() }
                }
            }
            out += Item.Row("آدرس", "address") { fieldRow("آدرس میزبان، مثل 192.168.1.5:4000", roomHostIn, true) { roomHostIn = it } }
            out += Item.Row("کد", "code") { fieldRow("کد ۶ رقمی", roomCodeIn, true) { roomCodeIn = it } }
            if (roomStatus.isNotEmpty()) out += Item.Row("وضعیت", "status") { SettingsKit.noteRow(this, roomStatus) }
            out += Item.Row("اتصال", "join") { SettingsKit.actionRow(this, Icon.CHECK, "اتصال", "آهنگ و جایگاه پخش با میزبان همگام می‌شود") { roomJoin() } }
        }
        return out
    }

    // ------------------------------------------------------------------ downloads (NAVAQ Downloader)

    private var dlUrl = ""
    private var dlName = ""
    private var dlBox: LinearLayout? = null
    private var dlDoneCount = -1
    private val dlTicker = object : Runnable {
        override fun run() {
            if (screen == "settings" && stack.lastOrNull() == "downloads") {
                fillDownloads()
                handler.postDelayed(this, 1000L)
            }
        }
    }

    private fun dlAdd() {
        val url = dlUrl.trim()
        if (url.isEmpty()) { toast("لینک را وارد کنید"); return }
        toast("در حال بررسی لینک…")
        executor.execute {
            val probe = LinkDownloader.probe(url)
            runOnUiThread {
                when {
                    probe.error != null -> toast(probe.error)
                    probe.cands.size == 1 -> dlStart(probe.cands[0], probe.title)
                    else -> showSheet("چند فایل پیدا شد؛ کدام را دانلود کنم؟", probe.cands.map { c -> c.label to { dlStart(c, probe.title) } })
                }
            }
        }
    }

    private fun dlStart(cand: LinkDownloader.Cand, pageTitle: String?) {
        val title = dlName.ifBlank { pageTitle ?: cand.label.substringAfter("— ").substringBeforeLast('.') }
        LinkDownloader.add(
            applicationContext, cand, title,
            runtimePrefs().getBoolean("dl_audio_only", false), runtimePrefs().getBoolean("dl_wifi_only", false)
        )
        dlUrl = ""; dlName = ""
        toast("به صف دانلود اضافه شد")
        buildScreen()
    }

    private fun dlContainer(): View {
        val box = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        dlBox = box
        fillDownloads()
        handler.removeCallbacks(dlTicker)
        handler.postDelayed(dlTicker, 1000L)
        return box
    }

    private fun fillDownloads() {
        val box = dlBox ?: return
        val jobs = LinkDownloader.list(applicationContext)
        box.removeAllViews()
        val active = jobs.count { it.state == LinkDownloader.RUNNING || it.state == LinkDownloader.QUEUED }
        val paused = jobs.count { it.state == LinkDownloader.PAUSED }
        val failed = jobs.count { it.state == LinkDownloader.FAILED }
        val completed = jobs.count { it.state == LinkDownloader.DONE }
        box.addView(label(this, "کل: ${fa(jobs.size)}  •  فعال: ${fa(active)}  •  متوقف: ${fa(paused)}  •  ناموفق: ${fa(failed)}  •  کامل: ${fa(completed)}", 12f, false, T.fg2)
            .apply { setPadding(0, T.dp(4), 0, T.dp(10)) })
        if (jobs.isEmpty()) {
            box.addView(label(this, "هنوز دانلودی ندارید.", 13f, false, T.fg2).apply { setPadding(0, T.dp(12), 0, T.dp(12)) })
        }
        jobs.forEach { box.addView(dlRow(it)) }
        val done = jobs.count { it.state == LinkDownloader.DONE }
        if (dlDoneCount >= 0 && done > dlDoneCount) loadTracks()
        dlDoneCount = done
    }

    private fun dlStatus(j: LinkDownloader.Job): String {
        val base = when (j.state) {
            LinkDownloader.DONE -> "آماده؛ آفلاین در کتابخانه"
            LinkDownloader.FAILED -> "ناموفق: " + j.error.orEmpty()
            LinkDownloader.PAUSED -> "متوقف" + (j.error?.let { " — " + it } ?: "") + if (j.total > 0) " (" + fa(j.percent) + "٪)" else ""
            LinkDownloader.QUEUED -> "در صف"
            else -> if (j.hls) "دریافت بخش‌ها: " + fa(j.done) + " از " + fa(j.total)
            else if (j.total > 0) fa(j.percent) + "٪ — " + fa(j.done / 1024 / 1024) + " از " + fa(j.total / 1024 / 1024) + " مگابایت"
            else "در حال دانلود… " + fa(j.done / 1024 / 1024) + " مگابایت"
        }
        val speed = if (j.state == LinkDownloader.RUNNING && j.speedBps > 0L) {
            val rate = if (j.speedBps >= 1_048_576L) String.format("%.1f MB/s", j.speedBps / 1_048_576.0)
                else fa(j.speedBps / 1024L) + " KB/s"
            " • " + rate
        } else ""
        val eta = if (j.state == LinkDownloader.RUNNING && j.etaSeconds >= 0L) {
            " • زمان باقی‌مانده: " + fa(j.etaSeconds / 60) + ":" + fa(j.etaSeconds % 60)
        } else ""
        return base + speed + eta
    }

    private fun dlRow(j: LinkDownloader.Job): View = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        glass(0.88f)
        setPadding(T.dp(13), T.dp(12), T.dp(13), T.dp(12))
        layoutParams = LinearLayout.LayoutParams(-1, -2).apply {
            setMargins(0, T.dp(5), 0, T.dp(5))
        }
        val stateColor = when (j.state) {
            LinkDownloader.DONE -> T.neonA
            LinkDownloader.FAILED -> 0xFFFF7373.toInt()
            LinkDownloader.PAUSED -> T.neonC
            else -> T.accent
        }
        addView(LinearLayout(this@NavaqHomeActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(label(this@NavaqHomeActivity, j.title.ifBlank { "دانلود" }, 14.5f, true, T.fg, 1),
                LinearLayout.LayoutParams(0, -2, 1f))
            addView(label(this@NavaqHomeActivity, when (j.state) {
                LinkDownloader.DONE -> "کامل"
                LinkDownloader.FAILED -> "خطا"
                LinkDownloader.PAUSED -> "مکث"
                LinkDownloader.QUEUED -> "صف"
                else -> "دریافت"
            }, 10.5f, true, stateColor).apply {
                setPadding(T.dp(8), T.dp(5), T.dp(8), T.dp(5))
                background = GradientDrawable().apply {
                    cornerRadius = T.dpf(30f)
                    setColor((stateColor and 0x00FFFFFF) or (0x25 shl 24))
                    setStroke(T.dp(1), (stateColor and 0x00FFFFFF) or (0x70 shl 24))
                }
            })
        })
        addView(label(this@NavaqHomeActivity, dlStatus(j), 11.5f, false, T.fg2)
            .apply { setPadding(0, T.dp(7), 0, T.dp(9)) })
        if (j.state == LinkDownloader.RUNNING || j.state == LinkDownloader.PAUSED || j.state == LinkDownloader.QUEUED) {
            val p = j.percent.toFloat().coerceIn(0f, 100f)
            addView(LinearLayout(this@NavaqHomeActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                background = GradientDrawable().apply {
                    cornerRadius = T.dpf(4f)
                    setColor(Color.argb(55, 128, 134, 160))
                }
                addView(View(this@NavaqHomeActivity).apply {
                    background = GradientDrawable(
                        GradientDrawable.Orientation.LEFT_RIGHT,
                        intArrayOf(T.accent, T.neonB, T.neonA)
                    ).apply { cornerRadius = T.dpf(4f) }
                }, LinearLayout.LayoutParams(0, T.dp(5), p.coerceAtLeast(0.1f)))
                addView(View(this@NavaqHomeActivity), LinearLayout.LayoutParams(0, T.dp(5), (100f - p).coerceAtLeast(0.1f)))
            }, LinearLayout.LayoutParams(-1, -2))
        }
        contentDescription = "${j.title}، ${dlStatus(j)}. برای مدیریت لمس کنید."
        isFocusable = true
        pressable { dlActions(j) }
    }

    private fun dlActions(j: LinkDownloader.Job) {
        val ctx = applicationContext
        val opts = mutableListOf<Pair<String, () -> Unit>>()
        when (j.state) {
            LinkDownloader.DONE -> {
                opts += "پخش" to {
                    val name = File(j.path).name
                    executor.execute {
                        val t = byId["dl:$name"] ?: Downloads.tracks(ctx).firstOrNull { it.id == "dl:$name" }
                        runOnUiThread { if (t != null) playFrom(listOf(t), 0) else toast("فایل پیدا نشد") }
                    }
                }
                opts += "حذف فایل" to { LinkDownloader.remove(ctx, j); loadTracks(); fillDownloads() }
            }
            LinkDownloader.FAILED, LinkDownloader.PAUSED -> {
                opts += (if (j.state == LinkDownloader.FAILED) "تلاش دوباره" else "ادامه") to { LinkDownloader.start(ctx, j); fillDownloads() }
                opts += "حذف" to { LinkDownloader.remove(ctx, j); fillDownloads() }
            }
            else -> {
                opts += "مکث" to { LinkDownloader.pause(ctx, j); fillDownloads() }
                opts += "لغو و حذف" to { LinkDownloader.remove(ctx, j); fillDownloads() }
            }
        }
        showSheet(j.title.ifBlank { "دانلود" }, opts)
    }

    // ------------------------------------------------------------------ playlists

    private fun plPrefs() = getSharedPreferences("navaq_playlists", MODE_PRIVATE)

    private fun playlists(): LinkedHashMap<String, MutableList<String>> {
        val out = LinkedHashMap<String, MutableList<String>>()
        try {
            val o = JSONObject(plPrefs().getString("all", "{}") ?: "{}")
            o.keys().forEach { name ->
                val a = o.getJSONArray(name)
                out[name] = MutableList(a.length()) { a.getString(it) }
            }
        } catch (_: Exception) {
        }
        return out
    }

    private fun savePlaylists(all: Map<String, List<String>>) {
        val o = JSONObject()
        all.forEach { (k, v) -> o.put(k, JSONArray(v)) }
        plPrefs().edit().putString("all", o.toString()).apply()
    }

    private fun newPlaylistName(all: Map<String, *>): String {
        var n = all.size + 1
        while ("پلی‌لیست ${fa(n)}" in all) n++
        return "پلی‌لیست ${fa(n)}"
    }

    private fun addToPlaylistSheet(trackId: String) {
        val all = playlists()
        val opts = mutableListOf<Pair<String, () -> Unit>>()
        all.keys.forEach { name ->
            opts += name to {
                val cur = playlists()
                val list = cur.getOrPut(name) { mutableListOf() }
                if (trackId !in list) list.add(trackId)
                savePlaylists(cur)
                toast("به «$name» افزوده شد")
            }
        }
        opts += "＋ پلی‌لیست جدید" to {
            val cur = playlists()
            val name = newPlaylistName(cur)
            cur[name] = mutableListOf(trackId)
            savePlaylists(cur)
            toast("«$name» ساخته شد")
        }
        showSheet("افزودن به پلی‌لیست", opts)
    }

    // ------------------------------------------------------------------ Music DNA (code exchange, no server)

    private fun dnaProfile(): JSONObject {
        val artists = java.util.TreeMap<String, Double>()
        allTracks.forEach { t ->
            val a = t.artist
            if (!a.isNullOrBlank() && !t.id.startsWith("created:")) {
                artists[a] = (artists[a] ?: 0.0) + 1.0 + if (favs.isFavorite(t.id)) 3.0 else 0.0
            }
        }
        val o = JSONObject()
        val ar = JSONObject()
        artists.forEach { (k, v) -> ar.put(k, v) }
        return o.put("artists", ar).put("genres", JSONObject()).put("avg_tempo", JSONObject.NULL)
            .put("energy", JSONObject.NULL).put("sample_size", allTracks.size)
    }

    private fun dnaCode(): String = android.util.Base64.encodeToString(
        dnaProfile().toString().toByteArray(Charsets.UTF_8),
        android.util.Base64.NO_WRAP or android.util.Base64.URL_SAFE
    )

    private var dnaResult = ""
    private var dnaShared: List<String> = emptyList()
    private var dnaInput = ""

    private fun dnaCompare() {
        val raw = dnaInput.trim()
        if (raw.isEmpty()) { toast("کد دوست را وارد کنید"); return }
        val theirs = try {
            String(android.util.Base64.decode(raw, android.util.Base64.URL_SAFE), Charsets.UTF_8)
        } catch (_: IllegalArgumentException) {
            toast("کد معتبر نیست"); return
        }
        val mine = dnaProfile().toString()
        executor.execute {
            val out = try { NavaqNativeBridge.nativeDnaCompare(mine, theirs) } catch (e: Throwable) { null }
            runOnUiThread {
                val json = try { JSONObject(out ?: "{}") } catch (_: Exception) { JSONObject() }
                if (!json.has("score")) { toast("مقایسه ناموفق بود"); return@runOnUiThread }
                val shared = json.optJSONArray("shared_artists")
                dnaShared = (0 until (shared?.length() ?: 0)).map { shared!!.getString(it) }
                val pct = (json.getDouble("score") * 100).toInt()
                dnaResult = "شباهت سلیقه: ${fa(pct)}٪" +
                    if (dnaShared.isEmpty()) "" else "\nهنرمندان مشترک: " + dnaShared.take(8).joinToString("، ")
                if (screen == "settings" && stack.lastOrNull() == "dna") buildScreen()
            }
        }
    }

    private fun sharedPlaylist() {
        val wanted = dnaShared.toSet()
        val tracks = allTracks.filter { it.artist in wanted }
            .groupBy { it.artist }.values.flatMap { it.take(3) }.shuffled()
        if (tracks.isEmpty()) { toast("آهنگ مشترکی در کتابخانه‌ی شما پیدا نشد"); return }
        val cur = playlists()
        val name = "مشترک " + fa(cur.size + 1)
        cur[name] = tracks.map { it.id }.toMutableList()
        savePlaylists(cur)
        playFrom(tracks, 0)
        toast("پلی‌لیست مشترک ساخته شد")
    }

    private fun copyText(text: String) {
        val cm = getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
        cm.setPrimaryClip(android.content.ClipData.newPlainText("NAVAQ", text))
        toast("کپی شد")
    }

    // ------------------------------------------------------------------ created songs

    private fun createdPrefs() = getSharedPreferences("navaq_created", MODE_PRIVATE)

    private fun createdTracks(): List<AvaCatalogTrack> {
        val raw = createdPrefs().getString("songs", null) ?: return emptyList()
        return try {
            val arr = JSONArray(raw)
            (0 until arr.length()).map { arr.getJSONObject(it) }.filter { File(it.getString("path")).exists() }.map {
                AvaCatalogTrack(
                    id = it.getString("id"), title = it.getString("title"), artist = "NAVAQ Composer",
                    album = "ساخته‌شده", durationMs = it.optLong("dur"), sourceKind = "local_file",
                    sourceValue = "file://" + it.getString("path")
                )
            }
        } catch (_: Exception) {
            emptyList()
        }
    }

    private fun rememberCreated(id: String, title: String, path: String, durMs: Long) {
        val arr = try { JSONArray(createdPrefs().getString("songs", "[]")) } catch (_: Exception) { JSONArray() }
        arr.put(JSONObject().put("id", id).put("title", title).put("path", path).put("dur", durMs))
        createdPrefs().edit().putString("songs", arr.toString()).apply()
    }

    private var createLyrics = ""

    private fun rpStr(key: String, def: String) = runtimePrefs().getString(key, def) ?: def
    private fun rpInt(key: String, def: Int) = runtimePrefs().getInt(key, def)

    private fun generateSong() {
        val p = runtimePrefs()
        val instrumental = p.getBoolean("cr_instr", false)
        val lyrics = if (instrumental) (1..rpInt("cr_lines", 6)).joinToString("\n") { "la la la la" } else createLyrics.trim()
        if (lyrics.isBlank()) { toast("متن آهنگ را بنویسید"); return }
        val request = JSONObject()
            .put("lyrics", lyrics)
            .put("style", rpStr("cr_style", "pop"))
            .put("tempo_bpm", rpInt("cr_tempo", 100))
            .put("key", rpStr("cr_key", "C major"))
            .put("duration_secs", JSONObject.NULL)
            .put("also_instrumental", false)
        val stamp = System.currentTimeMillis()
        val dir = File(filesDir, "created").apply { mkdirs() }
        val base = File(dir, "song_$stamp").path
        toast("در حال ساخت آهنگ…")
        executor.execute {
            val out = try { NavaqNativeBridge.nativeGenerateSong(request.toString(), base) } catch (e: Throwable) { null }
            runOnUiThread {
                val json = try { JSONObject(out ?: "{}") } catch (_: Exception) { JSONObject() }
                if (!json.optBoolean("ok")) {
                    toast("ساخت آهنگ ناموفق بود: " + json.optString("error", "هسته در دسترس نیست"))
                    return@runOnUiThread
                }
                val title = (if (instrumental) "آهنگ بی‌کلام" else lyrics.lineSequence().first().take(28)) + " · " + fa(stamp % 1000)
                val id = "created:$stamp"
                rememberCreated(id, title, json.getString("wav"), json.optLong("duration_ms"))
                allTracks = allTracks.filter { !it.id.startsWith("created:") } + createdTracks()
                byId = allTracks.associateBy { it.id }
                val made = byId[id]
                if (made != null) playFrom(listOf(made), 0)
                toast("آهنگ ساخته شد؛ فایل MIDI هم کنار آن ذخیره شد")
            }
        }
    }

    // ------------------------------------------------------------------ spatial audio

    private fun furnitureJson(level: String): JSONArray {
        fun item(kind: String, n: Int) = JSONObject().put("kind", JSONObject().put("kind", kind)).put("count", n)
        val arr = JSONArray()
        when (level) {
            "few" -> arr.put(item("sofa", 1))
            "mid" -> { arr.put(item("sofa", 1)); arr.put(item("carpet", 1)); arr.put(item("curtains", 2)); arr.put(item("bookshelf", 1)) }
            "many" -> { arr.put(item("sofa", 2)); arr.put(item("armchair", 2)); arr.put(item("carpet", 1)); arr.put(item("curtains", 4)); arr.put(item("bed", 1)) }
        }
        return arr
    }

    private fun spatialApply() {
        val p = runtimePrefs()
        if (!p.getBoolean("sp_on", false)) {
            client.setRoom(0, 0f, 0f)
            p.edit().putString("sp_summary", "").apply()
            return
        }
        val w = rpInt("sp_w", 45) / 10.0
        val d = rpInt("sp_d", 50) / 10.0
        val h = rpInt("sp_h", 26) / 10.0
        val mix = rpInt("sp_mix", 25) / 100.0
        fun pos(x: Double, y: Double, z: Double) = JSONObject().put("x", x).put("y", y).put("z", z)
        val settings = JSONObject()
            .put("enabled", true)
            .put("room", JSONObject().put("width", w).put("depth", d).put("height", h))
            .put("absorption", JSONObject().put("walls", 0.3).put("floor", 0.4).put("ceiling", 0.25))
            .put("layout", JSONObject().put("kind", "stereo")
                .put("left", pos(-w * 0.3, 0.0, d * 0.3)).put("right", pos(w * 0.3, 0.0, d * 0.3)))
            .put("listener", pos(0.0, 1.2.coerceAtMost(h - 0.1), 0.0))
            .put("hrtf_profile", JSONObject.NULL)
            .put("reverb_mix", mix)
            .put("furniture", furnitureJson(rpStr("sp_furn", "mid")))
        executor.execute {
            val out = try { NavaqNativeBridge.nativeRenderSpatial(settings.toString()) } catch (e: Throwable) { null }
            runOnUiThread {
                val json = try { JSONObject(out ?: "{}") } catch (_: Exception) { JSONObject() }
                if (json.has("error") || !json.has("rt60_s")) {
                    toast("تنظیمات اتاق معتبر نیست")
                    return@runOnUiThread
                }
                val rt60 = json.getDouble("rt60_s")
                val refl = json.optJSONArray("reflections")
                var sum = 0.0
                val n = minOf(refl?.length() ?: 0, 6)
                for (i in 0 until n) sum += refl!!.getJSONObject(i).getDouble("delay_ms")
                val delay = if (n > 0) (sum / n).toInt().coerceIn(5, 120) else 20
                val feedback = (rt60 * 0.4).coerceIn(0.05, 0.55).toFloat()
                client.setRoom(delay, feedback, (mix * 0.5).toFloat())
                p.edit().putString("sp_summary",
                    "زمان پژواک ≈ ${fa("%.2f".format(Locale.US, rt60))} ثانیه · فاصله بحرانی ≈ ${fa("%.1f".format(Locale.US, json.optDouble("critical_distance_m")))} متر").apply()
                if (screen == "settings" && stack.lastOrNull() == "sound") buildScreen()
            }
        }
    }

    private fun toggleRawThen(title: String, desc: String?, key: String, then: () -> Unit): Item.Row =
        Item.Row(title, desc.orEmpty()) {
            SettingsKit.toggleRow(this, title, desc, runtimePrefs().getBoolean(key, false)) { v ->
                runtimePrefs().edit().putBoolean(key, v).apply(); then(); buildScreen()
            }
        }

    private fun toggleRaw(title: String, desc: String?, key: String): Item.Row =
        Item.Row(title, desc.orEmpty()) {
            SettingsKit.toggleRow(this, title, desc, runtimePrefs().getBoolean(key, false)) { v ->
                runtimePrefs().edit().putBoolean(key, v).apply(); buildScreen()
            }
        }

    private fun prefSlider(title: String, key: String, min: Int, max: Int, def: Int, format: (Int) -> String, onCommit: () -> Unit = {}): Item.Row =
        Item.Row(title, title) {
            SettingsKit.sliderRow(this, title, min, max, rpInt(key, def), format, { }, { v ->
                runtimePrefs().edit().putInt(key, v).apply(); onCommit()
            })
        }

    private fun prefChoice(title: String, key: String, def: String, options: List<Pair<String, String>>, onCommit: () -> Unit = {}): Item.Row =
        Item.Row(title, options.joinToString(" ") { it.second }) {
            SettingsKit.choiceRow(this, title, options, rpStr(key, def)) { k ->
                runtimePrefs().edit().putString(key, k).apply(); onCommit(); buildScreen()
            }
        }

    private fun lyricsBox(): View = LinearLayout(this).apply {
        setPadding(0, T.dp(12), 0, T.dp(12))
        addView(android.widget.EditText(this@NavaqHomeActivity).apply {
            setText(createLyrics)
            hint = "هر خط یک بند از آهنگ است؛ مثلاً: شب از پنجره آرام می‌آید"
            setHintTextColor(T.fg2)
            setTextColor(T.fg)
            textSize = 15f * T.fontScale
            minLines = 4
            gravity = Gravity.TOP or Gravity.START
            inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE
            background = GradientDrawable().apply { cornerRadius = T.dpf(14f); setColor(Color.argb(60, 128, 134, 160)) }
            setPadding(T.dp(12), T.dp(10), T.dp(12), T.dp(10))
            addTextChangedListener(object : android.text.TextWatcher {
                override fun beforeTextChanged(t: CharSequence?, a: Int, b: Int, c: Int) = Unit
                override fun onTextChanged(t: CharSequence?, a: Int, b: Int, c: Int) = Unit
                override fun afterTextChanged(e: android.text.Editable?) { createLyrics = e?.toString().orEmpty() }
            })
        }, LinearLayout.LayoutParams(-1, -2))
    }

    private fun pageItems(id: String): List<Item> = when (id) {
        "room" -> roomItems()
        "downloads" -> listOf(
            Item.Sec("NAVAQ Downloader"),
            Item.Row("لینک", "url link video") { fieldRow("لینک مستقیم رسانهٔ عمومی Wikimedia Commons یا صفحهٔ فایل آن", dlUrl, true) { dlUrl = it } },
            Item.Row("نام فایل", "name") { fieldRow("نام فایل (اختیاری)", dlName, false) { dlName = it } },
            Item.Row("چسباندن", "paste") {
                SettingsKit.actionRow(this, Icon.QUEUE, "چسباندن لینک از کلیپ‌بورد", null) {
                    val cm = getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
                    val text = cm.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.text?.toString().orEmpty().trim()
                    if (text.isEmpty()) toast("کلیپ‌بورد خالی است") else { dlUrl = text; buildScreen() }
                }
            },
            toggleRaw("فقط صدا", "از ویدیو آهنگ (m4a) جدا می‌کند؛ اگر کدک پشتیبانی نشود فایل کامل می‌ماند", "dl_audio_only"),
            toggleRaw("فقط با وای‌فای", "جلوگیری از مصرف اینترنت همراه", "dl_wifi_only"),
            Item.Row("دانلود", "add") { SettingsKit.actionRow(this, Icon.DOWN, "دانلود", "فایل در کتابخانه‌ی NAVAQ ذخیره می‌شود") { dlAdd() } },
            Item.Row("توضیح", "") { SettingsKit.noteRow(this, "در این نسخه فقط رسانه‌های عمومی Wikimedia Commons پشتیبانی می‌شوند؛ لینک‌های سایت‌های دیگر پذیرفته نمی‌شوند. پیش از استفادهٔ مجدد، مجوز و الزام درج نام سازنده را بررسی کنید. DRM، جریان رمزگذاری‌شده و پخش زنده پشتیبانی نمی‌شود. دانلودها با امکان مکث و ادامه، از فایل موقت ادامه‌پذیرند.") },
            Item.Sec("صف دانلود"),
            Item.Row("فهرست", "queue jobs") { dlContainer() }
        )
        "dna" -> listOf(
            Item.Sec("کد DNA من"),
            Item.Row("کد من", "dna code") {
                LinearLayout(this).apply {
                    orientation = LinearLayout.VERTICAL
                    setPadding(0, T.dp(12), 0, T.dp(12))
                    addView(label(this@NavaqHomeActivity, "این کد سلیقه‌ی شما را (فقط نام هنرمندان) نشان می‌دهد. برای دوستتان بفرستید.", 12.5f, false, T.fg2))
                    addView(label(this@NavaqHomeActivity, dnaCode().take(48) + "…", 12f, false, T.fg).apply { setPadding(0, T.dp(8), 0, T.dp(8)); textDirection = View.TEXT_DIRECTION_LTR })
                }
            },
            Item.Row("کپی کد", "copy") { SettingsKit.actionRow(this, Icon.CHECK, "کپی کد DNA من", null) { copyText(dnaCode()) } },
            Item.Sec("مقایسه با دوست"),
            Item.Row("کد دوست", "friend code") {
                LinearLayout(this).apply {
                    setPadding(0, T.dp(12), 0, T.dp(12))
                    addView(android.widget.EditText(this@NavaqHomeActivity).apply {
                        setText(dnaInput)
                        hint = "کد DNA دوست را اینجا بچسبانید"
                        setHintTextColor(T.fg2); setTextColor(T.fg)
                        textSize = 14f * T.fontScale
                        textDirection = View.TEXT_DIRECTION_LTR
                        background = GradientDrawable().apply { cornerRadius = T.dpf(14f); setColor(Color.argb(60, 128, 134, 160)) }
                        setPadding(T.dp(12), T.dp(10), T.dp(12), T.dp(10))
                        addTextChangedListener(object : android.text.TextWatcher {
                            override fun beforeTextChanged(t: CharSequence?, a: Int, b: Int, c: Int) = Unit
                            override fun onTextChanged(t: CharSequence?, a: Int, b: Int, c: Int) = Unit
                            override fun afterTextChanged(e: android.text.Editable?) { dnaInput = e?.toString().orEmpty() }
                        })
                    }, LinearLayout.LayoutParams(-1, -2))
                }
            },
            Item.Row("مقایسه", "compare") { SettingsKit.actionRow(this, Icon.SEARCH, "مقایسه سلیقه", "محاسبه با هسته، بدون ارسال اطلاعات به جایی") { dnaCompare() } },
            Item.Row("نتیجه", "result") { SettingsKit.noteRow(this, dnaResult.ifBlank { "هنوز مقایسه‌ای انجام نشده است." }) },
            Item.Row("پلی‌لیست مشترک", "shared playlist") { SettingsKit.actionRow(this, Icon.MUSIC, "ساخت پلی‌لیست مشترک", "از هنرمندان مشترک در کتابخانه‌ی شما") { sharedPlaylist() } }
        )
        "create" -> listOf(
            Item.Sec("متن آهنگ"),
            Item.Row("متن", "lyrics text") { lyricsBox() },
            toggleRaw("آهنگ بی‌کلام", "فقط ملودی و هارمونی؛ بدون نیاز به متن", "cr_instr"),
            prefSlider("طول آهنگ بی‌کلام", "cr_lines", 2, 16, 6, { fa(it) + " بند" }),
            Item.Sec("سبک"),
            prefChoice("سبک موسیقی", "cr_style", "pop", listOf("pop" to "پاپ", "sad" to "غمگین", "rock" to "راک", "lofi" to "لوفای", "jazz" to "جاز", "persian" to "ایرانی", "cinematic" to "سینمایی")),
            prefChoice("گام", "cr_key", "C major", listOf("C major" to "دو ماژور", "G major" to "سل ماژور", "F major" to "فا ماژور", "A minor" to "لا مینور", "D minor" to "ر مینور", "E minor" to "می مینور")),
            prefSlider("سرعت", "cr_tempo", 60, 160, 100, { fa(it) + " BPM" }),
            Item.Sec("ساخت"),
            Item.Row("ساخت و پخش", "generate") {
                SettingsKit.actionRow(this, Icon.MUSIC, "ساخت و پخش", "ملودی و هارمونی را با هسته می‌سازد؛ WAV و MIDI ذخیره می‌شود") { generateSong() }
            }
        )
        "sound" -> {
            val gains = eqGains()
            val on = runtimePrefs().getBoolean("eq_on", false)
            val rows = mutableListOf<Item>(
                Item.Sec("اکولایزر"),
                Item.Row("اکولایزر", "equalizer eq") {
                    SettingsKit.toggleRow(this, "اکولایزر ۱۰ باندی", "پردازش با هسته‌ی Rust روی صدای پخش", on) { v -> saveEq(gains, v); buildScreen() }
                },
                Item.Row("قالب اکولایزر", "preset bass vocal rock") {
                    SettingsKit.choiceRow(this, "قالب", listOf("flat" to "فلت", "bass" to "باس", "treble" to "زیر", "vocal" to "وکال", "rock" to "راک", "classical" to "کلاسیک"), "") { k ->
                        saveEq(eqPresets.getValue(k).copyOf(), true); buildScreen()
                    }
                },
                Item.Sec("باندها")
            )
            eqFreqs.forEachIndexed { i, f ->
                val name = if (f >= 1000) fa(f / 1000) + " kHz" else fa(f) + " Hz"
                rows += Item.Row("باند $name", "band $f") {
                    SettingsKit.sliderRow(this, name, -12, 12, gains[i].toInt(), { fa(it) + " dB" }, { }, { v ->
                        gains[i] = v.toFloat(); saveEq(gains, true)
                    })
                }
            }
            rows += Item.Sec("صدای فضایی شخصی")
            rows += toggleRawThen("صدای فضایی اتاق", "طنین اتاق شما با مدل هسته روی صدا اعمال می‌شود", "sp_on") { spatialApply() }
            rows += prefSlider("عرض اتاق", "sp_w", 30, 120, 45, { fa(it / 10.0) + " متر" }) { spatialApply() }
            rows += prefSlider("طول اتاق", "sp_d", 30, 120, 50, { fa(it / 10.0) + " متر" }) { spatialApply() }
            rows += prefSlider("ارتفاع سقف", "sp_h", 20, 50, 26, { fa(it / 10.0) + " متر" }) { spatialApply() }
            rows += prefSlider("شدت طنین", "sp_mix", 0, 100, 25, { fa(it) + "٪" }) { spatialApply() }
            rows += prefChoice("مبلمان و پرده", "sp_furn", "mid", listOf("none" to "خالی", "few" to "کم", "mid" to "متوسط", "many" to "پر")) { spatialApply() }
            rows += Item.Row("نتیجه اتاق", "rt60") {
                SettingsKit.noteRow(this, rpStr("sp_summary", "").ifBlank { "با روشن کردن صدای فضایی، ویژگی‌های اتاق محاسبه می‌شود." })
            }
            rows
        }

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
            toggleRawThen("تحلیل پیشرفته ضرب", "تشخیص جداگانه‌ی طبل، باس و های‌هت با هسته", "haptics_adv") { client.setAdvancedHaptics(runtimePrefs().getBoolean("haptics_adv", false)) },
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
            Item.Row("گوش دادن مشترک", "room share together cast") { SettingsKit.navRow(this, Icon.MUSIC, "گوش دادن مشترک", "پخش هم‌زمان با دوستان روی همین وای‌فای، با کد اتصال") { open("room") } },
            Item.Row("دانلودها", "downloads") { SettingsKit.navRow(this, Icon.DOWN, "دانلودها", "صف دانلود، پیشرفت، لغو و پخش آفلاین") { open("downloads") } },
            Item.Row("Music DNA", "dna taste friend") { SettingsKit.navRow(this, Icon.DISC, "Music DNA", "مقایسه سلیقه با دوستان و پلی‌لیست مشترک، بدون سرور") { open("dna") } },
            Item.Row("ساخت آهنگ", "create song lyric melody") { SettingsKit.navRow(this, Icon.MUSIC, "ساخت آهنگ", "تبدیل متن به ملودی، آهنگ بی‌کلام") { open("create") } },
            Item.Row("صدا و اکولایزر", "sound equalizer eq") { SettingsKit.navRow(this, Icon.SLIDERS, "صدا و اکولایزر", "اکولایزر ۱۰ باندی و قالب‌های آماده") { open("sound") } },
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
                val hits = listOf("look", "glass", "layout", "sound", "create", "dna", "room", "downloads", "playback", "library", "backup")
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
        layer.background = GradientDrawable(
            GradientDrawable.Orientation.TOP_BOTTOM,
            intArrayOf(
                (T.neonB and 0x00FFFFFF) or (0x2A shl 24),
                top,
                T.bg,
                T.bg
            )
        )
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
            background = GradientDrawable(
                GradientDrawable.Orientation.TL_BR,
                intArrayOf(T.accent, T.neonB, T.neonA)
            ).apply {
                shape = GradientDrawable.OVAL
                setStroke(T.dp(1), Color.argb(180, 255, 255, 255))
            }
            elevation = T.dp(9).toFloat()
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
            addView(btn(Icon.TIMER, 52, 22, if (sleepRunnable != null || sleepEndId != null) T.neonC else T.fg) { sleepSheet() }, LinearLayout.LayoutParams(0, T.dp(52), 1f))
            addView(btn(Icon.LIBRARY, 52, 22) { addToPlaylistSheet(track.id) }, LinearLayout.LayoutParams(0, T.dp(52), 1f))
            addView(btn(Icon.QUEUE, 52, 22, if (playerQueue) T.neonA else T.fg) { playerQueue = !playerQueue; buildPlayer() }, LinearLayout.LayoutParams(0, T.dp(52), 1f))
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
        if (LanRoom.host != null) roomPush()
        sleepEndId?.let { target ->
            if (client.currentId != null && client.currentId != target) {
                // Track changed (seek past the end or skip): stop now.
                sleepRunnable?.let { handler.removeCallbacks(it) }
                sleepRunnable = null; sleepEndId = null
                client.pause()
            }
        }
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
        opts += "پایان این آهنگ" to { setSleepAtTrackEnd() }
        opts += "خاموش" to { setSleep(0) }
        showSheet("تایمر خواب", opts)
    }

    private var sleepEndId: String? = null

    private fun setSleepAtTrackEnd() {
        sleepRunnable?.let { handler.removeCallbacks(it) }
        sleepRunnable = null
        val id = client.currentId
        if (id == null) { toast("آهنگی در حال پخش نیست"); return }
        sleepEndId = id
        val r = Runnable { sleepEndId = null; sleepRunnable = null; client.pause(); toast("تایمر خواب: پایان آهنگ") }
        if (client.durationMs > 0L) {
            sleepRunnable = r
            handler.postDelayed(r, (client.durationMs - client.positionMs - 300L).coerceAtLeast(0L))
        }
        toast("پخش در پایان این آهنگ متوقف می‌شود")
        if (playerLayer != null) buildPlayer()
    }

    private fun setSleep(minutes: Int) {
        sleepRunnable?.let { handler.removeCallbacks(it) }
        sleepRunnable = null
        sleepEndId = null
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
