package dev.wingmate.mobile.runtime.phone

import android.annotation.SuppressLint
import android.app.ActivityManager
import android.app.ActivityOptions
import android.app.KeyguardManager
import android.app.PendingIntent
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.PixelFormat
import android.hardware.display.DisplayManager
import android.hardware.display.VirtualDisplay
import android.media.ImageReader
import android.os.Binder
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Parcel
import android.os.PowerManager
import android.os.Process
import android.os.SystemClock
import android.view.Display
import androidx.annotation.Keep
import androidx.annotation.RequiresApi
import kotlin.system.exitProcess

// Shizuku UserService and isolated display flags adapted from dsh-Go's PrivilegedDisplayCore.
// The privileged process only creates, launches onto and tears down its own display.
@Keep
class BackgroundDisplayService(context: Context) : Binder() {
    private val ownerUid = context.applicationInfo.uid
    private val host by lazy {
        if (Process.myUid() == Process.SHELL_UID) {
            context.createPackageContext("com.android.shell", Context.CONTEXT_IGNORE_SECURITY)
        } else context
    }
    private val lock = Any()
    private var display: VirtualDisplay? = null
    private var reader: ImageReader? = null
    private var lease: IBinder? = null
    @Volatile private var retired = false
    private val ownerDied = IBinder.DeathRecipient { exitProcess(0) }

    init {
        attachInterface(null, DESCRIPTOR)
    }

    override fun onTransact(code: Int, data: Parcel, reply: Parcel?, flags: Int): Boolean {
        if (code != OPEN && code != CLOSE && code != DESTROY)
            return super.onTransact(code, data, reply, flags)
        val sender = getCallingUid()
        if (code == DESTROY) {
            if (sender != ownerUid && sender != Process.myUid())
                throw SecurityException("Invalid owner.")
            data.enforceInterface(DESCRIPTOR)
            retired = true
            val identity = clearCallingIdentity()
            try {
                synchronized(lock) { release() }
            } finally {
                restoreCallingIdentity(identity)
                exitProcess(0)
            }
        }
        if (sender != ownerUid) throw SecurityException("Invalid owner.")
        data.enforceInterface(DESCRIPTOR)
        if (code == CLOSE) {
            retired = true
            val identity = clearCallingIdentity()
            try {
                synchronized(lock) { release() }
                checkNotNull(reply).writeNoException()
                return true
            } finally {
                restoreCallingIdentity(identity)
            }
        }
        val component = data.readString()?.let(ComponentName::unflattenFromString)
        val owner = data.readStrongBinder()
        if (Build.VERSION.SDK_INT >= 33) data.enforceNoDataAvail()
        val identity = clearCallingIdentity()
        try {
            val result =
                synchronized(lock) {
                    if (display != null) {
                        Result.failure(PhoneProblem("Close the current background screen first."))
                    } else
                        runCatching {
                            if (Build.VERSION.SDK_INT < 34)
                                throw PhoneProblem(BackgroundPolicy.REQUIREMENT)
                            phoneCheck(
                                component != null && owner != null,
                                "Invalid background screen target.",
                            )
                            open(checkNotNull(component), checkNotNull(owner))
                        }
                            .onFailure { release() }
                }
            checkNotNull(reply).writeNoException()
            reply.writeInt(result.getOrNull() ?: Display.INVALID_DISPLAY)
            val failure = result.exceptionOrNull()
            reply.writeString(
                if (failure is PhoneProblem) failure.message
                else if (failure != null) "Android declined the Shizuku background screen."
                else null,
            )
            return true
        } finally {
            restoreCallingIdentity(identity)
        }
    }

    @RequiresApi(34)
    @SuppressLint("BlockedPrivateApi")
    @Suppress("DEPRECATION")
    private fun open(component: ComponentName, owner: IBinder): Int {
        phoneCheck(
            Process.myUid() == Process.SHELL_UID || Process.myUid() == 0,
            "Start the Shizuku service and authorize Wingmate.",
        )
        phoneCheck(!PhoneSafety.blocked(component.packageName), "This application is protected.")
        val launcher =
            Intent(Intent.ACTION_MAIN)
                .addCategory(Intent.CATEGORY_LAUNCHER)
                .setPackage(component.packageName)
        phoneCheck(
            host.packageManager.queryIntentActivities(launcher, 0).any {
                it.activityInfo.exported &&
                    it.activityInfo.enabled &&
                    ComponentName(it.activityInfo.packageName, it.activityInfo.name) == component
            },
            "The background target is no longer a launcher app.",
        )

        owner.linkToDeath(ownerDied, 0)
        lease = owner
        ensureReady(owner)
        val displayManager = host.getSystemService(DisplayManager::class.java)
        // Every flag is required. In particular, do not create a display that steals top focus.
        val flags =
            listOf(
                    "PUBLIC",
                    "OWN_CONTENT_ONLY",
                    "SUPPORTS_TOUCH",
                    "DESTROY_CONTENT_ON_REMOVAL",
                    "TRUSTED",
                    "OWN_FOCUS",
                    "STEAL_TOP_FOCUS_DISABLED",
                )
                .fold(0) { bits, name ->
                    bits or
                        DisplayManager::class
                            .java
                            .getDeclaredField("VIRTUAL_DISPLAY_FLAG_$name")
                            .getInt(null)
                }
        val surfaceReader = ImageReader.newInstance(720, 1280, PixelFormat.RGBA_8888, 2)
        reader = surfaceReader
        // Window screenshots are taken by the accessibility service. Drain the output surface.
        surfaceReader.setOnImageAvailableListener(
            { runCatching { it.acquireLatestImage()?.close() } },
            Handler(Looper.getMainLooper()),
        )
        val virtual =
            displayManager.createVirtualDisplay(
                "Wingmate background",
                720,
                1280,
                320,
                surfaceReader.surface,
                flags,
            ) ?: throw PhoneProblem("Android could not create a background screen.")
        display = virtual
        val id = virtual.display.displayId
        phoneCheck(id > Display.DEFAULT_DISPLAY, "Android did not allocate an independent display.")
        hideIme(id)

        val activityManager = host.getSystemService(ActivityManager::class.java)
        val intent =
            launcher
                .setComponent(component)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_MULTIPLE_TASK)
        phoneCheck(
            activityManager.isActivityStartAllowedOnDisplay(host, id, intent),
            "Android does not allow this application on the background screen.",
        )
        // A Shizuku UserService has no ActivityManager application thread.
        val options =
            ActivityOptions.makeBasic().apply {
                launchDisplayId = id
                pendingIntentBackgroundActivityStartMode =
                    ActivityOptions.MODE_BACKGROUND_ACTIVITY_START_ALLOWED
            }
        ensureReady(owner)
        val launch =
            PendingIntent.getActivity(
                host.createDisplayContext(virtual.display),
                id,
                intent,
                PendingIntent.FLAG_ONE_SHOT or PendingIntent.FLAG_IMMUTABLE,
            )
        try {
            launch.send(host, 0, null, null, null, null, options.toBundle())
        } finally {
            launch.cancel()
        }

        val taskDisplay = ActivityManager.RunningTaskInfo::class.java.getField("displayId")
        val deadline = SystemClock.uptimeMillis() + 4000
        while (SystemClock.uptimeMillis() < deadline) {
            ensureReady(owner)
            @Suppress("DEPRECATION")
            val top =
                activityManager.getRunningTasks(50).firstOrNull { taskDisplay.getInt(it) == id }
            if (top?.topActivity?.packageName == component.packageName) return id
            SystemClock.sleep(120)
        }
        throw PhoneProblem("Android did not confirm the target app on the background screen.")
    }

    private fun ensureReady(owner: IBinder) {
        phoneCheck(
            !retired &&
                owner.isBinderAlive &&
                host.getSystemService(PowerManager::class.java).isInteractive &&
                !host.getSystemService(KeyguardManager::class.java).isKeyguardLocked,
            "Unlock the phone and keep its screen on to use the background screen.",
        )
    }

    @SuppressLint("BlockedPrivateApi")
    private fun hideIme(displayId: Int) {
        val manager = Class.forName("android.os.ServiceManager")
        val binder =
            manager.getMethod("getService", String::class.java).invoke(null, "window") as IBinder
        val wm =
            Class.forName("android.view.IWindowManager\$Stub")
                .getMethod("asInterface", IBinder::class.java)
                .invoke(null, binder)
        wm.javaClass
            .getMethod(
                "setDisplayImePolicy",
                Int::class.javaPrimitiveType,
                Int::class.javaPrimitiveType,
            )
            .invoke(wm, displayId, 2) // DISPLAY_IME_POLICY_HIDE
    }

    private fun release() {
        runCatching { display?.release() }
        display = null
        runCatching { reader?.close() }
        reader = null
        runCatching { lease?.unlinkToDeath(ownerDied, 0) }
        lease = null
    }

    internal companion object {
        const val DESCRIPTOR = "dev.wingmate.mobile.runtime.phone.background"
        const val OPEN = IBinder.FIRST_CALL_TRANSACTION
        const val CLOSE = IBinder.FIRST_CALL_TRANSACTION + 1
        private const val DESTROY = 16777115
    }
}
