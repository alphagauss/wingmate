package dev.wingmate.mobile.remote

import android.app.Activity
import android.content.Intent
import android.content.res.ColorStateList
import android.graphics.Color
import android.text.TextUtils
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.webkit.CookieManager
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.widget.AppCompatImageButton
import androidx.drawerlayout.widget.DrawerLayout
import dev.wingmate.mobile.R
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.theme.ConnectionUi
import dev.wingmate.mobile.theme.dp
import dev.wingmate.mobile.theme.rounded
import java.util.concurrent.Executors

// Port of apps/native/src/app/index.tsx ConnectionDrawer and its slide layout.
internal class ConnectionDrawer(
    private val activity: AppCompatActivity,
    private val content: View,
    private val flow: ConnectionFlow,
    private val current: String? = null,
    private val openLocalAgent: (() -> Unit)? = null,
    private val localCurrent: Boolean = false,
) : DrawerLayout(activity) {
    private var ui = ConnectionUi(activity)
    private val labels = ConnectionStrings(activity)
    private var theme = "system"
    private val panel = column()
    private val body = column()
    private val notice = ui.text("", 14f, ui.danger).apply { visibility = GONE }
    private var health: Map<String, Boolean> = emptyMap()
    private var checking = false
    private val refreshTimer = object : Runnable {
        override fun run() {
            if (showing()) {
                refresh()
                postDelayed(this, 15_000)
            }
        }
    }

    init {
        addView(content, LayoutParams(-1, -1))
        stylePanel()
        val displayWidth = resources.displayMetrics.widthPixels
        addView(panel, LayoutParams(drawerWidth(displayWidth), -1, Gravity.RIGHT))
        addOnLayoutChangeListener { _, left, _, right, _, _, _, _, _ ->
            val width = drawerWidth(right - left)
            if (width > 0 && panel.layoutParams.width != width) {
                panel.layoutParams = panel.layoutParams.apply { this.width = width }
            }
        }
        addDrawerListener(object : SimpleDrawerListener() {
            override fun onDrawerSlide(drawerView: View, slideOffset: Float) {
                content.translationX = -drawerView.width * slideOffset
            }

            override fun onDrawerOpened(drawerView: View) {
                render()
                refresh()
                removeCallbacks(refreshTimer)
                postDelayed(refreshTimer, 15_000)
            }

            override fun onDrawerClosed(drawerView: View) { removeCallbacks(refreshTimer) }
        })
    }

    fun applyTheme(next: String) {
        if (next == theme) return
        theme = next
        ui = ConnectionUi(activity, when (next) { "dark" -> true; "light" -> false; else -> null })
        stylePanel()
    }

    private fun stylePanel() {
        setBackgroundColor(ui.background)
        setScrimColor(ui.backdrop)
        notice.setTextColor(ui.danger)
        (body.parent as? ViewGroup)?.removeView(body)
        panel.apply {
            removeAllViews()
            setBackgroundColor(ui.surface)
            addView(heading(labels["connection.connectionInfo"], 20f, R.drawable.ic_scan, labels["scan.scanQr"]) {
                close()
                flow.scan(::showNotice)
            }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(8) })
            addView(ScrollView(activity).apply {
                addView(body)
            }, LinearLayout.LayoutParams(-1, 0, 1f))
            addView(View(activity).apply { setBackgroundColor(ui.border) }, -1, dp(1))
            addView(column().apply {
                setPadding(dp(20), dp(20), dp(20), dp(16))
                addView(ui.button(labels["connection.disconnect"]) { disconnect(scan = true) }.apply {
                    background = activity.rounded(ui.danger, 16)
                    setTextColor(ui.onDanger)
                    isEnabled = current != null
                    alpha = if (isEnabled) 1f else .5f
                }, -1, dp(40))
                addView(ui.button(labels["connection.closeDrawer"]) { close() }.apply {
                    setCompoundDrawablesWithIntrinsicBounds(R.drawable.ic_close, 0, 0, 0)
                    compoundDrawableTintList = ColorStateList.valueOf(ui.muted)
                    compoundDrawablePadding = dp(8)
                }, LinearLayout.LayoutParams(-1, dp(40)).apply { topMargin = dp(12) })
            })
        }
        render()
    }

    private fun dp(value: Int) = activity.dp(value)
    private fun drawerWidth(width: Int) = (width * .88f).toInt().coerceAtMost(dp(384))

    override fun open() = openDrawer(Gravity.RIGHT)
    override fun close() = closeDrawer(Gravity.RIGHT)
    fun showing() = isDrawerVisible(Gravity.RIGHT)

    override fun onDetachedFromWindow() {
        removeCallbacks(refreshTimer)
        super.onDetachedFromWindow()
    }

    private fun column() = LinearLayout(activity).apply { orientation = LinearLayout.VERTICAL }

    private fun heading(label: String, size: Float, icon: Int, description: String, action: () -> Unit) =
        LinearLayout(activity).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(20), 0, dp(20), 0)
            addView(ui.text(label, size, bold = true), LinearLayout.LayoutParams(0, -2, 1f))
            addView(AppCompatImageButton(activity).apply {
                setImageResource(icon)
                imageTintList = ColorStateList.valueOf(ui.foreground)
                setBackgroundResource(android.R.color.transparent)
                contentDescription = description
                val inset = dp(11)
                setPadding(inset, inset, inset, inset)
                setOnClickListener { action() }
            }, dp(40), dp(40))
        }

    private fun render() {
        val entries = flow.store.entries().take(5)
        body.removeAllViews()
        (notice.parent as? ViewGroup)?.removeView(notice)
        body.addView(notice, LinearLayout.LayoutParams(-1, -2).apply { setMargins(dp(20), 0, dp(20), dp(12)) })
        body.addView(ui.text(labels["connection.currentConnection"], 14f, bold = true), LinearLayout.LayoutParams(-1, -2).apply {
            setMargins(dp(20), 0, dp(20), dp(12))
        })
        if (localCurrent) {
            addLocalAgent()
        } else if (current == null) {
            body.addView(ui.text(labels["connection.noConnection"], 14f, ui.muted).apply {
                setPadding(dp(32), dp(20), dp(32), dp(20))
            })
        } else {
            body.addView(row(entries.firstOrNull { it.origin == current } ?: SavedConnection(current)))
        }
        if (!localCurrent) addLocalAgent()
        body.addView(heading(labels["connection.recentConnections"], 14f, R.drawable.ic_refresh, labels["connection.refreshConnections"], ::refresh),
            LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(24) })
        if (entries.isEmpty()) {
            body.addView(ui.text(labels["connection.noHistory"], 14f, ui.muted).apply {
                setPadding(dp(20), dp(16), dp(20), dp(16))
            })
        } else {
            entries.forEach { entry -> body.addView(row(entry), LinearLayout.LayoutParams(-1, -2).apply {
                topMargin = dp(8)
            }) }
        }
        body.setPadding(0, 0, 0, dp(24))
    }

    private fun addLocalAgent() {
        val open = openLocalAgent ?: return
        body.addView(ui.button(labels["localAgent.title"]) {
            close()
            open()
        }, LinearLayout.LayoutParams(-1, -2).apply { setMargins(dp(20), dp(8), dp(20), 0) })
    }

    private fun row(entry: SavedConnection) = LinearLayout(activity).apply {
        gravity = Gravity.CENTER_VERTICAL
        minimumHeight = dp(64)
        setPadding(dp(32), dp(16), dp(32), dp(16))
        val status = health[entry.origin]
        val dot = when (status) {
            true -> Color.parseColor("#22C55E")
            false -> ui.danger
            null -> ui.muted
        }
        addView(View(activity).apply { background = activity.rounded(dot, 5) }, dp(10), dp(10))
        addView(ui.text(entry.label, 14f).apply {
            maxLines = 1
            ellipsize = TextUtils.TruncateAt.END
        }, LinearLayout.LayoutParams(-1, -2).apply { marginStart = dp(12) })
        val state = when (status) { true -> labels["connection.available"]; false -> labels["connection.unavailable"]; null -> labels["connection.checking"] }
        contentDescription = labels["connection.hostStatus"].replace("{{host}}", entry.label)
            .replace("{{status}}", state) + " · " + labels["connection.editHint"]
        isFocusable = true
        setOnClickListener { flow.connect(entry.origin, ::showNotice) }
        setOnLongClickListener { edit(entry); true }
    }

    private fun showNotice(message: String?) {
        notice.text = message
        notice.visibility = if (message == null) GONE else VISIBLE
        if (message != null && !showing()) open()
    }

    private fun refresh() {
        if (checking) return
        val remoteCurrent = current.takeUnless { localCurrent }
        val origins = (flow.store.entries().take(5).map { it.origin } + listOfNotNull(remoteCurrent)).distinct()
        if (origins.isEmpty()) return
        checking = true
        val workers = Executors.newFixedThreadPool(origins.size)
        val results = origins.associateWith { origin -> workers.submit<Boolean> { probeConnection(origin) } }
        workers.shutdown()
        Thread {
            val updated = results.mapValues { (_, result) -> result.get() }
            activity.runOnUiThread {
                checking = false
                if (!activity.isFinishing && !activity.isDestroyed) {
                    if (current != null && health[current] == true && updated[current] == false)
                        showNotice(labels["connection.hostOffline"])
                    if (current != null && updated[current] == true && notice.text == labels["connection.hostOffline"])
                        showNotice(null)
                    health = updated
                    render()
                }
            }
        }.start()
    }

    private fun edit(entry: SavedConnection) {
        AlertDialog.Builder(activity)
            .setTitle(entry.label)
            .setItems(arrayOf(labels["connection.rename"], labels["connection.delete"])) { _, selected ->
                if (selected == 0) rename(entry) else delete(entry)
            }
            .setNegativeButton(labels["common.cancel"], null)
            .show()
    }

    private fun rename(entry: SavedConnection) {
        val input = EditText(activity).apply {
            setText(entry.label)
            setSelectAllOnFocus(true)
            setSingleLine()
            contentDescription = labels["connection.name"]
        }
        AlertDialog.Builder(activity)
            .setTitle(labels["connection.rename"])
            .setView(input)
            .setNegativeButton(labels["common.cancel"], null)
            .setPositiveButton(labels["common.save"]) { _, _ ->
                val name = input.text.toString().trim()
                if (name.isNotEmpty()) flow.store.rename(entry.origin, name)
                render()
            }
            .show()
    }

    private fun delete(entry: SavedConnection) {
        val active = entry.origin == current
        AlertDialog.Builder(activity)
            .setTitle(labels["connection.delete"])
            .setMessage(if (active) labels["connection.deleteCurrentConfirm"] else labels["connection.deleteConfirm"])
            .setNegativeButton(labels["common.cancel"], null)
            .setPositiveButton(labels["connection.delete"]) { _, _ ->
                flow.store.forget(entry.origin)
                val cookies = CookieManager.getInstance()
                val secure = if (entry.origin.startsWith("https:")) "; Secure" else ""
                cookies.getCookie(entry.origin)?.split(';')?.forEach { cookie ->
                    val name = cookie.substringBefore('=').trim()
                    if (name.startsWith("wingmate_share_"))
                        cookies.setCookie(entry.origin, "$name=; Path=/; Max-Age=0; HttpOnly$secure")
                }
                cookies.flush()
                if (active) disconnect(scan = false) else render()
            }
            .show()
    }

    private fun disconnect(scan: Boolean) {
        if (current == null) return
        if (scan) activity.setResult(Activity.RESULT_OK, Intent().putExtra("scanAfterDisconnect", current))
        activity.finish()
    }
}
