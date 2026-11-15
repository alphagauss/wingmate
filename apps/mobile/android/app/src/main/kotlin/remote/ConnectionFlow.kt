package dev.wingmate.mobile.remote

import android.app.Activity
import android.content.Intent
import android.webkit.CookieManager
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.web.WorkbenchActivity

internal class ConnectionFlow(
    private val activity: ComponentActivity,
    private val onConnectionRequested: () -> Unit = {}
) {
    val store by lazy { ConnectionStore(activity) }
    private val labels by lazy { ConnectionStrings(activity) }
    private var scanningResult: ((String?) -> Unit)? = null
    private var attempt = 0
    private val scanner = activity.registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        val result = scanningResult
        scanningResult = null
        result?.invoke(if (it.resultCode == Activity.RESULT_OK) it.data?.getStringExtra("text") else null)
    }

    fun scan(onNotice: (String?) -> Unit, manual: Boolean = false) {
        readPairingLink(manual) { link -> if (link != null) connect(link, onNotice) }
    }

    fun readPairingLink(manual: Boolean = false, onResult: (String?) -> Unit) {
        onConnectionRequested()
        cancelConnection()
        scanningResult = onResult
        scanner.launch(Intent(activity, ScannerActivity::class.java).putExtra("manual", manual))
    }

    fun cancelConnection() {
        attempt++
    }

    fun connect(raw: String, onNotice: (String?) -> Unit) {
        onConnectionRequested()
        val generation = ++attempt
        val target = ConnectionTarget.parse(raw)
        if (target == null) {
            onNotice(labels["scan.invalidAddress"])
            return
        }
        Thread {
            val resolved = resolveConnection(activity, target, 5000)
            activity.runOnUiThread {
                if (!current(generation)) return@runOnUiThread
                if (resolved == null) {
                    onNotice(labels["connection.connectionFailed"])
                    return@runOnUiThread
                }
                onNotice(null)
                launch(if (resolved.origin == target.origin) target else resolved.target, resolved.previousOrigin)
            }
        }.start()
    }

    fun select(host: DiscoveredHost, onResult: (String?) -> Unit) {
        select({ host }) { message, _ -> onResult(message) }
    }

    fun select(entry: SavedConnection, onResult: (String?, DiscoveredHost?) -> Unit) {
        select({ ConnectionTarget.parse(entry.origin)?.let { resolveConnection(activity, it) } }, onResult)
    }

    private fun select(resolve: () -> DiscoveredHost?, onResult: (String?, DiscoveredHost?) -> Unit) {
        onConnectionRequested()
        val generation = ++attempt
        Thread {
            val host = runCatching { resolve() }.getOrNull()
            val session = runCatching { host != null && (host.origin.startsWith("https:") || hasLanSession(host.origin)) }
            activity.runOnUiThread {
                if (!current(generation)) return@runOnUiThread
                when {
                    host == null || session.isFailure -> onResult(labels["connection.connectionFailed"], null)
                    session.getOrDefault(false) -> launch(host.target, host.previousOrigin)
                    host.identity.password -> onResult(null, host)
                    else -> onResult(labels["connection.pairRequired"], host)
                }
            }
        }.start()
    }

    fun login(host: DiscoveredHost, password: String, onError: (String) -> Unit) {
        val generation = ++attempt
        Thread {
            val reply = runCatching { submitLanPassword(host.origin, password) }.getOrNull()
            activity.runOnUiThread {
                if (!current(generation)) return@runOnUiThread
                val cookie = reply?.cookie
                if (reply?.status == 204 && cookie != null) {
                    val cookies = CookieManager.getInstance()
                    cookies.setCookie(host.origin, cookie) { stored ->
                        if (!current(generation)) return@setCookie
                        if (stored) {
                            cookies.flush()
                            launch(host.target, host.previousOrigin)
                        } else onError(labels["connection.connectionFailed"])
                    }
                } else onError(labels[when (reply?.status) {
                    400, 401 -> "connection.passwordIncorrect"
                    404 -> "connection.pairRequired"
                    429 -> "connection.passwordRateLimited"
                    else -> "connection.connectionFailed"
                }])
            }
        }.start()
    }

    private fun current(generation: Int) = generation == attempt && !activity.isFinishing && !activity.isDestroyed

    private fun launch(target: ConnectionTarget, previousOrigin: String) {
        store.remember(target.origin, previousOrigin)
        val intent = Intent(activity, WorkbenchActivity::class.java).putExtra("link", target.link)
        activity.startActivity(intent)
        if (activity is WorkbenchActivity) activity.finish()
    }
}
