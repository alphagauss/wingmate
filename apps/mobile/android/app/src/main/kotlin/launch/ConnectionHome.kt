package dev.wingmate.mobile.launch

import android.content.res.ColorStateList
import android.graphics.Color
import android.graphics.drawable.RippleDrawable
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import dev.wingmate.mobile.R
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.remote.ConnectionFlow
import dev.wingmate.mobile.remote.ConnectionTarget
import dev.wingmate.mobile.remote.NoLanNetwork
import dev.wingmate.mobile.remote.discoverConnection
import dev.wingmate.mobile.settings.LocalEngineSettings
import dev.wingmate.mobile.theme.ConnectionUi
import dev.wingmate.mobile.theme.dp
import dev.wingmate.mobile.theme.rounded

internal class ConnectionHome(
    private val activity: AppCompatActivity,
    private val connections: ConnectionFlow,
    private val localEngine: LocalEngineSettings?,
    private val openLocalSettings: () -> Unit,
    private val openHosts: () -> Unit,
    private val openSettings: () -> Unit
) {
    val view = ScrollView(activity).apply { isFillViewport = true }
    private val body = LinearLayout(activity).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(activity.dp(24), activity.dp(16), activity.dp(24), activity.dp(24))
    }
    private val labels = ConnectionStrings(activity)
    private var notice: String? = null
    private var progress: String? = null
    private var discovery: Thread? = null

    init {
        view.addView(body, FrameLayout.LayoutParams(-1, -2))
        render()
    }

    fun render() {
        val ui = ConnectionUi(activity)
        val monochrome = if (ui.dark) Color.WHITE else Color.BLACK
        view.setBackgroundColor(ui.background)
        body.removeAllViews()
        val heading = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            minimumHeight = activity.dp(192)
            addView(ImageView(activity).apply {
                setImageResource(R.drawable.ic_wingmate_foreground)
                scaleType = ImageView.ScaleType.CENTER_CROP
                imageTintList = ColorStateList.valueOf(monochrome)
                importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
            }, activity.dp(152), activity.dp(88))
            addView(ui.text(labels["app.name"], 28f, bold = true).apply {
                gravity = Gravity.CENTER
                includeFontPadding = false
                ViewCompat.setAccessibilityHeading(this, true)
            }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = activity.dp(8) })
            progress?.let { key ->
                addView(ProgressBar(activity).apply {
                    indeterminateTintList = ColorStateList.valueOf(monochrome)
                    importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
                }, LinearLayout.LayoutParams(activity.dp(30), activity.dp(30)).apply {
                    topMargin = activity.dp(18)
                })
                addView(ui.text(labels[key], 15f, ui.muted).apply {
                    gravity = Gravity.CENTER
                    accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
                }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = activity.dp(12) })
                if (discovery != null) addView(ui.button(labels["common.cancel"]) { cancelDiscovery() })
            }
        }
        body.addView(heading, LinearLayout.LayoutParams(-1, -2, 1f))
        if (progress != null) return

        notice?.let { message ->
            body.addView(ui.text(message, 14f, ui.danger).apply {
                gravity = Gravity.CENTER
                accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
            })
        }
        fun action(key: String, icon: Int, run: () -> Unit) {
            val button = ui.button(labels[key], action = run).apply {
                stateListAnimator = null
                minHeight = activity.dp(54)
                minimumHeight = activity.dp(54)
                val ripple = (ui.foreground and 0x00FFFFFF) or 0x24000000
                background = RippleDrawable(ColorStateList.valueOf(ripple),
                    activity.rounded(ui.surface, 14), null)
                gravity = Gravity.CENTER_VERTICAL or Gravity.START
                setPaddingRelative(activity.dp(18), 0, activity.dp(18), 0)
                setCompoundDrawablesRelativeWithIntrinsicBounds(icon, 0, 0, 0)
                compoundDrawablePadding = activity.dp(12)
                compoundDrawableTintList = ColorStateList.valueOf(ui.foreground)
            }
            body.addView(button, LinearLayout.LayoutParams(-1, -2).apply { topMargin = activity.dp(8) })
        }
        localEngine?.let { engine ->
            val openWorkbench = engine.openWorkbench
            action(if (openWorkbench == null) "localAgent.title" else "localAgent.workbench",
                R.drawable.ic_settings_engine, run = openWorkbench ?: openLocalSettings)
        }
        action("connection.autoScan", R.drawable.ic_settings_search) { discover(false) }
        action("scan.scanQr", R.drawable.ic_scan, run = ::scan)
        action("connection.recentConnections", R.drawable.ic_settings_host, run = openHosts)
        body.addView(ui.button(labels["settings.title"], action = openSettings).apply {
            stateListAnimator = null
            setTextColor(ui.muted)
        }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = activity.dp(4) })
    }

    fun discover(recentOnly: Boolean) {
        cancelDiscovery()
        notice = null
        progress = if (recentOnly) "connection.checkingHistory" else "connection.scanning"
        discovery = Thread {
            val worker = Thread.currentThread()
            val result = runCatching { discoverConnection(activity, connections.store.entries(), recentOnly) }
            activity.runOnUiThread {
                if (discovery !== worker) return@runOnUiThread
                discovery = null
                progress = null
                val target = result.getOrNull()?.let { ConnectionTarget.parse(it) }
                if (target == null) {
                    notice = labels[when {
                        result.exceptionOrNull() is NoLanNetwork -> "connection.noNetwork"
                        result.isFailure -> "connection.scanFailed"
                        recentOnly -> "connection.noHistoryAvailable"
                        else -> "connection.noHosts"
                    }]
                }
                render()
                if (target != null) connections.launch(target)
            }
        }.also { it.start() }
        render()
    }

    fun cancelDiscovery() {
        val worker = discovery ?: return
        discovery = null
        worker.interrupt()
        progress = null
        render()
    }

    private fun scan() {
        notice = null
        render()
        connections.readPairingLink { link ->
            if (link == null) return@readPairingLink
            progress = "connection.loading"
            render()
            connections.connect(link) { error ->
                progress = null
                notice = error
                render()
            }
        }
    }
}
