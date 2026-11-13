package dev.wingmate.mobile.remote

import android.app.Activity
import android.content.Intent
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
            val available = probeConnection(target.origin, 5000)
            activity.runOnUiThread {
                if (generation != attempt || activity.isFinishing || activity.isDestroyed) return@runOnUiThread
                if (!available) {
                    onNotice(labels["connection.connectionFailed"])
                    return@runOnUiThread
                }
                onNotice(null)
                launch(target)
            }
        }.start()
    }

    fun launch(target: ConnectionTarget) {
        store.remember(target.origin)
        val intent = Intent(activity, WorkbenchActivity::class.java).putExtra("link", target.link)
        activity.startActivity(intent)
        if (activity is WorkbenchActivity) activity.finish()
    }
}
