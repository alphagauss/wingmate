package dev.wingmate.mobile.runtime.phone

import android.content.ComponentName
import android.content.Context
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.os.Binder
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Parcel
import android.view.Display
import androidx.lifecycle.LiveData
import androidx.lifecycle.MutableLiveData
import java.util.concurrent.Executors
import rikka.shizuku.Shizuku

enum class BackgroundPhoneStatus {
    UNSUPPORTED,
    SHIZUKU_UNAVAILABLE,
    PERMISSION_REQUIRED,
    READY,
    OPENING,
    OPEN,
    CLOSING,
}

// Main-thread owner of a single display lease. Calls into the Shizuku service run on the worker.
internal class BackgroundPhone(private val context: Context) {
    class Target(val displayId: Int, val pkg: String)

    val supported = BackgroundPolicy.supported(Build.VERSION.SDK_INT)
    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newFixedThreadPool(2)
    private val changes = MutableLiveData(Unit)
    val changed: LiveData<Unit> = changes
    private var session: Session? = null
    private var closing = false

    val status: BackgroundPhoneStatus
        get() =
            when {
                !supported -> BackgroundPhoneStatus.UNSUPPORTED
                closing -> BackgroundPhoneStatus.CLOSING
                !shizukuAvailable() -> BackgroundPhoneStatus.SHIZUKU_UNAVAILABLE
                !authorized() -> BackgroundPhoneStatus.PERMISSION_REQUIRED
                session?.target != null -> BackgroundPhoneStatus.OPEN
                session != null -> BackgroundPhoneStatus.OPENING
                else -> BackgroundPhoneStatus.READY
            }

    init {
        if (supported) {
            Shizuku.addBinderReceivedListener { main.post { refresh() } }
            Shizuku.addBinderDeadListener {
                main.post {
                    session?.fail(PhoneProblem("Shizuku disconnected from the background screen."))
                        ?: refresh()
                }
            }
            Shizuku.addRequestPermissionResultListener { request, _ ->
                if (request == PERMISSION_REQUEST) main.post { refresh() }
            }
        }
    }

    private fun refresh() {
        changes.value = Unit
    }

    private fun shizukuAvailable() = runCatching {
        Shizuku.pingBinder() && Shizuku.getVersion() >= 13
    }
        .getOrDefault(false)

    private fun authorized() = runCatching {
        Shizuku.checkSelfPermission() == PackageManager.PERMISSION_GRANTED
    }
        .getOrDefault(false)

    fun requestPermission() {
        if (status == BackgroundPhoneStatus.PERMISSION_REQUIRED)
            runCatching { Shizuku.requestPermission(PERMISSION_REQUEST) }.onFailure { refresh() }
    }

    fun requireCanOpen() {
        requireAccess()
        phoneCheck(
            session == null && !closing,
            "Close the current background screen in Wingmate's Phone Control settings before opening another.",
        )
    }

    fun target(): Target {
        requireAccess()
        return session?.target
            ?: throw PhoneProblem(
                "Use open with background=true to open a background screen first.",
            )
    }

    private fun requireAccess() {
        val issue =
            when {
                !supported -> BackgroundPolicy.REQUIREMENT
                !shizukuAvailable() ->
                    "Install and start Shizuku 13 or later on the phone to use the background screen."
                !authorized() -> "Authorize Shizuku in Wingmate's Phone Control settings."
                else -> return
            }
        if (session != null) close()
        throw PhoneProblem(issue)
    }

    fun matches(target: Target) = session?.target === target && shizukuAvailable() && authorized()

    fun open(component: ComponentName, live: () -> Boolean, complete: (Result<Boolean>) -> Unit) {
        requireCanOpen()
        val args =
            Shizuku.UserServiceArgs(ComponentName(context, BackgroundDisplayService::class.java))
                .tag("wingmate-background-1")
                .processNameSuffix("phone_background")
                .daemon(false)
        val next = Session(args, component, live, complete)
        session = next
        main.postDelayed(next.timeout, 12000)
        refresh()
        try {
            Shizuku.bindUserService(args, next)
        } catch (error: Exception) {
            next.fail(PhoneProblem("Shizuku could not start the background screen service."))
        }
    }

    fun close() {
        val old = session
        if (old == null) {
            refresh()
            return
        }
        session = null
        closing = true
        main.removeCallbacks(old.timeout)
        refresh()
        worker.execute {
            old.remote?.let { runCatching { transact(it, BackgroundDisplayService.CLOSE) } }
            runCatching { Shizuku.unbindUserService(old.args, old, true) }
            main.post {
                closing = false
                refresh()
            }
        }
    }

    private inner class Session(
        val args: Shizuku.UserServiceArgs,
        private val component: ComponentName,
        private val live: () -> Boolean,
        private val complete: (Result<Boolean>) -> Unit,
    ) : ServiceConnection {
        private val lease = Binder()
        var remote: IBinder? = null
        var target: Target? = null
        val timeout = Runnable {
            fail(PhoneProblem("Shizuku did not finish opening the background screen."))
        }

        override fun onServiceConnected(name: ComponentName, binder: IBinder) {
            main.post { connected(binder) }
        }

        private fun connected(binder: IBinder) {
            if (session !== this || remote != null) return
            remote = binder
            if (!live()) {
                close()
                return
            }
            worker.execute {
                val result = runCatching {
                    phoneCheck(live(), "The background screen request was cancelled.")
                    transact(binder, BackgroundDisplayService.OPEN, component, lease)
                }
                main.post {
                    if (session !== this) return@post
                    if (!live()) {
                        close()
                        return@post
                    }
                    val id = result.getOrElse {
                        fail(it)
                        return@post
                    }
                    main.removeCallbacks(timeout)
                    target = Target(id, component.packageName)
                    refresh()
                    complete(Result.success(true))
                }
            }
        }

        override fun onServiceDisconnected(name: ComponentName) {
            main.post { fail(PhoneProblem("Shizuku disconnected from the background screen.")) }
        }

        fun fail(error: Throwable) {
            if (session !== this) return
            close()
            if (target == null) complete(Result.failure(error))
        }
    }

    private fun transact(
        binder: IBinder,
        operation: Int,
        component: ComponentName? = null,
        lease: IBinder? = null,
    ): Int {
        val data = Parcel.obtain()
        val reply = Parcel.obtain()
        try {
            data.writeInterfaceToken(BackgroundDisplayService.DESCRIPTOR)
            if (component != null) {
                data.writeString(component.flattenToString())
                data.writeStrongBinder(lease)
            }
            phoneCheck(
                binder.transact(operation, data, reply, 0),
                "Shizuku rejected the background screen request.",
            )
            reply.readException()
            if (operation == BackgroundDisplayService.CLOSE) return Display.INVALID_DISPLAY
            val id = reply.readInt()
            val error = reply.readString()
            phoneCheck(
                id > Display.DEFAULT_DISPLAY,
                error ?: "Android did not open an independent display.",
            )
            return id
        } finally {
            reply.recycle()
            data.recycle()
        }
    }

    private companion object {
        const val PERMISSION_REQUEST = 4642
    }
}
