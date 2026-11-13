package dev.wingmate.mobile.settings

import android.content.Context
import android.content.res.ColorStateList
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.text.TextUtils
import android.view.Gravity
import android.view.View
import android.widget.EditText
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import androidx.appcompat.content.res.AppCompatResources
import androidx.appcompat.widget.AppCompatImageButton
import androidx.appcompat.widget.SwitchCompat
import androidx.core.view.ViewCompat
import com.google.android.material.button.MaterialButton
import dev.wingmate.mobile.R
import dev.wingmate.mobile.theme.dp
import dev.wingmate.mobile.theme.rounded

internal class SettingsUi(private val context: Context, theme: String) {
    val dark = when (theme) {
        "dark" -> true
        "light" -> false
        else -> context.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK ==
            Configuration.UI_MODE_NIGHT_YES
    }

    // Resolved DSH Web aliases from design-platform.css for the mobile settings panel.
    val panelBackground = color("#FFFFFF", "#2C2C2E") // bg-layer-2
    val primaryText = color("#0F1115", "#F9FAFB") // label-primary and brand-primary
    private val field = color("#F5F6F7", "#353638") // bg-module-platform
    private val secondary = color("#61666B", "#CFD3D6") // label-secondary
    private val tertiary = color("#81858C", "#ADB2B8") // label-tertiary
    private val divider = color("#1A000000", "#1FFFFFFF") // border-l2
    private val pressed = color("#1A263148", "#24FFFFFF") // interactive-bg-active
    private val actionFill = color("#E4EDFD", "#34415B") // state-business-tertiary
    private val actionInk = color("#4868B2", "#7AAAFF") // deepseek-600 / state-business-primary
    private val switchOff = color("#1F000000", "#29FFFFFF") // border-l3
    private val thumbOn = color("#FFFFFF", "#0F1115") // label-primary-foreground
    private val thumbOff = color("#FFFFFF", "#ADB2B8") // switch-thumb
    val danger = color("#EC1313", "#F25A5A") // state-error-primary
    val success = color("#16803D", "#22C55E") // Darken the DSH success color for light text contrast.

    fun text(value: String, size: Float, color: Int = primaryText, bold: Boolean = false) =
        TextView(context).apply {
            text = value
            textSize = size
            setTextColor(color)
            if (bold) setTypeface(typeface, Typeface.BOLD)
        }

    fun page(
        title: String,
        backLabel: String,
        back: () -> Unit,
        action: View?,
        content: LinearLayout.() -> Unit
    ) =
        ScrollView(context).apply {
            isFillViewport = true
            isFocusableInTouchMode = true
            setBackgroundColor(panelBackground)
            addView(column().apply {
                setPadding(dp(16), dp(12), dp(16), dp(28))
                addView(LinearLayout(context).apply {
                    addView(toolbarButton(R.drawable.ic_settings_back, backLabel, back), dp(48), dp(48))
                    if (action != null) {
                        addView(View(context), LinearLayout.LayoutParams(0, 0, 1f))
                        addView(action, dp(48), dp(48))
                    }
                }, -1, -2)
                addView(text(title, 28f, bold = true).apply {
                    ViewCompat.setAccessibilityHeading(this, true)
                }, LinearLayout.LayoutParams(-1, -2).apply {
                    setMargins(dp(8), dp(12), dp(8), dp(16))
                })
                content()
            })
        }

    fun scanButton(label: String, action: () -> Unit) = toolbarButton(R.drawable.ic_scan, label, action)

    fun actionButton(label: String, icon: Int? = null, primary: Boolean = false, action: () -> Unit) =
        MaterialButton(context).apply {
            val ink = if (primary) actionInk else primaryText
            text = label
            textSize = 15f
            isAllCaps = false
            setTextColor(ink)
            gravity = Gravity.CENTER
            minHeight = dp(48)
            minimumHeight = dp(48)
            stateListAnimator = null
            insetTop = 0
            insetBottom = 0
            cornerRadius = dp(12)
            backgroundTintList = ColorStateList.valueOf(if (primary) actionFill else field)
            rippleColor = ColorStateList.valueOf(pressed)
            setPaddingRelative(dp(12), dp(8), dp(12), dp(8))
            if (icon != null) {
                setIconResource(icon)
                iconSize = dp(20)
                iconPadding = dp(8)
                iconGravity = MaterialButton.ICON_GRAVITY_TEXT_START
                iconTint = ColorStateList.valueOf(ink)
            }
            setOnClickListener { action() }
        }

    private fun toolbarButton(icon: Int, label: String, action: () -> Unit) =
        AppCompatImageButton(context).apply {
            setImageResource(icon)
            imageTintList = ColorStateList.valueOf(primaryText)
            background = ripple(24)
            contentDescription = label
            setOnClickListener { action() }
        }

    fun LinearLayout.section(title: String? = null, content: LinearLayout.() -> Unit) {
        if (title != null) addView(text(title, 14f, secondary, bold = true).apply {
            ViewCompat.setAccessibilityHeading(this, true)
        }, LinearLayout.LayoutParams(-1, -2).apply { setMargins(dp(8), 0, dp(8), dp(8)) })
        addView(column().apply {
            dividerDrawable = GradientDrawable().apply {
                setColor(divider)
                setSize(0, 1)
            }
            dividerPadding = dp(16)
            showDividers = LinearLayout.SHOW_DIVIDER_MIDDLE
            content()
        }, LinearLayout.LayoutParams(-1, -2).apply { bottomMargin = dp(22) })
    }

    fun row(
        title: String,
        subtitle: String? = null,
        value: String? = null,
        icon: Int? = null,
        valueColor: Int = secondary,
        subtitleColor: Int = secondary,
        maxSubtitleLines: Int = 2,
        tone: Tone = Tone.HOSTS,
        trailing: View? = null,
        action: (() -> Unit)? = null
    ) = LinearLayout(context).apply {
        gravity = Gravity.CENTER_VERTICAL
        minimumHeight = dp(if (subtitle == null) 52 else 64)
        setPadding(dp(16), dp(8), dp(16), dp(8))
        if (icon != null) addView(ImageView(context).apply {
            setImageResource(icon)
            setColorFilter(color(tone.lightInk, tone.darkInk))
            background = context.rounded(color(tone.lightFill, tone.darkFill), 16)
            setPadding(dp(7), dp(7), dp(7), dp(7))
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        }, LinearLayout.LayoutParams(dp(32), dp(32)).apply { marginEnd = dp(12) })
        addView(column().apply {
            addView(text(title, 15f))
            if (subtitle != null) addView(text(subtitle, 13f, subtitleColor).apply {
                maxLines = maxSubtitleLines
                ellipsize = TextUtils.TruncateAt.END
            })
        }, LinearLayout.LayoutParams(0, -2, 1f))
        if (value != null) addView(text(value, 13f, valueColor).apply {
            maxWidth = dp(130)
            maxLines = 2
            ellipsize = TextUtils.TruncateAt.END
        }, LinearLayout.LayoutParams(-2, -2).apply { marginStart = dp(8) })
        if (trailing != null) addView(trailing, LinearLayout.LayoutParams(-2, -2).apply {
            marginStart = dp(8)
        })
        if (action != null) {
            if (trailing == null) addView(ImageView(context).apply {
                setImageResource(R.drawable.ic_settings_chevron)
                setColorFilter(tertiary)
                importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
            }, LinearLayout.LayoutParams(dp(16), dp(20)).apply { marginStart = dp(6) })
            background = ripple(12)
            isFocusable = true
            setOnClickListener { action() }
        }
    }

    fun toggle(title: String, enabled: Boolean, change: (Boolean) -> Unit) =
        SwitchCompat(context).apply {
            text = title
            textSize = 15f
            setTextColor(primaryText)
            minimumHeight = dp(52)
            setPadding(dp(16), dp(8), dp(16), dp(8))
            switchMinWidth = dp(48)
            thumbTintList = switchTint(thumbOn, thumbOff)
            trackTintList = switchTint(primaryText, switchOff)
            isChecked = enabled
            setOnCheckedChangeListener { _, value -> change(value) }
        }

    fun search(label: String) = EditText(context).apply {
        hint = label
        textSize = 16f
        setTextColor(primaryText)
        setHintTextColor(secondary)
        setPadding(dp(16), 0, dp(16), 0)
        minimumHeight = dp(46)
        isSingleLine = true
        background = context.rounded(field, 24)
        setCompoundDrawablesRelativeWithIntrinsicBounds(searchIcon(), null, null, null)
        compoundDrawablePadding = dp(8)
    }

    fun searchButton(label: String, action: () -> Unit) = text(label, 16f, secondary).apply {
        gravity = Gravity.CENTER_VERTICAL
        minimumHeight = dp(46)
        setPadding(dp(16), 0, dp(16), 0)
        background = ripple(24, field)
        setCompoundDrawablesRelativeWithIntrinsicBounds(searchIcon(), null, null, null)
        compoundDrawablePadding = dp(8)
        isFocusable = true
        setOnClickListener { action() }
    }

    private fun searchIcon() = AppCompatResources.getDrawable(context, R.drawable.ic_settings_search)
        ?.mutate()?.apply { setTint(secondary) }

    private fun column() = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
    private fun dp(value: Int) = context.dp(value)
    private fun color(light: String, dark: String) = Color.parseColor(if (this.dark) dark else light)
    private fun ripple(radius: Int, fill: Int = Color.TRANSPARENT) =
        RippleDrawable(ColorStateList.valueOf(pressed), context.rounded(fill, radius),
            context.rounded(Color.WHITE, radius))

    private fun switchTint(checked: Int, unchecked: Int) = ColorStateList(
        arrayOf(intArrayOf(android.R.attr.state_checked), intArrayOf()), intArrayOf(checked, unchecked)
    )

    enum class Tone(val lightFill: String, val lightInk: String, val darkFill: String, val darkInk: String) {
        GENERAL("#E6FAED", "#0F1115", "#233C2C", "#4ED17E"),
        HOSTS("#E4EDFD", "#4868B2", "#34415B", "#7AAAFF"),
        HELP("#DBEAFE", "#1E40AF", "#172554", "#93C5FD")
    }
}
