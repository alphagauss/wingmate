package dev.wingmate.mobile.web

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.os.SystemClock
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import dev.wingmate.mobile.R
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.theme.ConnectionUi
import dev.wingmate.mobile.theme.dp
import kotlin.math.PI
import kotlin.math.cos

internal class WorkbenchLoading(
    context: Context,
    private val retry: () -> Unit,
    private val connections: () -> Unit
) : FrameLayout(context) {
    private var ui = ConnectionUi(context)
    private val labels = ConnectionStrings(context)
    private val body = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        gravity = Gravity.CENTER
        setPadding(context.dp(32), context.dp(16), context.dp(32), context.dp(16))
    }
    private val timeout = Runnable {
        error(labels["connection.loadingSlow"])
    }

    init {
        addView(body, LayoutParams(-1, -2, Gravity.CENTER))
        isClickable = true
        visibility = GONE
    }

    fun start(engineStatus: String? = null) {
        removeCallbacks(timeout)
        show()
        body.addView(ImageView(context).apply {
            setImageResource(R.drawable.ic_wingmate_foreground)
            setColorFilter(ui.foreground)
            // Crop the empty padding around the adaptive icon's whale.
            scaleType = ImageView.ScaleType.CENTER_CROP
            importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
        }, context.dp(172), context.dp(76))
        body.addView(ui.text("Wingmate", 24f, bold = true).apply {
            gravity = Gravity.CENTER
            includeFontPadding = false
        }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = context.dp(6) })
        body.addView(BreathingDots(context, ui.foreground).apply {
            contentDescription = labels["connection.loading"].takeIf { engineStatus == null }
            importantForAccessibility = if (engineStatus == null) IMPORTANT_FOR_ACCESSIBILITY_YES
                else IMPORTANT_FOR_ACCESSIBILITY_NO
        }, LinearLayout.LayoutParams(
            context.dp(44), context.dp(24)
        ).apply { topMargin = context.dp(18) })
        if (engineStatus == null) {
            postDelayed(timeout, 20_000)
        } else {
            body.addView(ui.text(engineStatus, 14f).apply { gravity = Gravity.CENTER },
                LinearLayout.LayoutParams(-1, -2).apply { topMargin = context.dp(8) })
        }
    }

    fun stop() {
        removeCallbacks(timeout)
        visibility = GONE
    }

    fun applyTheme(theme: String) {
        ui = ConnectionUi(context, when (theme) { "dark" -> true; "light" -> false; else -> null })
        setBackgroundColor(ui.background)
    }

    fun error(message: String) {
        removeCallbacks(timeout)
        show()
        body.addView(ui.text(message, 16f).apply { gravity = Gravity.CENTER })
        body.addView(ui.button(labels["connection.reconnect"], primary = true, action = retry), LinearLayout.LayoutParams(
            -1, -2
        ).apply { topMargin = context.dp(20) })
        body.addView(ui.button(labels["settings.title"], action = connections))
    }

    private fun show() {
        setBackgroundColor(ui.background)
        body.removeAllViews()
        visibility = VISIBLE
    }

    override fun onDetachedFromWindow() {
        removeCallbacks(timeout)
        super.onDetachedFromWindow()
    }
}

private class BreathingDots(context: Context, color: Int) : View(context) {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { this.color = color }
    private val start = SystemClock.uptimeMillis()

    override fun onDraw(canvas: Canvas) {
        val elapsed = SystemClock.uptimeMillis() - start
        repeat(3) { index ->
            val phase = ((elapsed - index * 150).coerceAtLeast(0) % 900) / 900.0
            val pulse = ((1 - cos(2 * PI * phase)) * .5).toFloat()
            paint.alpha = (255 * (.25f + .75f * pulse)).toInt()
            canvas.drawCircle(context.dp(4 + index * 18).toFloat(),
                height / 2f - context.dp(5) * pulse, context.dp(4).toFloat(), paint)
        }
        postInvalidateOnAnimation()
    }
}
