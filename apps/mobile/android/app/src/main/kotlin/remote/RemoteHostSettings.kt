package dev.wingmate.mobile.remote

import android.content.Intent
import android.view.View
import android.webkit.CookieManager
import android.widget.EditText
import android.widget.LinearLayout
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import dev.wingmate.mobile.MainActivity
import dev.wingmate.mobile.OPEN_HOST_SELECTION
import dev.wingmate.mobile.R
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.settings.AppPreferences
import dev.wingmate.mobile.settings.SettingsUi
import java.util.concurrent.Executors

internal class RemoteHostSettings(
    private val activity: AppCompatActivity,
    private val flow: ConnectionFlow,
    currentOrigin: String?,
    private val localCurrent: Boolean,
    private val autoConnectDefault: Boolean
) {
    private val current = currentOrigin.takeUnless { localCurrent }
    private val labels = ConnectionStrings(activity)
    private val preferences = AppPreferences(activity)
    private var content: LinearLayout? = null
    private var openLocal: (() -> Unit)? = null
    private lateinit var ui: SettingsUi
    private var health = emptyMap<String, Boolean>()
    private var notice: String? = null
    private var checking = false
    private val refreshTimer = object : Runnable {
        override fun run() {
            val view = content ?: return
            refresh()
            view.postDelayed(this, 15_000)
        }
    }

    val currentName: String?
        get() = if (localCurrent) labels["localAgent.workbench"] else current?.let { origin ->
            (flow.store.entries().firstOrNull { it.origin == origin } ?: SavedConnection(origin)).label
        }

    fun attach(ui: SettingsUi, parent: LinearLayout, openLocal: (() -> Unit)?) {
        stop()
        this.ui = ui
        this.openLocal = openLocal
        content = LinearLayout(activity).apply { orientation = LinearLayout.VERTICAL }
            .also(parent::addView)
        render()
        refreshTimer.run()
    }

    fun stop() {
        content?.removeCallbacks(refreshTimer)
        content = null
        openLocal = null
    }

    private fun render() {
        val body = content ?: return
        val entries = flow.store.entries()
        val recent = entries.filterNot { it.origin == current }
        body.removeAllViews()
        with(ui) {
            notice?.let { body.addView(colors.text(it, 14f, colors.danger)) }
            body.section(labels["connection.currentConnection"]) {
                if (current != null) {
                    addView(hostRow(entries.firstOrNull { it.origin == current } ?: SavedConnection(current)))
                } else {
                    addView(row(currentName ?: labels["connection.noConnection"],
                        icon = R.drawable.ic_settings_engine.takeIf { localCurrent }))
                }
                if (current != null || localCurrent) {
                    addView(row(labels["connection.disconnect"], action = ::disconnect))
                }
            }
            if (!localCurrent) openLocal?.let { open ->
                body.section {
                    addView(row(labels["localAgent.workbench"], value = labels["connection.available"],
                        icon = R.drawable.ic_settings_engine, valueColor = success, action = open))
                }
            }
            body.section(labels["connection.recentConnections"]) {
                recent.forEach { addView(hostRow(it)) }
                if (recent.isEmpty()) addView(row(labels["connection.noHistory"]))
                addView(row(labels["connection.refreshConnections"], icon = R.drawable.ic_refresh, action = ::refresh))
            }
            body.section {
                addView(row(labels["scan.scanQr"], icon = R.drawable.ic_scan,
                    action = { flow.scan(::showNotice) }))
                addView(row(labels["scan.enterAddress"], icon = R.drawable.ic_settings_host,
                    action = { flow.scan(::showNotice, manual = true) }))
            }
            body.section {
                addView(toggle(labels["settings.autoConnect"], preferences.autoConnect(autoConnectDefault),
                    preferences::setAutoConnect))
            }
        }
    }

    private fun hostRow(entry: SavedConnection): View {
        val status = health[entry.origin]
        val state = labels[when (status) {
            true -> "connection.available"
            false -> "connection.unavailable"
            null -> "connection.checking"
        }]
        return ui.row(entry.label, value = state, icon = R.drawable.ic_settings_host,
            valueColor = if (status == true) ui.success else ui.colors.foreground,
            action = if (entry.origin == current) null else ({ flow.connect(entry.origin, ::showNotice) })
        ).apply {
            setOnLongClickListener { edit(entry); true }
            androidx.core.view.ViewCompat.setTooltipText(this, labels["connection.editHint"])
        }
    }

    private fun showNotice(message: String?) {
        notice = message
        render()
    }

    private fun refresh() {
        if (checking) return
        val origins = (flow.store.entries().map { it.origin } + listOfNotNull(current)).distinct()
        if (origins.isEmpty()) return
        checking = true
        val workers = Executors.newFixedThreadPool(minOf(origins.size, 5))
        val results = origins.associateWith { origin -> workers.submit<Boolean> { probeConnection(origin) } }
        workers.shutdown()
        Thread {
            val updated = results.mapValues { (_, result) -> result.get() }
            activity.runOnUiThread {
                checking = false
                if (current != null && health[current] == true && updated[current] == false) {
                    notice = labels["connection.hostOffline"]
                } else if (updated[current] == true && notice == labels["connection.hostOffline"]) {
                    notice = null
                }
                if (health != updated) {
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
                if (active) activity.finish() else render()
            }
            .show()
    }

    private fun disconnect() {
        activity.startActivity(Intent(activity, MainActivity::class.java).putExtra(OPEN_HOST_SELECTION, true))
        activity.finish()
    }
}
