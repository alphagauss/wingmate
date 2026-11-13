package dev.wingmate.mobile.remote

import android.content.SharedPreferences
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.InsetDrawable
import android.view.Gravity
import android.view.MotionEvent
import android.view.ViewConfiguration
import android.widget.FrameLayout
import androidx.appcompat.widget.AppCompatImageButton
import dev.wingmate.mobile.R
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.theme.dp
import kotlin.math.abs

internal fun FrameLayout.addManagementButton(
    preferences: SharedPreferences,
    openManagement: () -> Unit
) {
    val button = AppCompatImageButton(context).apply {
        val inset = context.dp(5)
        background = InsetDrawable(GradientDrawable().apply {
            shape = GradientDrawable.OVAL
            setColor(Color.parseColor("#426FBB"))
        }, inset)
        setImageResource(R.drawable.ic_manage)
        val padding = context.dp(14)
        setPadding(padding, padding, padding, padding)
        alpha = .66f
        contentDescription = ConnectionStrings(context)["settings.title"]
        setOnClickListener { openManagement() }
    }
    val touchSize = context.dp(48)
    addView(button, FrameLayout.LayoutParams(touchSize, touchSize, Gravity.TOP or Gravity.LEFT))
    fun highlight(active: Boolean) {
        val scale = if (active) 1.16f else 1f
        button.animate().alpha(if (active) 1f else .66f).scaleX(scale).scaleY(scale)
            .setDuration(140).start()
    }

    val margin = context.dp(12)
    val touchSlop = ViewConfiguration.get(context).scaledTouchSlop
    var xFraction = preferences.getFloat("management_x", 1f)
    var yFraction = preferences.getFloat("management_y", .46f)

    fun horizontalInset() = minOf(margin, (width - button.width).coerceAtLeast(0) / 2)
    fun verticalInset() = minOf(margin, (height - button.height).coerceAtLeast(0) / 2)
    fun horizontalTravel() = (width - button.width - 2 * horizontalInset()).coerceAtLeast(0)
    fun verticalTravel() = (height - button.height - 2 * verticalInset()).coerceAtLeast(0)
    fun placeButton() {
        button.x = horizontalInset() + xFraction * horizontalTravel()
        button.y = verticalInset() + yFraction * verticalTravel()
    }
    addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ -> placeButton() }
    button.addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ -> placeButton() }

    var tracking = false
    var dragging = false
    var downX = 0f
    var downY = 0f
    var startX = 0f
    var startY = 0f
    button.setOnTouchListener { _, event ->
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                tracking = true
                dragging = false
                downX = event.rawX
                downY = event.rawY
                startX = button.x
                startY = button.y
                button.isPressed = true
                highlight(true)
            }
            MotionEvent.ACTION_MOVE -> if (tracking) {
                val dx = event.rawX - downX
                val dy = event.rawY - downY
                if (dragging || maxOf(abs(dx), abs(dy)) > touchSlop) {
                    dragging = true
                    button.isPressed = false
                    val left = horizontalInset().toFloat()
                    val top = verticalInset().toFloat()
                    button.x = (startX + dx).coerceIn(left, left + horizontalTravel())
                    button.y = (startY + dy).coerceIn(top, top + verticalTravel())
                }
            }
            MotionEvent.ACTION_UP -> if (tracking) {
                tracking = false
                button.isPressed = false
                highlight(false)
                if (dragging) {
                    val travelX = horizontalTravel()
                    val travelY = verticalTravel()
                    if (travelX > 0) xFraction = (button.x - horizontalInset()) / travelX
                    if (travelY > 0) yFraction = (button.y - verticalInset()) / travelY
                    preferences.edit().putFloat("management_x", xFraction).putFloat("management_y", yFraction).apply()
                } else {
                    button.performClick()
                }
            }
            MotionEvent.ACTION_CANCEL, MotionEvent.ACTION_POINTER_DOWN -> {
                tracking = false
                button.isPressed = false
                highlight(false)
                placeButton()
            }
        }
        true
    }
}
