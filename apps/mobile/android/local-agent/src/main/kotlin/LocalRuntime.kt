package dev.wingmate.mobile.runtime

import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.os.Handler
import android.os.Looper
import androidx.lifecycle.LiveData
import androidx.lifecycle.MutableLiveData
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

/** UI-independent state and process owner for the local runtime. Commands arrive on the main thread. */
object LocalRuntime {
    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor()
    private val state = MutableLiveData<RuntimeStatus>(RuntimeStatus.Stopped)
    val status: LiveData<RuntimeStatus> = state
    private val logs = ArrayDeque<String>()
    private var active: Run? = null

    fun start(context: Context) {
        context.startForegroundService(Intent(context, DshService::class.java))
    }

    fun stop(context: Context) {
        context.startService(Intent(context, DshService::class.java).setAction(DshService.STOP))
    }

    fun logs(): String = synchronized(logs) { logs.joinToString("\n") }

    internal fun launch(context: Context) {
        if (active != null) return
        synchronized(logs) { logs.clear() }
        val run = Run(context.applicationContext)
        active = run
        state.value = RuntimeStatus.Installing(0)
        worker.execute(run::launch)
    }

    internal fun requestStop() {
        val run = active ?: return
        if (run.stopping) return
        run.stopping = true
        state.value = RuntimeStatus.Stopping
        worker.execute { run.engine?.stop() }
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
        // Accessed by the worker. The main thread owns the active run reference.
        var engine: DshProcess? = null
        private lateinit var rootfs: File

        fun launch() {
            try {
                val manifest = RuntimeManifest.load(context)
                val installer = RuntimeInstaller(context, manifest)
                rootfs = installer.rootfs
                val process = DshProcess(context, manifest, rootfs, this)
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
                refreshNetwork()
                update(RuntimeStatus.Starting)
                process.start(mapOf("TZ" to TimeZone.getDefault().id))
                engine = process
            } catch (error: Exception) {
                val message = LogRedactor.redact(error.message ?: error.javaClass.simpleName)
                onLog(message, "system")
                finish(RuntimeStatus.Failed(message))
            }
        }

        fun refreshNetwork() {
            val manager = context.getSystemService(ConnectivityManager::class.java)
            val servers = manager.getLinkProperties(manager.activeNetwork)?.dnsServers.orEmpty()
            if (servers.isEmpty()) return
            val config = File(rootfs, "etc/resolv.conf")
            val next = File(config.parentFile, "resolv.conf.next")
            next.writeText(servers.joinToString("\n", postfix = "\noptions timeout:2 attempts:2\n") {
                "nameserver ${it.hostAddress}"
            })
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
                finish(if (expected) RuntimeStatus.Stopped else
                    RuntimeStatus.Failed(source.startupError ?: "DSH exited with code $code"))
            }
        }

        private fun update(value: RuntimeStatus) {
            main.post {
                if (active === this && !stopping) state.value = value
            }
        }

        private fun finish(value: RuntimeStatus) {
            main.post {
                if (active === this) {
                    active = null
                    state.value = value
                }
            }
        }
    }
}
