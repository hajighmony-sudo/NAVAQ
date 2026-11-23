package com.navaq.android.ui

import android.animation.ValueAnimator
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapShader
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.LinearGradient
import android.graphics.Matrix
import android.graphics.Paint
import android.graphics.RadialGradient
import android.graphics.RectF
import android.graphics.RenderEffect
import android.graphics.Shader
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.LruCache
import android.util.Size
import android.view.View
import android.view.animation.LinearInterpolator
import java.util.concurrent.Executors
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sin

/** Animated colour blobs behind every glass panel. */
class WallpaperView(context: Context) : View(context) {
    private var hues = floatArrayOf(212f, 268f, 172f)
    private var sat = 0.85f
    private var phase = 0f
    private var animator: ValueAnimator? = null
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private var albumHue = 210f
    private var shaders = arrayOfNulls<RadialGradient>(3)
    private var shaderKey = ""

    fun setAlbumHue(h: Float) {
        if (h == albumHue) return
        albumHue = h
        if (T.s.wallpaper == Wallpaper.ALBUM) configure()
    }

    fun configure() {
        val t = T
        when (t.s.wallpaper) {
            Wallpaper.AURORA -> { hues = floatArrayOf(212f, 268f, 172f); sat = 0.85f }
            Wallpaper.SUNSET -> { hues = floatArrayOf(20f, 335f, 48f); sat = 0.88f }
            Wallpaper.OCEAN -> { hues = floatArrayOf(196f, 232f, 160f); sat = 0.80f }
            Wallpaper.FOREST -> { hues = floatArrayOf(140f, 95f, 182f); sat = 0.70f }
            Wallpaper.MONO -> { hues = floatArrayOf(220f, 220f, 220f); sat = 0.04f }
            Wallpaper.ALBUM -> { hues = floatArrayOf(albumHue, albumHue + 60f, albumHue - 55f); sat = 0.80f }
        }
        shaderKey = ""
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val r = t.s.blurPx / 3f
            setRenderEffect(if (r > 0.5f) RenderEffect.createBlurEffect(r, r, Shader.TileMode.CLAMP) else null)
        }
        updateAnimation()
        invalidate()
    }

    private fun updateAnimation() {
        animator?.cancel()
        animator = null
        if (T.s.animations && isAttachedToWindow) {
            animator = ValueAnimator.ofFloat(0f, (Math.PI * 2).toFloat()).apply {
                duration = 40_000L
                repeatCount = ValueAnimator.INFINITE
                interpolator = LinearInterpolator()
                addUpdateListener { phase = it.animatedValue as Float; invalidate() }
                start()
            }
        }
    }

    override fun onAttachedToWindow() { super.onAttachedToWindow(); updateAnimation() }
    override fun onDetachedFromWindow() { animator?.cancel(); animator = null; super.onDetachedFromWindow() }

    override fun onDraw(c: Canvas) {
        val t = T
        c.drawColor(t.bg)
        val w = width.toFloat(); val h = height.toFloat()
        if (w <= 0f) return
        val big = max(w, h)
        val a = if (t.dark) 0.75f else 0.55f
        val v = if (t.dark) 0.85f else 1f
        val cx = floatArrayOf(
            w * (0.85f + 0.10f * cos(phase)), w * (0.10f + 0.12f * sin(phase * 1.0f + 1f)), w * (0.90f + 0.08f * sin(phase + 2f))
        )
        val cy = floatArrayOf(
            h * (0.08f + 0.06f * sin(phase)), h * (0.80f + 0.06f * cos(phase + 1f)), h * (0.45f + 0.08f * cos(phase + 2f))
        )
        val rad = floatArrayOf(big * 0.62f, big * 0.60f, big * 0.38f)
        for (i in 0..2) {
            val col = Color.HSVToColor(floatArrayOf(((hues[i] % 360f) + 360f) % 360f, sat, v))
            paint.shader = RadialGradient(
                cx[i], cy[i], rad[i],
                intArrayOf((col and 0x00FFFFFF) or ((a * 255).toInt() shl 24), (col and 0x00FFFFFF) or ((a * 110).toInt() shl 24), col and 0x00FFFFFF),
                floatArrayOf(0f, 0.55f, 1f), Shader.TileMode.CLAMP
            )
            c.drawCircle(cx[i], cy[i], rad[i], paint)
        }
    }
}

/** Loads embedded album art off the main thread. */
object ArtLoader {
    private val cache = LruCache<String, Bitmap>(80)
    private val missing = HashSet<String>()
    private val executor = Executors.newFixedThreadPool(2)
    private val main = Handler(Looper.getMainLooper())

    fun cached(key: String): Bitmap? = cache.get(key)

    fun load(context: Context, key: String, uri: String, done: (Bitmap?) -> Unit) {
        cache.get(key)?.let { done(it); return }
        if (key in missing || Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) { done(null); return }
        val app = context.applicationContext
        executor.execute {
            val bmp = try {
                app.contentResolver.loadThumbnail(Uri.parse(uri), Size(320, 320), null)
            } catch (_: Exception) {
                null
            }
            main.post {
                if (bmp != null) cache.put(key, bmp) else synchronized(missing) { missing.add(key) }
                done(bmp)
            }
        }
    }
}

fun hueOf(text: String): Float = (abs(text.hashCode()) % 360).toFloat()

/** Album cover: embedded artwork when present, otherwise a deterministic gradient. */
class CoverView(context: Context) : View(context) {
    private var hue = 210f
    private var bitmap: Bitmap? = null
    private var boundKey: String? = null
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val rect = RectF()
    private val matrix = Matrix()

    fun bind(key: String, uri: String?, seed: String) {
        boundKey = key
        hue = hueOf(seed)
        bitmap = ArtLoader.cached(key)
        invalidate()
        if (bitmap == null && uri != null) {
            ArtLoader.load(context, key, uri) { bmp ->
                if (boundKey == key && bmp != null) { bitmap = bmp; invalidate() }
            }
        }
    }

    override fun onDraw(c: Canvas) {
        val w = width.toFloat(); val h = height.toFloat()
        if (w <= 0f) return
        val r = when (T.s.coverShape) {
            CoverShape.SQUARE -> T.dpf(6f)
            CoverShape.ROUNDED -> min(w, h) * 0.22f
            CoverShape.CIRCLE -> min(w, h) / 2f
        }
        rect.set(0f, 0f, w, h)
        val bmp = bitmap
        if (bmp != null) {
            val shader = BitmapShader(bmp, Shader.TileMode.CLAMP, Shader.TileMode.CLAMP)
            matrix.setScale(w / bmp.width, h / bmp.height)
            shader.setLocalMatrix(matrix)
            paint.shader = shader
            c.drawRoundRect(rect, r, r, paint)
            return
        }
        paint.shader = LinearGradient(
            0f, 0f, w, h,
            Color.HSVToColor(floatArrayOf(hue, 0.72f, 0.88f)),
            Color.HSVToColor(floatArrayOf((hue + 55f) % 360f, 0.78f, 0.55f)), Shader.TileMode.CLAMP
        )
        c.drawRoundRect(rect, r, r, paint)
        paint.shader = RadialGradient(w * 0.25f, h * 0.15f, max(w, h) * 0.7f, Color.argb(110, 255, 255, 255), Color.TRANSPARENT, Shader.TileMode.CLAMP)
        c.drawRoundRect(rect, r, r, paint)
        paint.shader = null
        paint.style = Paint.Style.STROKE
        paint.strokeWidth = w * 0.05f
        paint.color = Color.argb(150, 255, 255, 255)
        paint.strokeCap = Paint.Cap.ROUND
        // Simple note glyph so artwork-less covers still read as music.
        val x = w * 0.5f; val y = h * 0.62f
        c.drawLine(x + w * 0.10f, y, x + w * 0.10f, h * 0.28f, paint)
        c.drawLine(x + w * 0.10f, h * 0.28f, x + w * 0.24f, h * 0.34f, paint)
        c.drawCircle(x - w * 0.02f, y, w * 0.10f, paint)
        paint.style = Paint.Style.FILL
    }
}

/** Three animated bars; decorative "now playing" indicator. */
class BarsView(context: Context) : View(context) {
    var playing = true
        set(value) { field = value; invalidate() }
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private var t = 0f
    private val animator = ValueAnimator.ofFloat(0f, 6.2832f).apply {
        duration = 900L; repeatCount = ValueAnimator.INFINITE; interpolator = LinearInterpolator()
        addUpdateListener { t = it.animatedValue as Float; invalidate() }
    }

    override fun onAttachedToWindow() { super.onAttachedToWindow(); if (T.s.animations) animator.start() }
    override fun onDetachedFromWindow() { animator.cancel(); super.onDetachedFromWindow() }

    override fun onDraw(c: Canvas) {
        paint.color = T.accent
        val w = width.toFloat(); val h = height.toFloat()
        val bw = w / 5f
        for (i in 0..2) {
            val f = if (playing && T.s.animations) 0.25f + 0.75f * (0.5f + 0.5f * sin(t + i * 1.7f)) else 0.35f
            val bh = h * f
            c.drawRoundRect(i * bw * 2f, h - bh, i * bw * 2f + bw, h, bw / 2f, bw / 2f, paint)
        }
    }
}
