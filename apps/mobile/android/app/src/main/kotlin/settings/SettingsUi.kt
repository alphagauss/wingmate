package dev.wingmate.mobile.settings

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.text.TextUtils
import android.view.Gravity
import android.view.View
import android.widget.EditText
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import androidx.appcompat.content.res.AppCompatResources
import androidx.appcompat.widget.AppCompatImageButton
import androidx.appcompat.widget.SwitchCompat
import androidx.core.view.ViewCompat
import dev.wingmate.mobile.R
import dev.wingmate.mobile.theme.ConnectionUi
import dev.wingmate.mobile.theme.dp
import dev.wingmate.mobile.theme.rounded

internal class SettingsUi(private val context: Context, theme: String) {
    val colors = ConnectionUi(context, when (theme) {
        "dark" -> true
        "light" -> false
        else -> null
    })
    private val dark = colors.background != Color.WHITE
    private val secondary = Color.parseColor(if (dark) "#adb2b8" else "#61666b")
    val success = Color.parseColor(if (dark) "#8dd5ad" else "#26724a")

    fun page(title: String, backLabel: String, back: () -> Unit, content: LinearLayout.() -> Unit) =
        ScrollView(context).apply {
            isFillViewport = true
            isFocusableInTouchMode = true
            background = GradientDrawable(GradientDrawable.Orientation.TOP_BOTTOM, intArrayOf(
                Color.parseColor(if (dark) "#202733" else "#eaf0fc"), colors.background
            ))
            addView(column().apply {
                setPadding(dp(16), dp(12), dp(16), dp(28))
                addView(AppCompatImageButton(context).apply {
                    setImageResource(R.drawable.ic_settings_back)
                    imageTintList = ColorStateList.valueOf(colors.foreground)
                    background = ripple(24, colors.surface)
                    contentDescription = backLabel
                    setOnClickListener { back() }
                }, dp(44), dp(44))
                addView(colors.text(title, 28f, bold = true).apply {
                    ViewCompat.setAccessibilityHeading(this, true)
                }, LinearLayout.LayoutParams(-1, -2).apply {
                    setMargins(dp(8), dp(12), dp(8), dp(16))
                })
                content()
            })
        }

    fun LinearLayout.section(title: String? = null, content: LinearLayout.() -> Unit) {
        if (title != null) addView(colors.text(title, 14f, secondary, bold = true).apply {
            ViewCompat.setAccessibilityHeading(this, true)
        }, LinearLayout.LayoutParams(-1, -2).apply { setMargins(dp(8), 0, dp(8), dp(8)) })
        addView(column().apply {
            background = context.rounded(colors.surface, 22)
            clipToOutline = true
            content()
        }, LinearLayout.LayoutParams(-1, -2).apply { bottomMargin = dp(22) })
    }

    fun row(
        title: String,
        subtitle: String? = null,
        value: String? = null,
        icon: Int? = null,
        valueColor: Int = secondary,
        maxSubtitleLines: Int = 2,
        tone: Tone = Tone.AI,
        action: (() -> Unit)? = null
    ) = LinearLayout(context).apply {
        gravity = Gravity.CENTER_VERTICAL
        minimumHeight = dp(if (subtitle == null) 52 else 64)
        setPadding(dp(16), dp(8), dp(16), dp(8))
        if (icon != null) addView(ImageView(context).apply {
            setImageResource(icon)
            setColorFilter(Color.parseColor(if (dark) tone.darkInk else tone.lightInk))
            background = context.rounded(Color.parseColor(if (dark) tone.darkFill else tone.lightFill), 16)
            setPadding(dp(7), dp(7), dp(7), dp(7))
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        }, LinearLayout.LayoutParams(dp(32), dp(32)).apply { marginEnd = dp(12) })
        addView(column().apply {
            addView(colors.text(title, 15f))
            if (subtitle != null) addView(colors.text(subtitle, 13f, secondary).apply {
                maxLines = maxSubtitleLines
                ellipsize = TextUtils.TruncateAt.END
            })
        }, LinearLayout.LayoutParams(0, -2, 1f))
        if (value != null) addView(colors.text(value, 13f, valueColor).apply {
            maxWidth = dp(130)
            maxLines = 2
            ellipsize = TextUtils.TruncateAt.END
        }, LinearLayout.LayoutParams(-2, -2).apply { marginStart = dp(8) })
        if (action != null) {
            addView(ImageView(context).apply {
                setImageResource(R.drawable.ic_settings_chevron)
                setColorFilter(colors.muted)
                importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
            }, LinearLayout.LayoutParams(dp(16), dp(20)).apply { marginStart = dp(6) })
            background = ripple(0, Color.TRANSPARENT)
            isFocusable = true
            setOnClickListener { action() }
        }
    }

    fun toggle(title: String, enabled: Boolean, change: (Boolean) -> Unit) =
        SwitchCompat(context).apply {
            text = title
            textSize = 15f
            setTextColor(colors.foreground)
            minimumHeight = dp(52)
            setPadding(dp(16), dp(8), dp(16), dp(8))
            switchMinWidth = dp(48)
            isChecked = enabled
            setOnCheckedChangeListener { _, value -> change(value) }
        }

    fun search(label: String) = EditText(context).apply {
        hint = label
        textSize = 16f
        setTextColor(colors.foreground)
        setHintTextColor(secondary)
        setPadding(dp(16), 0, dp(16), 0)
        minimumHeight = dp(46)
        isSingleLine = true
        background = context.rounded(colors.surface, 24)
        setCompoundDrawablesRelativeWithIntrinsicBounds(searchIcon(), null, null, null)
        compoundDrawablePadding = dp(8)
    }

    fun searchButton(label: String, action: () -> Unit) = colors.text(label, 16f, secondary).apply {
        gravity = Gravity.CENTER_VERTICAL
        minimumHeight = dp(46)
        setPadding(dp(16), 0, dp(16), 0)
        background = ripple(24, colors.surface)
        setCompoundDrawablesRelativeWithIntrinsicBounds(searchIcon(), null, null, null)
        compoundDrawablePadding = dp(8)
        isFocusable = true
        setOnClickListener { action() }
    }

    private fun searchIcon() = AppCompatResources.getDrawable(context, R.drawable.ic_settings_search)
        ?.mutate()?.apply { setTint(secondary) }

    private fun column() = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
    private fun dp(value: Int) = context.dp(value)
    private fun ripple(radius: Int, fill: Int) =
        RippleDrawable(ColorStateList.valueOf(colors.border), context.rounded(fill, radius),
            context.rounded(Color.WHITE, radius))

    enum class Tone(val lightFill: String, val lightInk: String, val darkFill: String, val darkInk: String) {
        AI("#e2eeff", "#285fa7", "#23364f", "#93bdff"),
        CAPABILITIES("#fff0dc", "#785213", "#49391f", "#f0d3a5"),
        APPEARANCE("#eee8fb", "#594788", "#38324b", "#d7c8ff"),
        GENERAL("#e6f0e1", "#3a6533", "#2e402a", "#bfdcb5"),
        HOSTS("#def3ee", "#1c6256", "#1e403b", "#a0e0d0"),
        HELP("#e0f1f6", "#246378", "#253e49", "#a7d9ed")
    }
}
