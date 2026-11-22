package com.navaq.android.ui

import android.content.Context
import android.graphics.Color

enum class ThemeMode { SYSTEM, LIGHT, DARK }
enum class Wallpaper { AURORA, SUNSET, OCEAN, FOREST, MONO, ALBUM }
enum class Density { COMPACT, COZY, ROOMY }
enum class CoverShape { SQUARE, ROUNDED, CIRCLE }
enum class ListStyle { LIST, GRID }
enum class NavStyle { FLOATING, DOCKED }
enum class SortOrder { TITLE, ARTIST, DURATION }

/** Kotlin mirror of the Rust `ui_settings::UiSettings` schema (plus library sorting). */
data class UiSettings(
    val theme: ThemeMode = ThemeMode.DARK,
    val accent: String = "#3e91ff",
    val wallpaper: Wallpaper = Wallpaper.AURORA,
    val blurPx: Int = 28,
    val glassOpacityPercent: Int = 22,
    val cornerRadiusDp: Int = 24,
    val fontScalePercent: Int = 100,
    val density: Density = Density.COZY,
    val coverShape: CoverShape = CoverShape.ROUNDED,
    val listStyle: ListStyle = ListStyle.LIST,
    val navStyle: NavStyle = NavStyle.FLOATING,
    val edgeGlow: Boolean = true,
    val animations: Boolean = true,
    val visualizer: Boolean = true,
    val spinCircularCover: Boolean = false,
    val showDuration: Boolean = true,
    val showArtistsTab: Boolean = true,
    val sort: SortOrder = SortOrder.TITLE,
    val minTrackSeconds: Int = 30
) {
    fun accentColor(): Int = try {
        Color.parseColor(accent)
    } catch (_: IllegalArgumentException) {
        Color.parseColor(DEFAULT_ACCENT)
    }

    /** Clamps ranges and repairs an unusable accent, matching the Rust `sanitized()`. */
    fun sanitized(): UiSettings = copy(
        accent = if (isValidAccent(accent)) accent else DEFAULT_ACCENT,
        blurPx = blurPx.coerceIn(0, 60),
        glassOpacityPercent = glassOpacityPercent.coerceIn(4, 85),
        cornerRadiusDp = cornerRadiusDp.coerceIn(6, 38),
        fontScalePercent = fontScalePercent.coerceIn(85, 130),
        minTrackSeconds = minTrackSeconds.coerceIn(0, 120)
    )

    companion object {
        const val DEFAULT_ACCENT = "#3e91ff"
        private val ACCENT = Regex("^#[0-9a-fA-F]{6}$")
        fun isValidAccent(value: String) = ACCENT.matches(value)

        val AuroraGlass = UiSettings()
        val Sakura = UiSettings(
            theme = ThemeMode.LIGHT, accent = "#ff5d8f", wallpaper = Wallpaper.SUNSET,
            blurPx = 30, glassOpacityPercent = 50, cornerRadiusDp = 28, edgeGlow = false
        )
        val Midnight = UiSettings(
            accent = "#8b5cf6", wallpaper = Wallpaper.OCEAN,
            blurPx = 36, glassOpacityPercent = 30, cornerRadiusDp = 20
        )
        val Minimal = UiSettings(
            theme = ThemeMode.SYSTEM, accent = "#2fd08a", wallpaper = Wallpaper.MONO,
            blurPx = 44, glassOpacityPercent = 40, cornerRadiusDp = 12, edgeGlow = false
        )
    }
}

/** SharedPreferences persistence; values that fail to parse fall back to defaults. */
class UiSettingsStore(context: Context) {
    private val prefs = context.applicationContext
        .getSharedPreferences("navaq_ui_settings", Context.MODE_PRIVATE)

    fun load(): UiSettings {
        val d = UiSettings()
        return UiSettings(
            theme = enumOf("theme", d.theme),
            accent = prefs.getString("accent", d.accent) ?: d.accent,
            wallpaper = enumOf("wallpaper", d.wallpaper),
            blurPx = prefs.getInt("blur", d.blurPx),
            glassOpacityPercent = prefs.getInt("glass", d.glassOpacityPercent),
            cornerRadiusDp = prefs.getInt("radius", d.cornerRadiusDp),
            fontScalePercent = prefs.getInt("font", d.fontScalePercent),
            density = enumOf("density", d.density),
            coverShape = enumOf("cover", d.coverShape),
            listStyle = enumOf("list", d.listStyle),
            navStyle = enumOf("nav", d.navStyle),
            edgeGlow = prefs.getBoolean("glow", d.edgeGlow),
            animations = prefs.getBoolean("anim", d.animations),
            visualizer = prefs.getBoolean("viz", d.visualizer),
            spinCircularCover = prefs.getBoolean("spin", d.spinCircularCover),
            showDuration = prefs.getBoolean("dur", d.showDuration),
            showArtistsTab = prefs.getBoolean("artists", d.showArtistsTab),
            sort = enumOf("sort", d.sort),
            minTrackSeconds = prefs.getInt("minlen", d.minTrackSeconds)
        ).sanitized()
    }

    fun save(s: UiSettings) {
        prefs.edit()
            .putString("theme", s.theme.name).putString("accent", s.accent)
            .putString("wallpaper", s.wallpaper.name).putInt("blur", s.blurPx)
            .putInt("glass", s.glassOpacityPercent).putInt("radius", s.cornerRadiusDp)
            .putInt("font", s.fontScalePercent).putString("density", s.density.name)
            .putString("cover", s.coverShape.name).putString("list", s.listStyle.name)
            .putString("nav", s.navStyle.name).putBoolean("glow", s.edgeGlow)
            .putBoolean("anim", s.animations).putBoolean("viz", s.visualizer)
            .putBoolean("spin", s.spinCircularCover).putBoolean("dur", s.showDuration)
            .putBoolean("artists", s.showArtistsTab).putString("sort", s.sort.name)
            .putInt("minlen", s.minTrackSeconds)
            .apply()
    }

    private inline fun <reified E : Enum<E>> enumOf(key: String, fallback: E): E {
        val raw = prefs.getString(key, null) ?: return fallback
        return enumValues<E>().firstOrNull { it.name == raw } ?: fallback
    }
}

/** Favourite track ids, kept locally. */
class FavoriteStore(context: Context) {
    private val prefs = context.applicationContext
        .getSharedPreferences("navaq_favorites", Context.MODE_PRIVATE)

    fun all(): Set<String> = prefs.getStringSet("ids", emptySet()).orEmpty().toSet()
    fun isFavorite(id: String) = id in all()

    /** Returns the new state. */
    fun toggle(id: String): Boolean {
        val next = all().toMutableSet()
        val on = next.add(id)
        if (!on) next.remove(id)
        prefs.edit().putStringSet("ids", next).apply()
        return on
    }
}
