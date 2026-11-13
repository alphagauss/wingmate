package dev.wingmate.mobile.settings

import android.animation.ValueAnimator
import android.content.Context
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.animation.DecelerateInterpolator
import android.view.inputmethod.InputMethodManager
import android.widget.FrameLayout
import kotlin.math.abs
import kotlin.math.roundToLong

internal class SettingsOverlay(
    context: Context,
    private val workbench: View,
    private val back: () -> Unit
) : FrameLayout(context) {
    val page = FrameLayout(context).apply {
        visibility = GONE
        isClickable = true
    }
    private val slop = ViewConfiguration.get(context).scaledTouchSlop
    private var progress = 1f
    private var animator: ValueAnimator? = null
    private var downX = 0f
    private var downY = 0f
    private var dragging = false
    private var scrolling = false
    var closing = false
        private set
    val showing: Boolean get() = page.visibility == VISIBLE

    init {
        addView(workbench, LayoutParams(-1, -1))
        addView(page, LayoutParams(-1, -1))
    }

    fun open() {
        if (showing) return
        context.getSystemService(InputMethodManager::class.java)
            .hideSoftInputFromWindow(windowToken, 0)
        workbench.importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        translate(1f)
        page.visibility = VISIBLE
        page.getChildAt(0).requestFocus()
        slideTo(0f)
    }

    fun close() {
        if (closing || !showing) return
        context.getSystemService(InputMethodManager::class.java)
            .hideSoftInputFromWindow(windowToken, 0)
        closing = true
        slideTo(1f)
    }

    fun resetSwipe() {
        if (progress != 0f) slideTo(0f)
    }

    private fun slideTo(target: Float) {
        animator?.cancel()
        animator = ValueAnimator.ofFloat(progress, target).apply {
            duration = (270 * abs(target - progress)).roundToLong().coerceAtLeast(100)
            interpolator = DecelerateInterpolator()
            addUpdateListener {
                translate(it.animatedValue as Float)
                if (progress == 1f && target == 1f) {
                    page.visibility = GONE
                    workbench.importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_AUTO
                    closing = false
                }
            }
            start()
        }
    }

    private fun translate(position: Float) {
        progress = position
        page.translationX = width * position
        workbench.translationX = -width * (1f - position)
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        translate(progress)
    }

    override fun onInterceptTouchEvent(event: MotionEvent): Boolean {
        if (!showing) return false
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                if (animator?.isRunning == true) return true
                downX = event.x
                downY = event.y
                dragging = false
                scrolling = false
            }
            MotionEvent.ACTION_MOVE -> {
                val dx = event.x - downX
                val dy = abs(event.y - downY)
                if (dy > slop && dy >= abs(dx)) scrolling = true
                dragging = !scrolling && dx > slop && dx > dy
                if (dragging) translate((dx / width).coerceIn(0f, 1f))
            }
        }
        return dragging
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (!dragging) return showing
        when (event.actionMasked) {
            MotionEvent.ACTION_MOVE -> translate(((event.x - downX) / width).coerceIn(0f, 1f))
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                dragging = false
                if (event.actionMasked == MotionEvent.ACTION_UP && progress >= .25f) back()
                else resetSwipe()
            }
        }
        return true
    }

    override fun onDetachedFromWindow() {
        animator?.cancel()
        super.onDetachedFromWindow()
    }
}
