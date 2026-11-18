package dev.wingmate.mobile.runtime.phone

import android.accessibilityservice.AccessibilityService
import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.Rect
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Base64
import android.view.Display
import java.io.ByteArrayOutputStream
import java.security.MessageDigest
import java.util.concurrent.Executors
import kotlin.math.roundToInt

// Accessibility screenshot and bounded JPEG encoding adapted from dsh-Go's ScreenShooter.
internal object PhoneScreenshot {
    data class Frame(
        val jpeg: String,
        val digest: String,
        val bounds: Rect,
        val width: Int,
        val height: Int,
        val capturedAt: Long,
    )

    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor()

    fun capture(
        service: AccessibilityService,
        bounds: Rect,
        windowId: Int?,
        complete: (Result<Frame>) -> Unit,
    ) {
        if (Build.VERSION.SDK_INT < 30) {
            complete(Result.failure(PhoneProblem("Screenshots require Android 11 or later.")))
            return
        }
        var finished = false
        val deadline = Runnable {
            finished = true
            complete(Result.failure(PhoneProblem("Android did not return a screenshot in time.")))
        }
        fun deliver(result: Result<Frame>) {
            main.post {
                if (finished) return@post
                finished = true
                main.removeCallbacks(deadline)
                complete(result)
            }
        }
        main.postDelayed(deadline, 3500)
        try {
            val callback =
                object : AccessibilityService.TakeScreenshotCallback {
                    override fun onSuccess(screenshot: AccessibilityService.ScreenshotResult) {
                        val result = runCatching {
                            screenshot.hardwareBuffer.use { buffer ->
                                val bitmap =
                                    Bitmap.wrapHardwareBuffer(buffer, screenshot.colorSpace)
                                        ?: throw PhoneProblem(
                                            "Android returned an unusable screenshot.",
                                        )
                                try {
                                    encode(bitmap, bounds, windowId != null)
                                } finally {
                                    bitmap.recycle()
                                }
                            }
                        }
                        deliver(result)
                    }

                    override fun onFailure(errorCode: Int) {
                        deliver(
                            Result.failure(
                                PhoneProblem("Android refused the screenshot (code $errorCode)."),
                            ),
                        )
                    }
                }
            if (windowId == null) {
                service.takeScreenshot(Display.DEFAULT_DISPLAY, worker, callback)
            } else if (Build.VERSION.SDK_INT >= 34) {
                service.takeScreenshotOfWindow(windowId, worker, callback)
            } else {
                deliver(Result.failure(PhoneProblem(BackgroundPolicy.REQUIREMENT)))
            }
        } catch (error: Exception) {
            deliver(Result.failure(error))
        }
    }

    private fun encode(bitmap: Bitmap, bounds: Rect, windowOnly: Boolean): Frame {
        val source = if (windowOnly) Rect(0, 0, bounds.width(), bounds.height()) else bounds
        phoneCheck(
            if (windowOnly) bitmap.width == source.width() && bitmap.height == source.height()
            else Rect(0, 0, bitmap.width, bitmap.height).contains(source),
            "The screen dimensions changed.",
        )
        val factor = minOf(1f, 1200f / maxOf(bounds.width(), bounds.height()))
        val width = (bounds.width() * factor).roundToInt().coerceAtLeast(1)
        val height = (bounds.height() * factor).roundToInt().coerceAtLeast(1)
        val cropped =
            Bitmap.createBitmap(bitmap, source.left, source.top, source.width(), source.height())
        val scaled = Bitmap.createScaledBitmap(cropped, width, height, true)
        val pixels =
            scaled.copy(Bitmap.Config.ARGB_8888, false)
                ?: throw PhoneProblem("Android could not encode the screenshot.")
        try {
            val sha = MessageDigest.getInstance("SHA-256")
            val row = IntArray(width)
            val bytes = ByteArray(width * 3)
            var visible = false
            for (y in 0 until height) {
                pixels.getPixels(row, 0, width, 0, y, width, 1)
                for ((x, argb) in row.withIndex()) {
                    val red = Color.red(argb)
                    val green = Color.green(argb)
                    val blue = Color.blue(argb)
                    visible = visible || red > 8 || green > 8 || blue > 8
                    bytes[3 * x] = red.toByte()
                    bytes[3 * x + 1] = green.toByte()
                    bytes[3 * x + 2] = blue.toByte()
                }
                sha.update(bytes)
            }
            phoneCheck(
                visible,
                "The app returned a black frame. Use the available text or continue on the phone.",
            )
            val out = ByteArrayOutputStream()
            phoneCheck(
                pixels.compress(Bitmap.CompressFormat.JPEG, 78, out) && out.size() <= 512 * 1024,
                "Android could not encode a bounded screenshot.",
            )
            return Frame(
                Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP),
                Base64.encodeToString(sha.digest(), Base64.NO_WRAP),
                bounds,
                width,
                height,
                SystemClock.elapsedRealtime(),
            )
        } finally {
            pixels.recycle()
            if (scaled !== cropped) scaled.recycle()
            if (cropped !== bitmap) cropped.recycle()
        }
    }
}
