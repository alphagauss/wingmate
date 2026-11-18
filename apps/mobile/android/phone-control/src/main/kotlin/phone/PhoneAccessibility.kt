package dev.wingmate.mobile.runtime.phone

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.GestureDescription
import android.app.KeyguardManager
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.graphics.Path
import android.graphics.Rect
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.os.SystemClock
import android.view.WindowInsets
import android.view.WindowManager
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction
import android.view.accessibility.AccessibilityWindowInfo
import dev.wingmate.mobile.phone.R
import dev.wingmate.mobile.runtime.phone.PhoneControl.Action
import java.util.UUID

// Adapted from dsh-Go's PhoneA11yService, MainScreenDriver and Actuator.
class PhoneAccessibility : AccessibilityService() {
    private val control
        get() = PhoneControl.get(this)

    private val screenOff =
        object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                control.invalidate(this@PhoneAccessibility)
            }
        }
    private var receiving = false
    private var windowVersion = 0L

    override fun onServiceConnected() {
        super.onServiceConnected()
        if (!receiving) {
            val filter = IntentFilter(Intent.ACTION_SCREEN_OFF)
            if (Build.VERSION.SDK_INT >= 33)
                registerReceiver(screenOff, filter, RECEIVER_NOT_EXPORTED)
            else registerReceiver(screenOff, filter)
            receiving = true
        }
        control.connect(this)
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        if (
            event?.eventType == AccessibilityEvent.TYPE_WINDOWS_CHANGED ||
                event?.eventType == AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED
        )
            windowVersion++
    }

    override fun onInterrupt() = Unit

    override fun onUnbind(intent: Intent?): Boolean {
        disconnect()
        return super.onUnbind(intent)
    }

    override fun onDestroy() {
        disconnect()
        super.onDestroy()
    }

    private fun disconnect() {
        control.disconnect(this)
        if (receiving) {
            receiving = false
            unregisterReceiver(screenOff)
        }
    }

    internal fun ready() =
        getSystemService(PowerManager::class.java).isInteractive &&
            !getSystemService(KeyguardManager::class.java).isKeyguardLocked

    private fun current(
        preview: Boolean = false,
        background: BackgroundPhone.Target? = null,
    ): AccessibilityNodeInfo {
        phoneCheck(ready(), "Unlock the phone and keep its screen on.")
        if (background != null) {
            if (Build.VERSION.SDK_INT < 34) throw PhoneProblem(BackgroundPolicy.REQUIREMENT)
            val window = windowsOnAllDisplays[background.displayId].orEmpty().firstOrNull()
            val root = window?.root
            phoneCheck(
                window?.type == AccessibilityWindowInfo.TYPE_APPLICATION &&
                    window.displayId == background.displayId &&
                    root?.packageName?.toString() == background.pkg,
                "The approved app is not the top window on the background screen.",
            )
            return checkNotNull(root)
        }
        val root = rootInActiveWindow ?: throw PhoneProblem("No active application window.")
        val pkg = root.packageName?.toString() ?: throw PhoneProblem("Unknown active application.")
        val window = root.window
        phoneCheck(
            window?.type == AccessibilityWindowInfo.TYPE_APPLICATION &&
                (Build.VERSION.SDK_INT < 30 ||
                    window.displayId == android.view.Display.DEFAULT_DISPLAY),
            "The active window is outside the phone control scope.",
        )
        phoneCheck(
            !PhoneSafety.blocked(pkg) || (preview && pkg == packageName),
            "This application is protected.",
        )
        return root
    }

    private fun launchables(): List<App> {
        val launcher = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
        return packageManager
            .queryIntentActivities(launcher, 0)
            .mapNotNull { item ->
                val info = item.activityInfo
                if (PhoneSafety.blocked(info.packageName) || info.packageName == packageName) null
                else
                    App(
                        ComponentName(info.packageName, info.name),
                        item.loadLabel(packageManager).toString(),
                    )
            }
            .distinctBy { it.component.packageName }
            .sortedBy { it.label.lowercase() }
    }

    internal fun apps(query: String) =
        launchables()
            .filter {
                it.label.contains(query, true) || it.component.packageName.contains(query, true)
            }
            .take(150)
            .joinToString("\n") { "${it.label} · ${it.component.packageName}" }
            .ifEmpty { "No matching apps." }

    internal data class App(val component: ComponentName, val label: String)

    internal fun launchable(name: String): App {
        val requested = name.trim()
        val apps = launchables()
        return apps.firstOrNull { it.component.packageName == requested }
            ?: apps.singleOrNull { it.label.equals(requested, true) }
            ?: throw PhoneProblem("No single launchable app matched. Use a package name from apps.")
    }

    internal fun read(
        detail: Boolean,
        preview: Boolean = false,
        background: BackgroundPhone.Target? = null,
    ): Observation {
        val root = current(preview, background)
        val pkg = root.packageName.toString()
        val targets = mutableListOf<Target>()
        val id = (if (background == null) "s" else "b") + UUID.randomUUID().toString()
        val text = StringBuilder("$id · $pkg")
        val deadline = SystemClock.uptimeMillis() + 250
        var visited = 0
        var truncated = false
        var protected = false

        fun walk(node: AccessibilityNodeInfo, path: List<Int>) {
            if (
                ++visited > 1200 ||
                    targets.size >= 120 ||
                    path.size > 40 ||
                    SystemClock.uptimeMillis() >= deadline
            ) {
                truncated = true
                return
            }
            if (!node.isVisibleToUser) return
            if (node.packageName?.toString() != pkg) {
                protected = true
                return
            }
            if (secret(node)) {
                protected = true
                text.append("\n[protected field]")
                return
            }
            if (
                sequenceOf(node.text, node.contentDescription, node.hintText).filterNotNull().any {
                    val value = it.toString()
                    ScreenMask.mask(value) != value
                }
            )
                protected = true
            val mark = Mark(node)
            if (
                mark.label.isNotBlank() ||
                    node.isClickable ||
                    node.isLongClickable ||
                    node.isEditable ||
                    node.isScrollable
            ) {
                val kind =
                    when {
                        node.isEditable -> "input"
                        node.isScrollable -> "list"
                        node.isClickable || node.isLongClickable -> "button"
                        else -> "text"
                    }
                targets += Target(path, mark, node.isScrollable)
                text.append("\n[${targets.size}] $kind ${ScreenMask.mask(mark.label).take(160)}")
                if (detail) text.append(" ${mark.bounds.toShortString()}")
            }
            for (index in 0 until node.childCount) {
                if (
                    targets.size >= 120 || visited >= 1200 || SystemClock.uptimeMillis() >= deadline
                ) {
                    truncated = true
                    break
                }
                val child = node.getChild(index)
                if (child == null) protected = true else walk(child, path + index)
            }
        }
        walk(root, emptyList())
        if (truncated) text.append("\n[screen output truncated]")
        return Observation(
            id,
            pkg,
            root.windowId,
            targets,
            text.toString(),
            !truncated && !protected,
            background = background,
        )
    }

    internal data class Observation(
        val id: String,
        val pkg: String,
        val windowId: Int,
        val targets: List<Target>,
        val text: String,
        val imageSafe: Boolean,
        val frame: PhoneScreenshot.Frame? = null,
        val background: BackgroundPhone.Target? = null,
    )

    internal data class Target(val path: List<Int>, val mark: Mark, val scrollable: Boolean)

    internal data class Mark(
        val clazz: String?,
        val viewId: String?,
        val bounds: Rect,
        val label: String,
    ) {
        constructor(
            node: AccessibilityNodeInfo,
        ) : this(
            node.className?.toString(),
            node.viewIdResourceName,
            Rect().also(node::getBoundsInScreen),
            sequenceOf(node.text, node.contentDescription, node.hintText)
                .filterNotNull()
                .firstOrNull { it.isNotBlank() }
                ?.take(512)
                ?.toString()
                ?.replace('\n', ' ')
                .orEmpty(),
        )
    }

    internal fun screenshot(
        observation: Observation,
        complete: (Result<PhoneScreenshot.Frame>) -> Unit,
    ) {
        val scope = runCatching {
            screenshotBounds(observation)
        }
            .getOrElse {
                return complete(Result.failure(it))
            }
        val version = windowVersion
        val isolated = observation.background != null
        PhoneScreenshot.capture(this, scope, observation.windowId.takeIf { isolated }) { result ->
            complete(
                result.mapCatching { frame ->
                    phoneCheck(
                        (isolated || windowVersion == version) &&
                            screenshotBounds(observation) == frame.bounds,
                        "The window changed during capture.",
                    )
                    frame
                },
            )
        }
    }

    private fun screenshotBounds(observation: Observation): Rect {
        phoneCheck(Build.VERSION.SDK_INT >= 30, "Screenshots require Android 11 or later.")
        phoneCheck(
            observation.imageSafe,
            "The screen contains protected content or its element tree is incomplete.",
        )
        val background = observation.background
        val root = sameWindow(observation.pkg, observation.windowId, background)
        val fresh = read(detail = false, background = background)
        phoneCheck(
            fresh.imageSafe &&
                fresh.pkg == observation.pkg &&
                fresh.windowId == observation.windowId,
            "The screen contains protected content or the window changed.",
        )
        val window = root.window ?: throw PhoneProblem("No app window is available for capture.")
        val bounds = Rect().also(window::getBoundsInScreen)
        if (background != null) {
            phoneCheck(
                bounds.left >= 0 && bounds.top >= 0 && !bounds.isEmpty,
                "The background window has invalid capture bounds.",
            )
            return bounds
        }
        val content = getSystemService(WindowManager::class.java).maximumWindowMetrics
        val inset =
            content.windowInsets.getInsetsIgnoringVisibility(
                WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout(),
            )
        val display =
            Rect(content.bounds).apply { inset(inset.left, inset.top, inset.right, inset.bottom) }
        val visibleWindows = windows
        // OEM system bars can extend beyond the insets reported to a service context.
        visibleWindows
            .filter { it.type == AccessibilityWindowInfo.TYPE_SYSTEM }
            .forEach { bar ->
                val rect = Rect().also(bar::getBoundsInScreen)
                val physical = content.bounds
                if (
                    rect.left <= physical.left &&
                        rect.right >= physical.right &&
                        rect.height() in 1..(physical.height() / 8)
                ) {
                    if (rect.top <= physical.top) display.top = maxOf(display.top, rect.bottom)
                    if (rect.bottom >= physical.bottom)
                        display.bottom = minOf(display.bottom, rect.top)
                }
            }
        phoneCheck(bounds.intersect(display), "The active window has no visible capture area.")
        phoneCheck(
            visibleWindows.any { it.id == window.id } &&
                visibleWindows.none { other ->
                    other.id != window.id &&
                        (other.layer >= window.layer ||
                            other.type == AccessibilityWindowInfo.TYPE_APPLICATION) &&
                        Rect.intersects(bounds, Rect().also(other::getBoundsInScreen))
                },
            "Another window overlaps the app. Dismiss it before taking a screenshot.",
        )
        return bounds
    }

    internal fun prepare(
        call: McpServer.Call,
        observation: Observation?,
        background: BackgroundPhone.Target? = null,
    ): Action {
        val args = call.args
        phoneCheck(
            observation == null || observation.background === background,
            "Read the correct screen before acting.",
        )
        if (call.tool == "tap_xy") return prepareCoordinate(args, observation)
        if (call.tool == "open") {
            val app = launchable(args["app"] as String)
            return Action(getString(R.string.phone_control_open, app.label)) {
                val intent =
                    Intent(Intent.ACTION_MAIN)
                        .addCategory(Intent.CATEGORY_LAUNCHER)
                        .setComponent(app.component)
                        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                startActivity(intent)
                true
            }
        }

        val root = current(background = background)
        val screen = args["screen"]
        if (screen != null)
            phoneCheck(
                observation != null &&
                    screen == observation.id &&
                    root.windowId == observation.windowId &&
                    root.packageName.toString() == observation.pkg,
                "The phone screen changed. Read it again before acting.",
            )
        val pkg = root.packageName.toString()
        val windowId = root.windowId
        if (call.tool == "key") {
            return Action(getString(R.string.phone_control_back, pkg)) {
                sameWindow(pkg, windowId)
                performGlobalAction(GLOBAL_ACTION_BACK)
            }
        }

        val id = (args["id"] as? Long)?.toInt()
        val target =
            if (call.tool == "scroll" && id == null) {
                observation?.targets?.firstOrNull(Target::scrollable)
                    ?: throw PhoneProblem("No scrollable list found. Read the screen again.")
            } else {
                id?.let { observation?.targets?.getOrNull(it - 1) }
                    ?: throw PhoneProblem("Use an element number from the latest screen.")
            }
        val node = resolve(root, target)
        val selected =
            when (call.tool) {
                "tap" -> clickTarget(node, args["long"] == true)
                "scroll" -> parentTarget(node) { it.isScrollable }
                else ->
                    node.takeIf { it.isEditable && it.has(AccessibilityAction.ACTION_SET_TEXT.id) }
            } ?: throw PhoneProblem("That element does not support this action.")
        phoneCheck(safeNode(selected, pkg), "This element is protected.")
        val mark = Mark(selected)
        val label =
            ScreenMask.mask(mark.label).take(120).ifBlank {
                mark.clazz?.substringAfterLast('.') ?: pkg
            }
        val summary =
            when (call.tool) {
                "tap" ->
                    getString(
                        if (args["long"] == true) R.string.phone_control_long_press
                        else R.string.phone_control_tap,
                        label,
                        pkg,
                    )
                "type" -> getString(R.string.phone_control_type, label, pkg, args["text"])
                else -> getString(R.string.phone_control_scroll, args["direction"], pkg)
            }
        return Action(summary) {
            val freshNode = resolve(sameWindow(pkg, windowId, background), target)
            val fresh =
                when (call.tool) {
                    "tap" -> clickTarget(freshNode, args["long"] == true)
                    "scroll" -> parentTarget(freshNode) { it.isScrollable }
                    else -> freshNode
                }
            phoneCheck(
                fresh != null && safeNode(fresh, pkg) && Mark(fresh) == mark,
                "The target changed while waiting for approval. Read the screen again.",
            )
            checkNotNull(fresh)
            when (call.tool) {
                "tap" ->
                    fresh.performAction(
                        if (args["long"] == true) AccessibilityNodeInfo.ACTION_LONG_CLICK
                        else AccessibilityNodeInfo.ACTION_CLICK,
                    )
                "scroll" -> scroll(fresh, args["direction"] as String)
                else ->
                    fresh.performAction(
                        AccessibilityNodeInfo.ACTION_SET_TEXT,
                        Bundle().apply {
                            putCharSequence(
                                AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE,
                                args["text"] as String,
                            )
                        },
                    )
            }
        }
    }

    private fun prepareCoordinate(args: Map<*, *>, observation: Observation?): Action {
        val observed =
            observation?.takeIf { it.id == args["observation"] }
                ?: throw PhoneProblem(
                    "Request a fresh screenshot with screen(image: true) before tapping a coordinate. Include background: true for the background screen.",
                )
        val frame =
            observed.frame
                ?: throw PhoneProblem(
                    "This observation did not return a screenshot. Request screen image=true.",
                )
        val x = (args["x"] as Long).toInt()
        val y = (args["y"] as Long).toInt()
        fun currentFrame() {
            phoneCheck(
                SystemClock.elapsedRealtime() - frame.capturedAt <= 30_000,
                "The screenshot expired. Read the screen again.",
            )
            phoneCheck(
                screenshotBounds(observed) == frame.bounds,
                "The app window changed. Read the screen again.",
            )
        }
        currentFrame()
        phoneCheck(
            x in 0 until frame.width && y in 0 until frame.height,
            "Coordinates are outside the returned screenshot.",
        )
        val summary = getString(R.string.phone_control_tap_coordinates, x, y, observed.pkg)
        return Action(summary) { live, complete ->
            // Let the confirmation overlay leave the compositor before rechecking the frame.
            Handler(Looper.getMainLooper())
                .postDelayed(
                    {
                        if (!live()) return@postDelayed
                        val check = runCatching { currentFrame() }
                        if (check.isFailure) {
                            complete(Result.failure(check.exceptionOrNull()!!))
                            return@postDelayed
                        }
                        screenshot(observed) { result ->
                            if (!live()) return@screenshot
                            result
                                .getOrElse {
                                    complete(Result.failure(it))
                                    return@screenshot
                                }
                                .let { fresh ->
                                    if (
                                        fresh.bounds != frame.bounds || fresh.digest != frame.digest
                                    ) {
                                        complete(
                                            Result.failure(
                                                PhoneProblem(
                                                    "The screenshot changed while awaiting approval. Read the screen again before tapping.",
                                                ),
                                            ),
                                        )
                                        return@screenshot
                                    }
                                }
                            val px =
                                frame.bounds.left + (x + 0.5f) * frame.bounds.width() / frame.width
                            val py =
                                frame.bounds.top + (y + 0.5f) * frame.bounds.height() / frame.height
                            tapPosition(px, py, observed.background?.displayId, complete)
                        }
                    },
                    220,
                )
        }
    }

    private fun tapPosition(
        x: Float,
        y: Float,
        displayId: Int?,
        complete: (Result<Boolean>) -> Unit,
    ) {
        val gesture =
            GestureDescription.Builder()
                .apply {
                    if (displayId != null) {
                        if (Build.VERSION.SDK_INT < 34)
                            throw PhoneProblem(BackgroundPolicy.REQUIREMENT)
                        setDisplayId(displayId)
                    }
                }
                .addStroke(
                    GestureDescription.StrokeDescription(Path().apply { moveTo(x, y) }, 0, 80),
                )
                .build()
        val main = Handler(Looper.getMainLooper())
        val timeout = Runnable {
            complete(
                Result.failure(
                    PhoneProblem(
                        "Android did not confirm gesture completion. Read the screen again.",
                    ),
                ),
            )
        }
        val sent =
            dispatchGesture(
                gesture,
                object : GestureResultCallback() {
                    override fun onCompleted(gestureDescription: GestureDescription?) {
                        main.removeCallbacks(timeout)
                        complete(Result.success(true))
                    }

                    override fun onCancelled(gestureDescription: GestureDescription?) {
                        main.removeCallbacks(timeout)
                        complete(
                            Result.failure(
                                PhoneProblem(
                                    "Android cancelled the gesture. Read the screen again.",
                                ),
                            ),
                        )
                    }
                },
                main,
            )
        if (sent) main.postDelayed(timeout, 2000) else complete(Result.success(false))
    }

    private fun sameWindow(pkg: String, windowId: Int, background: BackgroundPhone.Target? = null) =
        current(background = background).also {
            phoneCheck(
                it.windowId == windowId && it.packageName?.toString() == pkg,
                "The application changed while waiting for approval.",
            )
        }

    private fun resolve(root: AccessibilityNodeInfo, target: Target): AccessibilityNodeInfo {
        var node = root
        val pkg = root.packageName.toString()
        for (index in target.path) {
            node =
                node.getChild(index)
                    ?: throw PhoneProblem("The element moved. Read the screen again.")
            phoneCheck(safeNode(node, pkg), "This element is protected or no longer available.")
        }
        phoneCheck(
            Mark(node) == target.mark && safeNode(node, pkg),
            "The element changed. Read the screen again.",
        )
        return node
    }

    private fun secret(node: AccessibilityNodeInfo) =
        node.isPassword ||
            PhoneSafety.secretHit(
                node.hintText?.toString(),
                node.contentDescription?.toString(),
                node.viewIdResourceName,
            ) != null ||
            PhoneSafety.secretHit(node.text?.take(512)?.toString(), null, null) != null

    private fun safeNode(node: AccessibilityNodeInfo, pkg: String) =
        node.isVisibleToUser &&
            node.isEnabled &&
            node.packageName?.toString() == pkg &&
            !secret(node)

    private fun parentTarget(
        node: AccessibilityNodeInfo,
        accepts: (AccessibilityNodeInfo) -> Boolean,
    ) = generateSequence(node) { it.parent }.take(5).firstOrNull(accepts)

    private fun clickTarget(node: AccessibilityNodeInfo, long: Boolean) =
        parentTarget(node) { if (long) it.isLongClickable else it.isClickable }

    private fun AccessibilityNodeInfo.has(action: Int) = actionList.any { it.id == action }

    private fun scroll(node: AccessibilityNodeInfo, direction: String): Boolean {
        val action =
            when (direction) {
                "up" -> AccessibilityAction.ACTION_SCROLL_UP
                "down" -> AccessibilityAction.ACTION_SCROLL_DOWN
                "left" -> AccessibilityAction.ACTION_SCROLL_LEFT
                else -> AccessibilityAction.ACTION_SCROLL_RIGHT
            }.id
        return when {
            node.has(action) -> node.performAction(action)
            direction == "up" -> node.performAction(AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD)
            direction == "down" -> node.performAction(AccessibilityNodeInfo.ACTION_SCROLL_FORWARD)
            else -> false
        }
    }
}

class PhoneProblem(message: String) : IllegalStateException(message)

fun phoneCheck(ok: Boolean, message: String) {
    if (!ok) throw PhoneProblem(message)
}
