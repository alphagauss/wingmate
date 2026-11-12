package dev.wingmate.mobile.theme

import android.content.Context
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.View
import android.widget.TextView
import androidx.appcompat.widget.AppCompatButton
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import kotlin.math.roundToInt

internal fun View.applyWindowInsets() {
    ViewCompat.setOnApplyWindowInsetsListener(this) { view, windowInsets ->
        val insets = windowInsets.getInsets(
            WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout() or
                WindowInsetsCompat.Type.ime()
        )
        view.setPadding(insets.left, insets.top, insets.right, insets.bottom)
        WindowInsetsCompat.CONSUMED
    }
    ViewCompat.requestApplyInsets(this)
}

internal fun Context.dp(value: Int): Int = (resources.displayMetrics.density * value).roundToInt()

internal fun Context.rounded(color: Int, radius: Int, stroke: Int? = null) = GradientDrawable().apply {
    setColor(color)
    cornerRadius = dp(radius).toFloat()
    if (stroke != null) setStroke(dp(2), stroke)
}

internal class ConnectionUi(private val context: Context, darkOverride: Boolean? = null) {
    private val dark = darkOverride ?: (context.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK ==
        Configuration.UI_MODE_NIGHT_YES)
    val background = Color.parseColor(if (dark) "#151517" else "#ffffff")
    val foreground = Color.parseColor(if (dark) "#f9fafb" else "#0f1115")
    val muted = Color.parseColor(if (dark) "#adb2b8" else "#81858c")
    val surface = Color.parseColor(if (dark) "#1b1b1c" else "#f9fafb")
    val backdrop = Color.parseColor(if (dark) "#80000000" else "#3d000000")
    val border = Color.parseColor(if (dark) "#1fffffff" else "#1a000000")
    val danger = Color.parseColor(if (dark) "#f87171" else "#dc2626")
    val onDanger = if (dark) Color.parseColor("#0f1115") else Color.WHITE

    fun text(value: String, size: Float, color: Int = foreground, bold: Boolean = false) =
        TextView(context).apply {
            text = value
            textSize = size
            setTextColor(color)
            if (bold) setTypeface(typeface, Typeface.BOLD)
        }

    fun button(label: String, primary: Boolean = false, action: () -> Unit) =
        AppCompatButton(context).apply {
            text = label
            textSize = 15f
            isAllCaps = false
            minimumHeight = context.dp(48)
            gravity = Gravity.CENTER
            backgroundTintList = null
            val palette = this@ConnectionUi
            background = context.rounded(if (primary) palette.foreground else Color.TRANSPARENT, 16)
            setTextColor(if (primary) palette.background else palette.foreground)
            setOnClickListener { action() }
        }
}
