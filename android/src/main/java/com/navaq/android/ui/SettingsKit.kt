package com.navaq.android.ui

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.view.Gravity
import android.view.View
import android.view.inputmethod.EditorInfo
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.SeekBar
import android.widget.Switch
import android.widget.TextView

/** Builders for the glass settings rows. Every row reads the live [T] theme at build time. */
object SettingsKit {
    private fun lp(w: Int = LinearLayout.LayoutParams.MATCH_PARENT, h: Int = LinearLayout.LayoutParams.WRAP_CONTENT) =
        LinearLayout.LayoutParams(w, h)

    private fun gray() = Color.argb(70, 128, 134, 160)

    fun section(ctx: Context, text: String): TextView = label(ctx, text, 12f, true, T.fg2).apply {
        setPadding(T.dp(8), T.dp(18), T.dp(8), T.dp(8))
    }

    fun card(ctx: Context, rows: List<View>): LinearLayout = LinearLayout(ctx).apply {
        orientation = LinearLayout.VERTICAL
        glass()
        setPadding(T.dp(14), T.dp(4), T.dp(14), T.dp(4))
        rows.forEachIndexed { i, r ->
            if (i > 0) addView(View(ctx).apply { setBackgroundColor(Color.argb(36, 128, 134, 160)) }, lp(h = 1))
            addView(r, lp())
        }
        layoutParams = lp().apply { bottomMargin = T.dp(12) }
    }

    fun iconChip(ctx: Context, icon: Icon): View = FrameLayout(ctx).apply {
        background = GradientDrawable(
            GradientDrawable.Orientation.TL_BR,
            intArrayOf(T.accent, T.neonB)
        ).apply {
            cornerRadius = T.dpf(11f)
            setStroke(T.dp(1), (T.neonA and 0x00FFFFFF) or (0x90 shl 24))
        }
        elevation = T.dpf(2f)
        addView(
            IconView(ctx, icon, T.accentFg),
            FrameLayout.LayoutParams(T.dp(20), T.dp(20), Gravity.CENTER)
        )
        layoutParams = LinearLayout.LayoutParams(T.dp(34), T.dp(34))
    }

    private fun textColumn(ctx: Context, title: String, desc: String?): LinearLayout = LinearLayout(ctx).apply {
        orientation = LinearLayout.VERTICAL
        addView(label(ctx, title, 15f, true))
        if (!desc.isNullOrBlank()) {
            addView(label(ctx, desc, 12f, false, T.fg2).apply { setPadding(0, T.dp(3), 0, 0) })
        }
    }

    fun navRow(ctx: Context, icon: Icon, title: String, desc: String?, onClick: () -> Unit): View =
        LinearLayout(ctx).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, T.dp(12), 0, T.dp(12))
            addView(iconChip(ctx, icon))
            addView(textColumn(ctx, title, desc), LinearLayout.LayoutParams(0, -2, 1f).apply {
                marginStart = T.dp(12); marginEnd = T.dp(8)
            })
            addView(IconView(ctx, Icon.FORWARD, T.fg2), LinearLayout.LayoutParams(T.dp(20), T.dp(20)))
            isFocusable = true
            contentDescription = title
            pressable(onClick)
        }

    fun toggleRow(ctx: Context, title: String, desc: String?, checked: Boolean, onChange: (Boolean) -> Unit): View =
        LinearLayout(ctx).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, T.dp(12), 0, T.dp(12))
            addView(textColumn(ctx, title, desc), LinearLayout.LayoutParams(0, -2, 1f).apply { marginEnd = T.dp(10) })
            val states = arrayOf(intArrayOf(android.R.attr.state_checked), intArrayOf())
            addView(Switch(ctx).apply {
                isChecked = checked
                contentDescription = title
                trackTintList = ColorStateList(states, intArrayOf(T.accent, gray()))
                thumbTintList = ColorStateList(states, intArrayOf(Color.WHITE, Color.WHITE))
                setOnCheckedChangeListener { _, value -> onChange(value) }
            })
        }

    /** [onLive] fires while dragging; [onCommit] once on release. */
    fun sliderRow(
        ctx: Context, title: String, min: Int, max: Int, value: Int,
        format: (Int) -> String, onLive: (Int) -> Unit, onCommit: (Int) -> Unit
    ): View = LinearLayout(ctx).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(0, T.dp(12), 0, T.dp(8))
        val out = label(ctx, format(value), 13f, true, T.accent)
        addView(LinearLayout(ctx).apply {
            orientation = LinearLayout.HORIZONTAL
            addView(label(ctx, title, 15f, true), LinearLayout.LayoutParams(0, -2, 1f))
            addView(out)
        })
        addView(SeekBar(ctx).apply {
            this.max = max - min
            progress = value - min
            progressTintList = ColorStateList.valueOf(T.accent)
            thumbTintList = ColorStateList.valueOf(T.accent)
            progressBackgroundTintList = ColorStateList.valueOf(gray())
            setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                override fun onProgressChanged(seekBar: SeekBar?, progress: Int, fromUser: Boolean) {
                    if (!fromUser) return
                    out.text = format(progress + min)
                    onLive(progress + min)
                }
                override fun onStartTrackingTouch(seekBar: SeekBar?) = Unit
                override fun onStopTrackingTouch(seekBar: SeekBar?) {
                    onCommit((seekBar?.progress ?: 0) + min)
                }
            })
        }, lp().apply { topMargin = T.dp(6) })
    }

    private fun chip(ctx: Context, text: String, on: Boolean, onClick: () -> Unit): TextView =
        label(ctx, text, 13f, on, if (on) T.accentFg else T.fg).apply {
            setPadding(T.dp(14), T.dp(8), T.dp(14), T.dp(8))
            background = GradientDrawable(
                GradientDrawable.Orientation.LEFT_RIGHT,
                if (on) intArrayOf(T.accent, T.neonB) else intArrayOf(gray(), Color.argb(38, 128, 134, 160))
            ).apply {
                cornerRadius = T.dpf(40f)
                if (on) setStroke(T.dp(1), (T.neonA and 0x00FFFFFF) or (0x88 shl 24))
            }
            isFocusable = true
            contentDescription = if (on) "$text، انتخاب‌شده" else text
            pressable(onClick)
        }

    fun chips(ctx: Context, options: List<Pair<String, String>>, selected: String, onSelect: (String) -> Unit): View =
        FlowLayout(ctx, T.dp(8)).apply {
            options.forEach { (value, text) -> addView(chip(ctx, text, value == selected) { onSelect(value) }) }
        }

    fun choiceRow(
        ctx: Context, title: String, options: List<Pair<String, String>>, selected: String, onSelect: (String) -> Unit
    ): View = LinearLayout(ctx).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(0, T.dp(12), 0, T.dp(12))
        addView(label(ctx, title, 15f, true))
        addView(chips(ctx, options, selected, onSelect), lp().apply { topMargin = T.dp(10) })
    }

    val SWATCHES = listOf(
        "#3e91ff", "#8b5cf6", "#ff5d8f", "#ff8a3d", "#2fd08a", "#1ec8d6", "#ff4d4d", "#f5b82e"
    )

    fun swatchRow(ctx: Context, selected: String, onPick: (String) -> Unit, onHexInvalid: () -> Unit): View =
        LinearLayout(ctx).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(0, T.dp(12), 0, T.dp(12))
            addView(label(ctx, "رنگ تأکید", 15f, true))
            addView(FlowLayout(ctx, T.dp(10)).apply {
                SWATCHES.forEach { hex ->
                    addView(View(ctx).apply {
                        background = GradientDrawable().apply {
                            shape = GradientDrawable.OVAL
                            setColor(Color.parseColor(hex))
                            if (hex.equals(selected, true)) setStroke(T.dp(3), T.fg) else setStroke(T.dp(2), Color.argb(140, 255, 255, 255))
                        }
                        layoutParams = LinearLayout.LayoutParams(T.dp(38), T.dp(38))
                        isFocusable = true
                        contentDescription = "رنگ تأکید $hex"
                        pressable { onPick(hex) }
                    })
                }
            }, lp().apply { topMargin = T.dp(10) })
            addView(EditText(ctx).apply {
                setText(selected)
                hint = "#rrggbb"
                setTextColor(T.fg)
                setHintTextColor(T.fg2)
                textSize = 14f * T.fontScale
                inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
                imeOptions = EditorInfo.IME_ACTION_DONE
                textDirection = View.TEXT_DIRECTION_LTR
                setSingleLine()
                background = GradientDrawable().apply { cornerRadius = T.dpf(12f); setColor(gray()) }
                setPadding(T.dp(12), T.dp(10), T.dp(12), T.dp(10))
                setOnEditorActionListener { v, _, _ ->
                    val text = v.text.toString().trim()
                    if (UiSettings.isValidAccent(text)) onPick(text.lowercase()) else onHexInvalid()
                    true
                }
            }, lp().apply { topMargin = T.dp(12) })
        }

    fun actionRow(ctx: Context, icon: Icon, title: String, desc: String?, onClick: () -> Unit): View =
        navRow(ctx, icon, title, desc, onClick)

    fun noteRow(ctx: Context, text: String): View = label(ctx, text, 12.5f, false, T.fg2).apply {
        setPadding(0, T.dp(12), 0, T.dp(12))
        setLineSpacing(0f, 1.25f)
    }

    fun searchBox(ctx: Context, hint: String, initial: String, onText: (String) -> Unit): View =
        LinearLayout(ctx).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            glass(0.7f)
            setPadding(T.dp(14), T.dp(2), T.dp(14), T.dp(2))
            addView(IconView(ctx, Icon.SEARCH, T.fg2), LinearLayout.LayoutParams(T.dp(20), T.dp(20)))
            addView(EditText(ctx).apply {
                setText(initial)
                this.hint = hint
                setHintTextColor(T.fg2)
                setTextColor(T.fg)
                textSize = 15f * T.fontScale
                background = null
                setSingleLine()
                addTextChangedListener(object : TextWatcher {
                    override fun beforeTextChanged(s: CharSequence?, st: Int, c: Int, a: Int) = Unit
                    override fun onTextChanged(s: CharSequence?, st: Int, b: Int, c: Int) = Unit
                    override fun afterTextChanged(s: Editable?) = onText(s?.toString().orEmpty())
                })
            }, LinearLayout.LayoutParams(0, T.dp(46), 1f).apply { marginStart = T.dp(8) })
        }
}
