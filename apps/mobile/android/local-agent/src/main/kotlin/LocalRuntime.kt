package dev.wingmate.mobile.runtime

import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import androidx.lifecycle.LiveData
import androidx.lifecycle.MutableLiveData
import dev.wingmate.mobile.runtime.phone.PhoneBridge
import dev.wingmate.mobile.runtime.phone.PhoneControl
import dev.wingmate.mobile.runtime.phone.PhoneGuest
import java.io.File
import java.nio.file.Files
import java.nio.file.StandardCopyOption.ATOMIC_MOVE
import java.nio.file.StandardCopyOption.REPLACE_EXISTING
import java.util.TimeZone
import java.util.concurrent.Executors

sealed interface RuntimeStatus {
    data object Stopped : RuntimeStatus

    data class Installing(val progress: Int) : RuntimeStatus

    data object Starting : RuntimeStatus

    data class Ready(val address: String, val token: String) : RuntimeStatus

    data object Stopping : RuntimeStatus

    data class Failed(val reason: String) : RuntimeStatus
}

/**
 * UI-independent state and process owner for the local runtime. Commands arrive on the main thread.
 */
object LocalRuntime {
    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor()
    private val state = MutableLiveData<RuntimeStatus>(RuntimeStatus.Stopped)
    val status: LiveData<RuntimeStatus> = state
    private val writingBackup = MutableLiveData(false)
    val backupBusy: LiveData<Boolean> = writingBackup
    private var pendingBackup: Runnable? = null
    private val logs = ArrayDeque<String>()
    private var active: Run? = null

    fun start(context: Context) {
        if (writingBackup.value == true) return
        context.startForegroundService(Intent(context, DshService::class.java))
    }

    fun stop(context: Context) {
        context.startService(Intent(context, DshService::class.java).setAction(DshService.STOP))
    }

    fun restart(context: Context) {
        context.startService(Intent(context, DshService::class.java).setAction(DshService.RESTART))
    }

    fun isInstalled(context: Context): Boolean {
        val runtime = File(context.filesDir, "runtime")
        return runtime.resolve("rootfs.version").isFile && runtime.resolve("rootfs").isDirectory
    }

    fun hasBackupData(context: Context): Boolean =
        DshBackup(uninstalledDshHome(context)).hasData() ||
            DshBackup(File(context.filesDir, "runtime/rootfs/root")).hasData()

    class PreparedBackup internal constructor(internal val directory: File, internal val home: File)

    fun prepareBackup(context: Context, source: Uri, completed: (Result<PreparedBackup>) -> Unit) {
        val app = context.applicationContext
        worker.execute {
            val result = runCatching {
                val directory =
                    Files.createTempDirectory(app.cacheDir.toPath(), "dsh-restore-").toFile()
                try {
                    val home =
                        checkNotNull(app.contentResolver.openInputStream(source)).use {
                            DshBackup.unpack(it, directory)
                        }
                    PreparedBackup(directory, home)
                } catch (error: Exception) {
                    runCatching { deleteRuntimeTree(directory) }
                    throw error
                }
            }
            main.post { completed(result) }
        }
    }

    fun discardBackup(backup: PreparedBackup) {
        worker.execute { runCatching { deleteRuntimeTree(backup.directory) } }
    }

    fun exportBackup(context: Context, target: Uri, completed: (Result<String?>) -> Unit) {
        val app = context.applicationContext
        withStoppedEngine(completed) {
            val backup = backupForStoppedEngine(app)
            val resolver = app.contentResolver
            checkNotNull(resolver.openOutputStream(target, "wt")).use(backup::export)
            runCatching {
                resolver
                    .query(target, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                    ?.use { if (it.moveToFirst()) it.getString(0) else null }
            }
                .getOrNull()
        }
    }

    fun discardExport(context: Context, target: Uri) {
        val resolver = context.applicationContext.contentResolver
        worker.execute { runCatching { DocumentsContract.deleteDocument(resolver, target) } }
    }

    fun restoreBackup(context: Context, backup: PreparedBackup, completed: (Result<Unit>) -> Unit) {
        val app = context.applicationContext
        withStoppedEngine(
            { result ->
                discardBackup(backup)
                completed(result)
            },
        ) {
            backupForStoppedEngine(app).restore(backup.home)
        }
    }

    private fun uninstalledDshHome(context: Context) = File(context.filesDir, "restored-dsh")

    private fun backupForStoppedEngine(context: Context): DshBackup {
        val rootfs = File(context.filesDir, "runtime/rootfs")
        // An interrupted engine upgrade can leave the rootfs intact without a version marker.
        val hasRootfs = rootfs.isDirectory
        if (hasRootfs) {
            // A previous process can leave owned tracees behind after the service goes away.
            DshProcess(context, rootfs, Run(context)).prepare()
        }
        val uninstalledHome = uninstalledDshHome(context)
        val restored = DshBackup(uninstalledHome)
        if (!hasRootfs || restored.hasData()) {
            Files.createDirectories(uninstalledHome.toPath())
            return restored
        }
        return DshBackup(File(rootfs, "root"))
    }

    private fun <T> withStoppedEngine(completed: (Result<T>) -> Unit, action: () -> T) {
        if (writingBackup.value == true) {
            completed(
                Result.failure(IllegalStateException("A DSH backup operation is already running")),
            )
            return
        }
        writingBackup.value = true
        pendingBackup = Runnable {
            val result = runCatching(action)
            main.post {
                writingBackup.value = false
                completed(result)
            }
        }
        if (active == null) runPendingBackup() else requestStop()
    }

    private fun runPendingBackup() {
        val action = pendingBackup ?: return
        pendingBackup = null
        worker.execute(action)
    }

    fun logs(): String = synchronized(logs) { logs.joinToString("\n") }

    internal fun launch(context: Context) {
        if (active != null || writingBackup.value == true) return
        synchronized(logs) { logs.clear() }
        val run = Run(context.applicationContext)
        active = run
        state.value = RuntimeStatus.Installing(0)
        worker.execute(run::launch)
    }

    internal fun requestStop(restart: Boolean = false) {
        val run = active ?: return
        run.restart = restart && writingBackup.value != true
        if (run.stopping) return
        run.stopping = true
        state.value = RuntimeStatus.Stopping
        worker.execute { run.stop() }
    }

    internal fun networkChanged() {
        val run = active ?: return
        worker.execute {
            if (run.engine != null && !run.stopping) {
                try {
                    run.refreshNetwork()
                } catch (error: Exception) {
                    run.onLog(error.message.orEmpty(), "network")
                }
            }
        }
    }

    private class Run(private val context: Context) : DshProcess.Listener {
        @Volatile var stopping = false
        var restart = false
        // Accessed by the worker. The main thread owns the active run reference.
        var engine: DshProcess? = null
        private lateinit var rootfs: File
        private var phone: PhoneBridge? = null

        fun launch() {
            try {
                val workspace = SharedWorkspace.prepare(context)
                val manifest = RuntimeManifest.load(context)
                val installer = RuntimeInstaller(context, manifest)
                rootfs = installer.rootfs
                val process = DshProcess(context, rootfs, this)
                process.prepare()
                var lastProgress = -1
                installer.prepare { progress ->
                    if (lastProgress != progress) {
                        lastProgress = progress
                        update(RuntimeStatus.Installing(progress))
                    }
                }
                if (stopping) {
                    finish(RuntimeStatus.Stopped)
                    return
                }
                val backup = DshBackup(File(rootfs, "root"))
                DshBackup(uninstalledDshHome(context)).moveInto(backup)
                backup.recover()
                Files.createDirectories(
                    File(rootfs, SharedWorkspace.guestPath.removePrefix("/")).toPath(),
                )
                refreshNetwork()
                update(RuntimeStatus.Starting)
                startPhone()
                process.start(
                    manifest,
                    workspace,
                    mapOf("TZ" to TimeZone.getDefault().id),
                    phone != null,
                )
                engine = process
            } catch (error: Exception) {
                val message = LogRedactor.redact(error.message ?: error.javaClass.simpleName)
                onLog(message, "system")
                finish(RuntimeStatus.Failed(message))
            }
        }

        private fun startPhone() {
            val control = PhoneControl.get(context)
            try {
                val bridge = PhoneBridge(control, rootfs)
                phone = bridge
                PhoneGuest.write(rootfs, bridge.name, bridge.token)
                bridge.start()
                control.attach(bridge)
            } catch (error: Exception) {
                closePhone()
                onLog("Phone tools unavailable (${error.javaClass.simpleName})", "system")
            }
        }

        fun stop() {
            closePhone()
            engine?.stop()
        }

        private fun closePhone() {
            val bridge = phone ?: return
            phone = null
            bridge.close()
            PhoneControl.get(context).detach(bridge, "The local engine stopped.")
            runCatching { PhoneGuest.clear(rootfs) }
        }

        fun refreshNetwork() {
            val manager = context.getSystemService(ConnectivityManager::class.java)
            val servers = manager.getLinkProperties(manager.activeNetwork)?.dnsServers.orEmpty()
            if (servers.isEmpty()) return
            val config = File(rootfs, "etc/resolv.conf")
            val next = File(config.parentFile, "resolv.conf.next")
            next.writeText(
                servers.joinToString("\n", postfix = "\noptions timeout:2 attempts:2\n") {
                    "nameserver ${it.hostAddress}"
                },
            )
            Files.move(next.toPath(), config.toPath(), ATOMIC_MOVE, REPLACE_EXISTING)
        }

        override fun onLog(line: String, stream: String) {
            synchronized(logs) {
                if (logs.size == 250) logs.removeFirst()
                logs.addLast("[$stream] $line")
            }
        }

        override fun onReady(source: DshProcess, baseUrl: String, token: String) =
            update(RuntimeStatus.Ready(baseUrl, token))

        override fun onExit(source: DshProcess, code: Int, expected: Boolean) {
            worker.execute {
                engine = null
                finish(
                    if (expected) RuntimeStatus.Stopped
                    else RuntimeStatus.Failed(source.startupError ?: "DSH exited with code $code"),
                )
            }
        }

        private fun update(value: RuntimeStatus) {
            main.post {
                if (active === this && !stopping) state.value = value
            }
        }

        private fun finish(value: RuntimeStatus) {
            closePhone()
            main.post {
                if (active === this) {
                    active = null
                    if (restart) launch(context) else state.value = value
                    runPendingBackup()
                }
            }
        }
    }
}
