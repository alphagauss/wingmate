package dev.wingmate.mobile.runtime.phone

import android.graphics.PixelFormat
import android.view.ContextThemeWrapper
import android.view.Gravity
import android.view.MotionEvent
import android.view.WindowManager
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import dev.wingmate.mobile.phone.R

// Native confirmation on the target screen, following dsh-Go's accessibility overlay.
internal object PhoneConfirmation {
    fun show(service: PhoneAccessibility, summary: String, decide: (Boolean) -> Unit): () -> Unit {
        val wm = service.getSystemService(WindowManager::class.java)
        val theme = ContextThemeWrapper(service, android.R.style.Theme_DeviceDefault_Dialog)
        val density = service.resources.displayMetrics.density
        fun dp(value: Int) = (value * density).toInt()
        val panel =
            LinearLayout(theme).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(dp(16), dp(12), dp(16), dp(8))
                setBackgroundResource(android.R.drawable.dialog_holo_light_frame)
            }
        val title =
            TextView(theme).apply {
                setText(R.string.phone_control_confirmation)
                textSize = 19f
                setTextColor(android.graphics.Color.BLACK)
            }
        val body =
            TextView(theme).apply {
                text = summary
                textSize = 16f
                setTextColor(android.graphics.Color.BLACK)
            }
        val scroll = ScrollView(theme).apply { addView(body) }
        val buttons =
            LinearLayout(theme).apply {
                gravity = Gravity.END
                fun button(label: Int, allowed: Boolean) =
                    Button(theme).apply {
                        setText(label)
                        filterTouchesWhenObscured = true
                        setOnTouchListener { _, event ->
                            event.flags and
                                (MotionEvent.FLAG_WINDOW_IS_OBSCURED or
                                    MotionEvent.FLAG_WINDOW_IS_PARTIALLY_OBSCURED) != 0
                        }
                        setOnClickListener { decide(allowed) }
                    }
                addView(button(android.R.string.cancel, false))
                addView(button(R.string.phone_control_allow_once, true))
            }
        panel.addView(title)
        panel.addView(
            scroll,
            LinearLayout.LayoutParams(-1, -2).apply {
                topMargin = dp(8)
                bottomMargin = dp(8)
                height =
                    minOf(
                        dp(180),
                        dp(64) + dp(18) * (summary.length / 35 + summary.count { it == '\n' }),
                    )
            },
        )
        panel.addView(buttons)
        val params =
            WindowManager.LayoutParams(
                    minOf(dp(480), service.resources.displayMetrics.widthPixels - dp(16)),
                    WindowManager.LayoutParams.WRAP_CONTENT,
                    WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY,
                    WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
                        WindowManager.LayoutParams.FLAG_SECURE,
                    PixelFormat.TRANSLUCENT,
                )
                .apply {
                    gravity = Gravity.TOP or Gravity.CENTER_HORIZONTAL
                    y = dp(12)
                    setTitle(service.getString(R.string.phone_control_confirmation))
                }
        wm.addView(panel, params)
        return {
            runCatching { wm.removeViewImmediate(panel) }
            Unit
        }
    }
}
