package dev.wingmate.mobile.runtime

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.os.IBinder
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import androidx.lifecycle.Observer

/** Holds the PRoot process and the install in the foreground across activity changes. */
class DshService : Service() {
    private val notifications by lazy { getSystemService(NotificationManager::class.java) }
    private val connectivity by lazy { getSystemService(ConnectivityManager::class.java) }
    private val main = Handler(Looper.getMainLooper())
    private val wakeLock by lazy {
        getSystemService(PowerManager::class.java).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "wingmate:local-agent")
    }
    private val network = object : ConnectivityManager.NetworkCallback() {
        override fun onLinkPropertiesChanged(network: Network, properties: LinkProperties) {
            // Network callbacks run off the main thread.
            main.post { LocalRuntime.networkChanged() }
        }
    }
    private var foreground = false
    private val observer = Observer<RuntimeStatus> { status ->
        when (status) {
            RuntimeStatus.Stopped, is RuntimeStatus.Failed -> stopSelf()
            else -> notifications.notify(NOTIFICATION, notification(status))
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val action = intent?.action
        if (action == STOP || action == RESTART) {
            if (foreground) LocalRuntime.requestStop(restart = action == RESTART) else stopSelf(startId)
            return START_NOT_STICKY
        }
        if (!foreground) {
            notifications.createNotificationChannel(
                NotificationChannel(CHANNEL, getString(R.string.local_runtime_title), NotificationManager.IMPORTANCE_LOW),
            )
            startForeground(NOTIFICATION, notification(RuntimeStatus.Starting))
            foreground = true
            wakeLock.acquire()
            connectivity.registerDefaultNetworkCallback(network)
            LocalRuntime.launch(this)
            LocalRuntime.status.observeForever(observer)
        }
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        if (foreground) {
            LocalRuntime.status.removeObserver(observer)
            connectivity.unregisterNetworkCallback(network)
            LocalRuntime.requestStop()
            wakeLock.release()
        }
        super.onDestroy()
    }

    private fun notification(status: RuntimeStatus): Notification {
        val launch = packageManager.getLaunchIntentForPackage(packageName)
            ?.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            ?.putExtra("openLocalAgent", true)
        val builder = Notification.Builder(this, CHANNEL)
            .setSmallIcon(android.R.drawable.ic_menu_manage)
            .setContentTitle(getString(R.string.local_runtime_title))
            .setOngoing(true)
            .setOnlyAlertOnce(true)
        if (launch != null) builder.setContentIntent(PendingIntent.getActivity(this, 0, launch, PendingIntent.FLAG_IMMUTABLE))
        when (status) {
            is RuntimeStatus.Installing -> builder.setContentText(getString(R.string.local_runtime_installing))
                .setProgress(100, status.progress, status.progress == 0)
            RuntimeStatus.Starting -> builder.setContentText(getString(R.string.local_runtime_starting))
            RuntimeStatus.Stopping -> builder.setContentText(getString(R.string.local_runtime_stopping))
            is RuntimeStatus.Ready -> builder.setContentText(getString(R.string.local_runtime_running))
                .addAction(Notification.Action.Builder(null, getString(R.string.local_runtime_stop),
                    PendingIntent.getService(this, 0, Intent(this, DshService::class.java).setAction(STOP),
                        PendingIntent.FLAG_IMMUTABLE)).build())
            else -> Unit
        }
        return builder.build()
    }

    companion object {
        internal const val STOP = "dev.wingmate.mobile.runtime.STOP"
        internal const val RESTART = "dev.wingmate.mobile.runtime.RESTART"
        private const val CHANNEL = "wingmate-local-agent"
        private const val NOTIFICATION = 3080
    }
}
