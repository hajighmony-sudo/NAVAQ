package com.navaq.android.ui

import android.content.Context
import android.content.res.Configuration
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.ColorFilter
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Path
import android.graphics.PixelFormat
import android.graphics.RectF
import android.graphics.Shader
import android.graphics.Typeface
import android.graphics.drawable.Drawable
import android.text.TextUtils
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import kotlin.math.max
import kotlin.math.min

/** Resolved colours and metrics for the current [UiSettings]. */
class GlassTheme(val s: UiSettings, val dark: Boolean, val density: Float) {
    val bg: Int = if (dark) 0xFF07080D.toInt() else 0xFFECEFF6.toInt()
    val fg: Int = if (dark) 0xFFF2F4FB.toInt() else 0xFF10131C.toInt()
    val fg2: Int = (fg and 0x00FFFFFF) or (0x9E shl 24)
    val glassBase: Int = if (dark) Color.rgb(12, 17, 32) else Color.WHITE
    /** ZX-inspired three-tone neon system; the user's chosen accent remains authoritative. */
    val neonA: Int = if (dark) Color.rgb(98, 231, 255) else Color.rgb(8, 127, 168)
    val neonB: Int = if (dark) Color.rgb(181, 107, 255) else Color.rgb(117, 64, 184)
    val neonC: Int = if (dark) Color.rgb(255, 200, 87) else Color.rgb(182, 106, 0)
    val accent: Int = s.accentColor()
    val accentFg: Int = run {
        val lum = 0.299 * Color.red(accent) + 0.587 * Color.green(accent) + 0.114 * Color.blue(accent)
        if (lum > 165) 0xFF10131C.toInt() else Color.WHITE
    }
    val fontScale: Float = s.fontScalePercent / 100f
    val radiusPx: Float get() = s.cornerRadiusDp * density
    val rowPadDp: Int = when (s.density) {
        Density.COMPACT -> 5
        Density.COZY -> 10
        Density.ROOMY -> 15
    }
    fun dp(v: Int): Int = (v * density + 0.5f).toInt()
    fun dpf(v: Float): Float = v * density
}

object GlassUi {
    lateinit var theme: GlassTheme
        private set

    fun update(context: Context, s: UiSettings) {
        val dark = when (s.theme) {
            ThemeMode.DARK -> true
            ThemeMode.LIGHT -> false
            ThemeMode.SYSTEM ->
                (context.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) ==
                    Configuration.UI_MODE_NIGHT_YES
        }
        theme = GlassTheme(s, dark, context.resources.displayMetrics.density)
    }
}

val T: GlassTheme get() = GlassUi.theme

fun fa(value: Any): String = value.toString().map { c ->
    if (c in '0'..'9') "۰۱۲۳۴۵۶۷۸۹"[c - '0'] else c
}.joinToString("")

fun fmtTime(ms: Long): String {
    val total = (ms.coerceAtLeast(0L) / 1000L).toInt()
    return fa("${total / 60}:${(total % 60).toString().padStart(2, '0')}")
}

fun label(
    context: Context,
    text: String,
    sizeSp: Float,
    bold: Boolean = false,
    color: Int = T.fg,
    lines: Int = 0
): TextView = TextView(context).apply {
    this.text = text
    textSize = sizeSp * T.fontScale
    setTextColor(color)
    typeface = if (bold) Typeface.DEFAULT_BOLD else Typeface.DEFAULT
    includeFontPadding = false
    if (lines > 0) {
        maxLines = lines
        ellipsize = TextUtils.TruncateAt.END
    }
}

/** Translucent glass panel; reads the live theme on every draw so sliders update it instantly. */
class GlassDrawable(private val radiusFactor: Float = 1f, private val topOnly: Boolean = false) : Drawable() {
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG)
    private val sheen = Paint(Paint.ANTI_ALIAS_FLAG)
    private val stroke = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE }
    private val rect = RectF()

    override fun draw(canvas: Canvas) {
        val t = T
        val b = bounds
        if (b.isEmpty) return
        val inset = t.dpf(0.5f)
        rect.set(b.left + inset, b.top + inset, b.right - inset, b.bottom - inset)
        val r = t.radiusPx * radiusFactor
        val bottomExtra = if (topOnly) r * 2 else 0f
        rect.bottom += bottomExtra
        val alpha = (t.s.glassOpacityPercent * 2.55f * (if (t.dark) 1f else 1.8f)).coerceIn(10f, 255f).toInt()
        fill.color = (t.glassBase and 0x00FFFFFF) or (alpha shl 24)
        canvas.save()
        canvas.clipRect(b)
        canvas.drawRoundRect(rect, r, r, fill)
        sheen.shader = LinearGradient(
            0f, rect.top, 0f, rect.bottom,
            Color.argb(if (t.dark) 40 else 120, 255, 255, 255), Color.TRANSPARENT, Shader.TileMode.CLAMP
        )
        canvas.drawRoundRect(rect, r, r, sheen)
        stroke.strokeWidth = t.dpf(1f)
        stroke.color = Color.argb(if (t.dark) 46 else 170, 255, 255, 255)
        canvas.drawRoundRect(rect, r, r, stroke)
        if (t.s.edgeGlow) {
            // Layered neon edge: user accent leads, ZX cyan/violet/gold add depth.
            stroke.strokeWidth = t.dpf(2f)
            stroke.shader = LinearGradient(
                rect.left, rect.top, rect.right, rect.bottom,
                intArrayOf(t.accent, t.neonB, t.neonC, t.neonA),
                floatArrayOf(0f, 0.38f, 0.72f, 1f),
                Shader.TileMode.CLAMP
            )
            stroke.alpha = if (t.dark) 178 else 112
            canvas.drawRoundRect(rect, r, r, stroke)
            stroke.shader = null
            stroke.alpha = 255
            // A restrained inner highlight keeps glass readable instead of over-glowing.
            stroke.strokeWidth = t.dpf(0.65f)
            stroke.color = Color.argb(if (t.dark) 34 else 92, 255, 255, 255)
            canvas.drawRoundRect(
                RectF(rect.left + t.dpf(1.5f), rect.top + t.dpf(1.5f), rect.right - t.dpf(1.5f), rect.bottom - t.dpf(1.5f)),
                (r - t.dpf(1.5f)).coerceAtLeast(0f),
                (r - t.dpf(1.5f)).coerceAtLeast(0f),
                stroke
            )
        }
        canvas.restore()
    }

    override fun setAlpha(alpha: Int) = Unit
    override fun setColorFilter(colorFilter: ColorFilter?) = Unit
    @Deprecated("Required by Drawable")
    override fun getOpacity(): Int = PixelFormat.TRANSLUCENT
}

fun View.glass(radiusFactor: Float = 1f, topOnly: Boolean = false) {
    background = GlassDrawable(radiusFactor, topOnly)
}

/** Invalidates every view so drawables that read the live theme repaint. */
fun refreshTree(view: View) {
    view.invalidate()
    if (view is ViewGroup) for (i in 0 until view.childCount) refreshTree(view.getChildAt(i))
}

fun View.pressable(onClick: () -> Unit) {
    isClickable = true
    isFocusable = true
    setOnClickListener { onClick() }
    setOnTouchListener { v, e ->
        when (e.action) {
            MotionEvent.ACTION_DOWN -> {
                v.animate().cancel()
                v.animate().alpha(0.82f).scaleX(0.985f).scaleY(0.985f).setDuration(90L).start()
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                v.animate().cancel()
                v.animate().alpha(1f).scaleX(1f).scaleY(1f).setDuration(150L).start()
            }
        }
        false
    }
}

enum class Icon {
    PLAY, PAUSE, NEXT, PREV, SHUFFLE, REPEAT, REPEAT_ONE, HEART, HEART_FILL, DOWN, BACK, FORWARD,
    SEARCH, SLIDERS, LIBRARY, MUSIC, QUEUE, TIMER, SPEED, PALETTE, GLASS, LAYOUT, HAPTIC, INFO,
    RESET, CHECK, MIC, DISC
}

/** Line icons drawn on a 24-unit grid so no drawable resources are needed. */
class IconView(context: Context, icon: Icon, tint: Int = T.fg) : View(context) {
    var icon: Icon = icon
        set(value) { field = value; invalidate() }
    var tint: Int = tint
        set(value) { field = value; invalidate() }
    private val line = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE; strokeWidth = 1.9f; strokeCap = Paint.Cap.ROUND; strokeJoin = Paint.Join.ROUND
    }
    private val solid = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL }
    private val p = Path()

    private fun ln(c: Canvas, a: Float, b: Float, x: Float, y: Float) = c.drawLine(a, b, x, y, line)
    private fun poly(c: Canvas, vararg v: Float) {
        p.reset(); p.moveTo(v[0], v[1])
        var i = 2
        while (i < v.size) { p.lineTo(v[i], v[i + 1]); i += 2 }
        c.drawPath(p, line)
    }
    private fun tri(c: Canvas, vararg v: Float) {
        p.reset(); p.moveTo(v[0], v[1]); p.lineTo(v[2], v[3]); p.lineTo(v[4], v[5]); p.close()
        c.drawPath(p, solid)
    }

    override fun onDraw(c: Canvas) {
        line.color = tint
        solid.color = tint
        val s = min(width, height) / 24f
        c.save()
        c.translate((width - 24f * s) / 2f, (height - 24f * s) / 2f)
        c.scale(s, s)
        when (icon) {
            Icon.PLAY -> tri(c, 7f, 4.5f, 7f, 19.5f, 19f, 12f)
            Icon.PAUSE -> { c.drawRoundRect(6f, 5f, 10f, 19f, 1.2f, 1.2f, solid); c.drawRoundRect(14f, 5f, 18f, 19f, 1.2f, 1.2f, solid) }
            Icon.NEXT -> { tri(c, 6f, 5f, 6f, 19f, 15f, 12f); ln(c, 18f, 5f, 18f, 19f) }
            Icon.PREV -> { tri(c, 18f, 5f, 18f, 19f, 9f, 12f); ln(c, 6f, 5f, 6f, 19f) }
            Icon.SHUFFLE -> {
                ln(c, 4f, 20f, 20f, 4f); ln(c, 4f, 4f, 9f, 9f); ln(c, 15f, 15f, 20f, 20f)
                poly(c, 16f, 4f, 20f, 4f, 20f, 8f); poly(c, 16f, 20f, 20f, 20f, 20f, 16f)
            }
            Icon.REPEAT, Icon.REPEAT_ONE -> {
                p.reset(); p.moveTo(4f, 11f); p.lineTo(4f, 9f); p.quadTo(4f, 6f, 7f, 6f); p.lineTo(19f, 6f); c.drawPath(p, line)
                poly(c, 16f, 3f, 19f, 6f, 16f, 9f)
                p.reset(); p.moveTo(20f, 13f); p.lineTo(20f, 15f); p.quadTo(20f, 18f, 17f, 18f); p.lineTo(5f, 18f); c.drawPath(p, line)
                poly(c, 8f, 15f, 5f, 18f, 8f, 21f)
                if (icon == Icon.REPEAT_ONE) poly(c, 11f, 10.5f, 12.5f, 9.5f, 12.5f, 14.5f)
            }
            Icon.HEART, Icon.HEART_FILL -> {
                p.reset(); p.moveTo(12f, 20.5f)
                p.cubicTo(4f, 15f, 2f, 10f, 5f, 6.5f); p.cubicTo(7.5f, 4f, 11f, 4.5f, 12f, 7.5f)
                p.cubicTo(13f, 4.5f, 16.5f, 4f, 19f, 6.5f); p.cubicTo(22f, 10f, 20f, 15f, 12f, 20.5f); p.close()
                c.drawPath(p, if (icon == Icon.HEART_FILL) solid else line)
            }
            Icon.DOWN -> poly(c, 6f, 9f, 12f, 15f, 18f, 9f)
            Icon.BACK -> poly(c, 9f, 6f, 15f, 12f, 9f, 18f)
            Icon.FORWARD -> poly(c, 15f, 6f, 9f, 12f, 15f, 18f)
            Icon.SEARCH -> { c.drawCircle(11f, 11f, 7f, line); ln(c, 16f, 16f, 21f, 21f) }
            Icon.SLIDERS -> {
                ln(c, 4f, 7f, 13f, 7f); ln(c, 17f, 7f, 20f, 7f); ln(c, 4f, 17f, 7f, 17f); ln(c, 11f, 17f, 20f, 17f)
                c.drawCircle(15f, 7f, 2f, line); c.drawCircle(9f, 17f, 2f, line)
            }
            Icon.LIBRARY -> { ln(c, 5f, 5f, 5f, 19f); ln(c, 10f, 5f, 10f, 19f); ln(c, 15f, 6.5f, 19.5f, 19f) }
            Icon.MUSIC -> {
                poly(c, 9f, 18f, 9f, 5f, 20f, 3f, 20f, 16f)
                c.drawCircle(6.5f, 18f, 2.5f, line); c.drawCircle(17.5f, 16f, 2.5f, line)
            }
            Icon.QUEUE -> { ln(c, 4f, 6f, 16f, 6f); ln(c, 4f, 12f, 16f, 12f); ln(c, 4f, 18f, 12f, 18f); tri(c, 18f, 14f, 22f, 17f, 18f, 20f) }
            Icon.TIMER -> { c.drawCircle(12f, 13f, 8f, line); poly(c, 12f, 9f, 12f, 13f, 15f, 15f); ln(c, 9f, 2f, 15f, 2f) }
            Icon.SPEED -> { c.drawArc(4f, 10f, 20f, 26f, 180f, 180f, false, line); ln(c, 12f, 18f, 16f, 12f) }
            Icon.PALETTE -> {
                c.drawCircle(12f, 12f, 9f, line)
                c.drawCircle(8f, 11f, 1.1f, solid); c.drawCircle(12f, 7.5f, 1.1f, solid); c.drawCircle(16f, 11f, 1.1f, solid)
            }
            Icon.GLASS -> {
                p.reset(); p.moveTo(12f, 3f)
                p.cubicTo(15f, 7f, 18f, 10f, 18f, 14f); p.cubicTo(18f, 17.5f, 15.3f, 20f, 12f, 20f)
                p.cubicTo(8.7f, 20f, 6f, 17.5f, 6f, 14f); p.cubicTo(6f, 10f, 9f, 7f, 12f, 3f); p.close()
                c.drawPath(p, line)
            }
            Icon.LAYOUT -> {
                c.drawRoundRect(4f, 4f, 11f, 11f, 1.5f, 1.5f, line); c.drawRoundRect(13f, 4f, 20f, 11f, 1.5f, 1.5f, line)
                c.drawRoundRect(4f, 13f, 11f, 20f, 1.5f, 1.5f, line); c.drawRoundRect(13f, 13f, 20f, 20f, 1.5f, 1.5f, line)
            }
            Icon.HAPTIC -> { poly(c, 8f, 6f, 5f, 12f, 8f, 18f); poly(c, 16f, 6f, 19f, 12f, 16f, 18f); ln(c, 12f, 8f, 12f, 16f) }
            Icon.INFO -> { c.drawCircle(12f, 12f, 9f, line); ln(c, 12f, 11f, 12f, 16f); c.drawCircle(12f, 8f, 1.1f, solid) }
            Icon.RESET -> { c.drawArc(4f, 4f, 20f, 20f, 40f, 280f, false, line); poly(c, 4f, 3f, 4f, 8f, 9f, 8f) }
            Icon.CHECK -> poly(c, 5f, 12f, 10f, 17f, 19f, 7f)
            Icon.MIC -> {
                c.drawRoundRect(9f, 3f, 15f, 14f, 3f, 3f, line)
                p.reset(); p.moveTo(6f, 11f); p.quadTo(6f, 18f, 12f, 18f); p.quadTo(18f, 18f, 18f, 11f); c.drawPath(p, line)
                ln(c, 12f, 18f, 12f, 21f)
            }
            Icon.DISC -> { c.drawCircle(12f, 12f, 9f, line); c.drawCircle(12f, 12f, 2.5f, line) }
        }
        c.restore()
    }
}

/** Wrapping row layout (used for choice chips and colour swatches). Honors RTL. */
class FlowLayout(context: Context, private val gapPx: Int) : ViewGroup(context) {
    override fun onMeasure(widthSpec: Int, heightSpec: Int) {
        val maxW = MeasureSpec.getSize(widthSpec)
        var x = 0; var y = 0; var rowH = 0
        for (i in 0 until childCount) {
            val c = getChildAt(i)
            if (c.visibility == GONE) continue
            val lp = c.layoutParams
            c.measure(
                getChildMeasureSpec(MeasureSpec.makeMeasureSpec(maxW, MeasureSpec.AT_MOST), 0, lp?.width ?: LayoutParams.WRAP_CONTENT),
                getChildMeasureSpec(MeasureSpec.makeMeasureSpec(0, MeasureSpec.UNSPECIFIED), 0, lp?.height ?: LayoutParams.WRAP_CONTENT)
            )
            if (x > 0 && x + c.measuredWidth > maxW) { x = 0; y += rowH + gapPx; rowH = 0 }
            x += c.measuredWidth + gapPx
            rowH = max(rowH, c.measuredHeight)
        }
        setMeasuredDimension(maxW, y + rowH)
    }

    override fun onLayout(changed: Boolean, l: Int, t: Int, r: Int, b: Int) {
        val w = r - l
        val rtl = layoutDirection == LAYOUT_DIRECTION_RTL
        var x = 0; var y = 0; var rowH = 0
        for (i in 0 until childCount) {
            val c = getChildAt(i)
            if (c.visibility == GONE) continue
            if (x > 0 && x + c.measuredWidth > w) { x = 0; y += rowH + gapPx; rowH = 0 }
            val left = if (rtl) w - x - c.measuredWidth else x
            c.layout(left, y, left + c.measuredWidth, y + c.measuredHeight)
            x += c.measuredWidth + gapPx
            rowH = max(rowH, c.measuredHeight)
        }
    }
}
