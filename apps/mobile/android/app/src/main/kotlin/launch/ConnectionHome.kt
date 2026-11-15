package dev.wingmate.mobile.launch

import android.content.res.ColorStateList
import android.graphics.Color
import android.graphics.drawable.RippleDrawable
import android.view.Gravity
import android.view.View
import android.transition.AutoTransition
import android.transition.TransitionManager
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
import dev.wingmate.mobile.remote.DiscoveredHost
import dev.wingmate.mobile.remote.NoLanNetwork
import dev.wingmate.mobile.remote.SavedConnection
import dev.wingmate.mobile.remote.discoverConnections
import dev.wingmate.mobile.remote.lanPasswordView
import dev.wingmate.mobile.settings.LocalEngineSettings
import dev.wingmate.mobile.theme.ConnectionUi
import dev.wingmate.mobile.theme.dp
import dev.wingmate.mobile.theme.rounded
import java.util.Collections

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
    private var partialResults: MutableList<Pair<DiscoveredHost, String>>? = null
    private var results: LinearLayout? = null

    init {
        view.addView(body, FrameLayout.LayoutParams(-1, -2))
        render()
    }

    fun render() {
        val ui = ConnectionUi(activity)
        val monochrome = if (ui.dark) Color.WHITE else Color.BLACK
        view.setBackgroundColor(ui.background)
        body.removeAllViews()
        results?.let {
            body.addView(it)
            return
        }
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
                if (discovery != null) addView(ui.button(labels["connection.cancelSearch"]) { cancelDiscovery(keepResults = true) })
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
        val partial = Collections.synchronizedList(mutableListOf<Pair<DiscoveredHost, String>>())
        partialResults = partial
        discovery = Thread {
            val worker = Thread.currentThread()
            val result = runCatching {
                val history = connections.store.entries()
                val saved = history.associateBy { it.origin }
                fun label(host: DiscoveredHost) = host to (saved[host.previousOrigin]?.name?.takeIf { it.isNotBlank() } ?: host.identity.name)
                discoverConnections(activity, history, recentOnly) { partial.add(label(it)) }.map(::label)
            }
            activity.runOnUiThread {
                if (discovery !== worker) return@runOnUiThread
                discovery = null
                partialResults = null
                progress = null
                val targets = result.getOrNull().orEmpty()
                if (targets.isEmpty()) {
                    notice = labels[when {
                        result.exceptionOrNull() is NoLanNetwork -> "connection.noNetwork"
                        result.isFailure -> "connection.scanFailed"
                        recentOnly -> "connection.noHistoryAvailable"
                        else -> "connection.noHosts"
                    }]
                }
                if (targets.isNotEmpty()) showResults(targets, autoConnect = recentOnly)
                render()
            }
        }.also { it.start() }
        render()
    }

    private fun showResults(hosts: List<Pair<DiscoveredHost, String>>, autoConnect: Boolean = false) {
        val ui = ConnectionUi(activity)
        val list = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            isFocusableInTouchMode = true
        }
        results = list
        list.addView(LinearLayout(activity).apply {
            gravity = Gravity.CENTER_VERTICAL
            addView(ui.text(labels["connection.lanHosts"], 20f, bold = true), LinearLayout.LayoutParams(0, -2, 1f))
            addView(ui.button(labels["settings.back"], action = { closeLanResults() }))
        })
        var selected: DiscoveredHost? = null
        var opened: LinearLayout? = null
        for ((host, name) in hosts) {
            val row = LinearLayout(activity).apply {
                orientation = LinearLayout.VERTICAL
                gravity = Gravity.CENTER_VERTICAL
                minimumHeight = activity.dp(60)
                setPadding(activity.dp(8), activity.dp(8), activity.dp(8), activity.dp(8))
                isFocusable = true
                val address = SavedConnection(host.origin).label
                addView(ui.text(name.ifBlank { address }, 16f, bold = true))
                if (name.isNotBlank()) addView(ui.text(address, 13f, ui.muted))
            }
            val details = LinearLayout(activity).apply { orientation = LinearLayout.VERTICAL }
            list.addView(row)
            list.addView(details)
            row.setOnClickListener {
                connections.cancelConnection()
                TransitionManager.beginDelayedTransition(list, AutoTransition().setDuration(180))
                opened?.removeAllViews()
                opened = null
                if (selected == host) {
                    selected = null
                    return@setOnClickListener
                }
                selected = host
                connections.select(host) { message ->
                    if (selected != host || results !== list) return@select
                    TransitionManager.beginDelayedTransition(list, AutoTransition().setDuration(180))
                    opened = details
                    if (message == null) {
                        details.addView(lanPasswordView(activity, ui) { password, onError ->
                            connections.login(host, password, onError)
                        })
                    } else details.addView(ui.text(message, 14f, ui.muted))
                }
            }
            if (autoConnect) row.performClick()
        }
    }

    fun closeLanResults(): Boolean {
        if (results == null) return false
        results = null
        connections.cancelConnection()
        render()
        return true
    }

    fun cancelDiscovery(keepResults: Boolean = false) {
        val worker = discovery ?: return
        discovery = null
        worker.interrupt()
        val found = partialResults
        partialResults = null
        progress = null
        if (keepResults && found != null) {
            val snapshot = synchronized(found) { found.toList() }
            if (snapshot.isNotEmpty()) showResults(snapshot)
        }
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
