package dev.wingmate.mobile.web

import android.Manifest
import android.annotation.SuppressLint
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.webkit.WebView
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ProcessLifecycleOwner
import androidx.webkit.WebMessageCompat
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import dev.wingmate.mobile.R
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.settings.AppPreferences
import java.util.UUID
import org.json.JSONObject

internal class WorkbenchBridge(
    private val activity: WorkbenchActivity,
    private val view: WebView,
    private val origin: String,
    private val onReady: () -> Unit,
    private val onTheme: (String) -> Unit
) {
    private val nonce = UUID.randomUUID().toString()
    private val labels = ConnectionStrings(activity)
    private val preferences = AppPreferences(activity)
    private val listener = WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)
    private val documentStart = listener && WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)
    private var ready = false
    private val script by lazy {
        activity.assets.open("web/workbench-bridge.js").bufferedReader().use { it.readText() }
            .replace("__WINGMATE_ORIGIN__", JSONObject.quote(origin))
            .replace("__WINGMATE_NONCE__", JSONObject.quote(nonce))
    }

    init {
        if (listener) {
            WebViewCompat.addWebMessageListener(view, "WingmateWorkbench", setOf(origin)) { _, message, source, top, _ ->
                if (top && source.toString().removeSuffix("/") == origin &&
                    message.type == WebMessageCompat.TYPE_STRING
                ) receive(message.data)
            }
        }
        if (documentStart) WebViewCompat.addDocumentStartJavaScript(view, script, setOf(origin))
    }

    fun started() { ready = false }

    fun finished() {
        if (!listener) onReady()
        else if (!documentStart) view.evaluateJavascript(script, null)
        else if (!ready) view.evaluateJavascript("window.__dshBridgeReady?.();", null)
    }

    fun visible(visible: Boolean) {
        send(JSONObject().put("type", "dsh://visibility-state").put("hidden", !visible))
    }

    fun focus(session: String, tag: String): Boolean =
        send(JSONObject().put("type", "dsh://focus-session")
            .put("requestId", UUID.randomUUID().toString()).put("sessionId", session).put("tag", tag))

    private fun send(message: JSONObject): Boolean {
        if (!ready) return false
        message.put("nonce", nonce)
        view.evaluateJavascript(
            "if (location.origin === ${JSONObject.quote(origin)}) window.__dshBridgeReceive?.($message);",
            null
        )
        return true
    }

    private fun receive(raw: String?) {
        if (raw == null || raw.length > 16384) return
        val message = runCatching { JSONObject(raw) }.getOrNull() ?: return
        if (message.optString("channel") != "dsh-bridge" || message.optString("origin") != origin ||
            message.optString("nonce") != nonce
        ) return
        val payload = message.optJSONObject("payload") ?: return
        when (payload.optString("type")) {
            "dsh://bridge-ready" -> {
                ready = true
                onReady()
            }
            "dsh://theme-state" -> {
                val theme = payload.optString("theme")
                if (ready && theme in listOf("light", "dark", "system")) onTheme(theme)
            }
            "dsh://native-notification" -> if (ready) notify(payload)
        }
    }

    @SuppressLint("MissingPermission")
    private fun notify(payload: JSONObject) {
        if (!preferences.taskNotifications) return
        if (ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)) return
        if (Build.VERSION.SDK_INT >= 33 && ContextCompat.checkSelfPermission(
                activity, Manifest.permission.POST_NOTIFICATIONS
            ) != PackageManager.PERMISSION_GRANTED
        ) return
        val title = payload.opt("title") as? String ?: return
        val body = payload.opt("body") as? String ?: return
        val tag = payload.optString("tag")
        val session = payload.optString("sessionId")
        if (title.isBlank() || title.length > 256 || body.length > 4096 || tag.length > 256 ||
            session.length > 256
        ) return

        val silent = payload.optBoolean("silent")
        val channel = if (silent) "dsh-bridge-silent" else "dsh-bridge"
        if (Build.VERSION.SDK_INT >= 26) {
            val manager = activity.getSystemService(NotificationManager::class.java)
            manager.createNotificationChannel(NotificationChannel(
                channel, if (silent) labels["notification.channelSilent"] else labels["notification.channel"],
                if (silent) NotificationManager.IMPORTANCE_LOW else NotificationManager.IMPORTANCE_DEFAULT
            ))
        }
        val id = "$origin/$tag".hashCode()
        val click = Intent(activity, WorkbenchActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            .putExtra("link", "$origin/").putExtra("notificationSession", session)
            .putExtra("localAgent", activity.intent.getBooleanExtra("localAgent", false))
            .putExtra("notificationTag", tag)
        val notification = NotificationCompat.Builder(activity, channel)
            .setSmallIcon(R.drawable.ic_manage)
            .setContentTitle(title).setContentText(body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setSilent(silent).setAutoCancel(true)
            .setContentIntent(PendingIntent.getActivity(
                activity, id, click, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
            ))
            .build()
        NotificationManagerCompat.from(activity).notify(id, notification)
    }
}
