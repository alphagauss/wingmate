package dev.wingmate.mobile.launch

import android.app.Activity
import android.webkit.JavascriptInterface
import android.webkit.WebView
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.remote.ConnectionFlow
import dev.wingmate.mobile.remote.ConnectionTarget
import dev.wingmate.mobile.remote.NoLanNetwork
import dev.wingmate.mobile.remote.discoverConnection
import dev.wingmate.mobile.settings.LocalEngineSettings
import org.json.JSONObject

internal class ConnectionBridge(
    private val activity: Activity,
    private val webView: WebView,
    private val connections: ConnectionFlow,
    private val openSettings: () -> Unit,
    private val openRemote: () -> Unit,
    private val localEngine: LocalEngineSettings?,
    private val openLocalAgent: () -> Unit,
    private var autoConnectOnLaunch: Boolean,
    private val cancelLocalAutoOpen: () -> Unit
) {
    private val labels by lazy { ConnectionStrings(activity) }
    private var discovery: Thread? = null

    @JavascriptInterface
    fun postMessage(message: String) {
        val request = runCatching { JSONObject(message) }.getOrNull() ?: return
        val id = request.optInt("id", -1)
        val args = request.optJSONObject("args") ?: JSONObject()
        activity.runOnUiThread {
            if (activity.isFinishing || activity.isDestroyed) return@runOnUiThread
            fun reply(value: JSONObject? = null, error: String? = null) {
                val result = JSONObject().put("value", value).put("error", error)
                webView.evaluateJavascript("window.receiveConnectionResult?.($id, $result)", null)
            }

            val action = request.optString("action")
            if (action != "restore") {
                autoConnectOnLaunch = false
                cancelLocalAutoOpen()
            }
            when (action) {
                "scan" -> connections.readPairingLink { link -> reply(JSONObject().put("text", link)) }
                "open" -> connections.connect(args.optString("link")) { notice -> reply(error = notice) }
                "drawer" -> {
                    openRemote()
                    reply()
                }
                "settings" -> {
                    openSettings()
                    reply()
                }
                "localAgent" -> {
                    (localEngine?.openWorkbench ?: openLocalAgent).invoke()
                    reply()
                }
                "restore" -> {
                    args.optJSONArray("recent")?.let(connections.store::migrate)
                    val autoConnect = autoConnectOnLaunch && connections.store.entries().isNotEmpty()
                    autoConnectOnLaunch = false
                    reply(JSONObject()
                        .put("language", labels.language).put("messages", labels.messages)
                        .put("autoConnect", autoConnect)
                        .put("localEntry", localEntry))
                }
                "discover" -> discover(args.optBoolean("historyOnly")) {
                    reply(error = it)
                }
                "cancel" -> {
                    cancelDiscovery()
                    reply()
                }
                else -> reply(error = "未知的连接操作")
            }
        }
    }

    private val localEntry: String?
        get() = localEngine?.let {
            if (it.openWorkbench != null) "localAgent.workbench" else "localAgent.title"
        }

    fun updateLocalEntry() {
        webView.evaluateJavascript("window.updateLocalEntry?.(${JSONObject.quote(localEntry)})", null)
    }

    fun updateLabels() {
        webView.evaluateJavascript("window.applyWingmateLabels?.(${JSONObject.quote(labels.language)}, ${labels.messages})", null)
    }

    fun cancelDiscovery() {
        discovery?.interrupt()
        discovery = null
    }

    fun cancelStartupConnection() {
        autoConnectOnLaunch = false
        cancelDiscovery()
    }

    private fun discover(historyOnly: Boolean, reply: (String?) -> Unit) {
        cancelDiscovery()
        discovery = Thread {
            val worker = Thread.currentThread()
            val result = runCatching {
                discoverConnection(activity, connections.store.entries(), historyOnly)
            }
            activity.runOnUiThread {
                if (activity.isFinishing || activity.isDestroyed) return@runOnUiThread
                if (discovery != worker) {
                    reply(null)
                    return@runOnUiThread
                }
                discovery = null
                val target = result.getOrNull()?.let { ConnectionTarget.parse(it) }
                if (target != null) {
                    connections.launch(target)
                    reply(null)
                } else {
                    val key = when {
                        result.exceptionOrNull() is NoLanNetwork -> "connection.noNetwork"
                        result.isFailure -> "connection.scanFailed"
                        historyOnly -> "connection.noHistoryAvailable"
                        else -> "connection.noHosts"
                    }
                    reply(labels[key])
                }
            }
        }.also { it.start() }
    }
}
