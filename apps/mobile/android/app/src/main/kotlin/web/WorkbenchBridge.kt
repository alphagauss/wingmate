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
import android.widget.Toast
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
import org.json.JSONObject

internal class WorkbenchBridge(
    private val activity: WorkbenchActivity,
    private val view: WebView,
    private val origin: String,
    private val onReady: () -> Unit,
    private val onTheme: (String) -> Unit
) {
    private val labels = ConnectionStrings(activity)
    private val preferences = AppPreferences(activity)
    private val listener = WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)
    private var ready = false
    private val script by lazy {
        activity.assets.open("web/workbench-bridge.js").bufferedReader().use { it.readText() }
            .replace("__WINGMATE_ORIGIN__", JSONObject.quote(origin))
    }

    init {
        if (listener) {
            WebViewCompat.addWebMessageListener(view, "WingmateWorkbench", setOf(origin)) { _, message, source, top, _ ->
                if (top && source.toString().removeSuffix("/") == origin &&
                    message.type == WebMessageCompat.TYPE_STRING
                ) receive(message.data)
            }
            WebViewCompat.addDocumentStartJavaScript(view, script, setOf(origin))
        }
    }

    fun started() { ready = false }

    fun finished() {
        ready = listener
        onReady()
    }

    fun focus(session: String): Boolean {
        if (!ready) return false
        view.evaluateJavascript(
            "if (location.origin === ${JSONObject.quote(origin)}) window.wingmateOpenSession?.(${JSONObject.quote(session)});",
            null
        )
        return true
    }

    private fun receive(raw: String?) {
        if (raw == null || raw.length > 4096) return
        val message = runCatching { JSONObject(raw) }.getOrNull() ?: return
        when (message.optString("event")) {
            "theme" -> {
                val theme = message.optString("theme")
                if (theme in listOf("light", "dark", "system")) onTheme(theme)
            }
            "completed" -> if (ready) notify(message)
            "session-unavailable" -> if (ready) {
                Toast.makeText(activity, labels["connection.focusFailed"], Toast.LENGTH_SHORT).show()
            }
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
        val session = payload.opt("sessionId") as? String ?: return
        if (session.length !in 1..256) return
        val title = (payload.opt("title") as? String).orEmpty().trim().take(256)
            .ifBlank { labels["notification.channel"] }
        val body = labels["notification.completed"]

        val channel = "wingmate-tasks"
        if (Build.VERSION.SDK_INT >= 26) {
            val manager = activity.getSystemService(NotificationManager::class.java)
            manager.createNotificationChannel(NotificationChannel(
                channel, labels["notification.channel"], NotificationManager.IMPORTANCE_DEFAULT
            ))
        }
        val id = "$origin/$session".hashCode()
        val click = Intent(activity, WorkbenchActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            .putExtra("link", "$origin/").putExtra("notificationSession", session)
            .putExtra("localAgent", activity.intent.getBooleanExtra("localAgent", false))
        val notification = NotificationCompat.Builder(activity, channel)
            .setSmallIcon(R.drawable.ic_manage)
            .setContentTitle(title).setContentText(body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setAutoCancel(true)
            .setContentIntent(PendingIntent.getActivity(
                activity, id, click, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
            ))
            .build()
        NotificationManagerCompat.from(activity).notify(id, notification)
    }
}
