package dev.wingmate.mobile.runtime

import android.content.Context
import android.os.Process as AndroidProcess
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import java.io.File
import java.io.IOException
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.nio.file.Files
import java.nio.file.NoSuchFileException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread

/** Runs DSH Web through PRoot and reports ready after receiving its startup token. */
internal class DshProcess(
    private val context: Context,
    private val manifest: RuntimeManifest,
    private val rootfs: File,
    private val listener: Listener,
) {
    interface Listener {
        fun onLog(line: String, stream: String)
        fun onReady(source: DshProcess, baseUrl: String, token: String)
        fun onExit(source: DshProcess, code: Int, expected: Boolean)
    }

    @Volatile private var process: Process? = null
    @Volatile private var stopping = false
    @Volatile private var expectedExit = false
    @Volatile private var readyReported = false
    private val startupSignal = CountDownLatch(1)
    @Volatile private var stopper: Thread? = null
    // Share captured identities across launcher replacement within this app
    // process. Both capture and cleanup use the shared state's monitor.
    private val cleanupState by lazy {
        RuntimeProcessOwnership.cleanupState(rootfs.canonicalPath, AndroidProcess.myUid())
    }
    private val cleanupKnown get() = cleanupState.known
    private var port = DEFAULT_PORT

    @Volatile var startupError: String? = null
        private set

    fun prepare() {
        val cleanup = reapStrays(allowObservationGap = true)
        check(!cleanup.ownedRemaining) { "Previous DSH processes are still active" }
        if (cleanup.canPruneTemp) {
            ProotTempFiles.prune(File(context.cacheDir, "proot-tmp")) { pid -> processReader.uid(pid) == null }
        }
    }

    fun start(guestEnv: Map<String, String>) {
        check(process == null)
        val pidFile = File(rootfs, "tmp/dsh-web.pid")
        pidFile.delete() // also removes a stale/dangling symlink; never reuse a previous run's PID
        check(!java.nio.file.Files.exists(pidFile.toPath(), java.nio.file.LinkOption.NOFOLLOW_LINKS)) { "cannot remove stale runtime pid file" }
        port = pickPort()
        val tmp = File(context.cacheDir, "proot-tmp").apply { mkdirs() }
        val vars = mapOf(
            "{NATIVE_LIB_DIR}" to context.applicationInfo.nativeLibraryDir,
            "{ROOTFS}" to rootfs.canonicalPath,
            "{TMP}" to tmp.absolutePath,
            "{PORT}" to port.toString(),
        )
        fun expand(s: String) = vars.entries.fold(s) { acc, (k, v) -> acc.replace(k, v) }

        val effectiveGuestEnv = guestEnv + ("DSH_GO_RUNTIME_ROOT" to rootfs.canonicalPath)
        val argv = manifest.launchArgv.flatMap { arg ->
            if (arg == GUEST_ENV) effectiveGuestEnv.map { (key, value) -> "$key=$value" } else listOf(expand(arg))
        }

        val builder = ProcessBuilder(argv).directory(context.filesDir)
        val env = builder.environment()
        env.clear()
        env["PATH"] = "/system/bin:/system/xbin"
        env["HOME"] = context.filesDir.absolutePath
        env["TMPDIR"] = tmp.absolutePath
        env["ANDROID_ROOT"] = System.getenv("ANDROID_ROOT") ?: "/system"
        env["ANDROID_DATA"] = System.getenv("ANDROID_DATA") ?: "/data"
        manifest.launchEnv.forEach { (k, v) -> env[k] = expand(v) }
        listener.onLog("Starting DSH Web on 127.0.0.1:$port", "system")
        val p = builder.start()
        process = p
        val out = pump(p.inputStream.bufferedReader(), "stdout")
        val err = pump(p.errorStream.bufferedReader(), "stderr")
        thread(name = "dsh-wait", isDaemon = true) {
            val code = try { p.waitFor() } catch (_: InterruptedException) { -1 }
            startupSignal.countDown()
            // The fatal message is usually the last output: let the pumps drain it before reporting.
            try {
                out.join(2000)
                err.join(2000)
            } catch (_: InterruptedException) {
            }
            // Report after a running stop() has finished: its killStrays() must not hit the next launch.
            val stopThread = synchronized(this) {
                process = null
                stopper
            }
            stopThread?.join()
            // An unexpected parent exit can leave tracees behind too. Keep the
            // controller's ownership until this UID sweep has finished.
            if (stopThread == null) killStrays()
            listener.onExit(this, code, expectedExit)
        }
        // The port deadline starts only after a token. A stuck loader/Node can emit nothing.
        thread(name = "dsh-start-deadline", isDaemon = true) {
            if (startupSignal.await(TOKEN_TIMEOUT_MS, TimeUnit.MILLISECONDS)) return@thread
            synchronized(this) {
                // Token detection and stop share this lock; a late deadline cannot retire either.
                if (process === p && p.isAlive && !readyReported && !stopping) {
                    failStart("no startup token received within ${TOKEN_TIMEOUT_MS / 1000}s")
                }
            }
        }
    }

    @Synchronized private fun failStart(message: String) {
        if (stopping) return
        startupError = message
        listener.onLog(message, "system")
        stop(expected = false)
    }

    /**
     * proot ignores SIGTERM, so stop DSH itself first (pid file written by the launch
     * command), then SIGQUIT proot, which kills every remaining tracee.
     */
    @Synchronized fun stop(expected: Boolean = true) {
        stopping = true
        startupSignal.countDown()
        if (expected) expectedExit = true
        if (stopper != null) return
        val p = process ?: return
        stopper = thread(name = "dsh-stop", isDaemon = true) {
            try {
                val node = if (p.isAlive) guestRecord() else null
                val proot = node?.let { child -> processSnapshot().records.firstOrNull { it.pid == child.parent && it.proot } }
                if (node != null) signalOwned(node, OsConstants.SIGTERM)
                if (!waitExit(p, 5000) && proot != null) signalOwned(proot, OsConstants.SIGQUIT)
                waitExit(p, 2000)
            } finally {
                try {
                    if (p.isAlive) p.destroyForcibly()
                } finally {
                    // Reap tracees before waiting: an OEM Process.waitFor may not
                    // return until they exit. The waiter still retains this owner.
                    killStrays()
                    p.waitFor()
                }
            }
        }
    }

    private fun waitExit(p: Process, millis: Long): Boolean {
        val deadline = System.nanoTime() + millis * 1_000_000L
        while (p.isAlive && System.nanoTime() < deadline) Thread.sleep(100)
        return !p.isAlive
    }

    /** Guest `dsh web` identity, retained through the final pre-signal PID-reuse check. */
    @Synchronized private fun guestRecord(): RuntimeProcessRecord? = synchronized(cleanupState) {
        captureGuestRecord()
    }

    private fun captureGuestRecord(): RuntimeProcessRecord? {
        return try {
            val pid = File(rootfs, "tmp/dsh-web.pid").takeIf { it.isFile }?.readText()?.trim()?.toIntOrNull() ?: return null
            val records = processSnapshot().records
            val owned = RuntimeProcessOwnership.owned(records, setOf(rootfs.absolutePath, rootfs.canonicalPath), AndroidProcess.myPid())
            owned.forEach { cleanupKnown[it.pid] = it.started }
            val record = owned.firstOrNull { it.pid == pid } ?: return null
            if (record.argv.any { it.contains("/opt/node") || it.contains("dsh") }) record else null
        } catch (_: Exception) {
            null
        }
    }

    private fun signalOwned(record: RuntimeProcessRecord, value: Int) = runCatching {
        val current = RuntimeProcessOwnership.stat(File("/proc/${record.pid}/stat").readText())
        if (current?.second == record.started && Os.stat("/proc/${record.pid}").st_uid == AndroidProcess.myUid()) {
            Os.kill(record.pid, value)
        }
    }

    private fun pump(reader: java.io.BufferedReader, stream: String) = thread(name = "dsh-$stream", isDaemon = true) {
        val redactor = LogRedactor()
        try {
            reader.useLines { lines ->
                lines.forEach { line ->
                    listener.onLog(LogRedactor.redact(redactor.line(line)), stream)
                    if (stream == "stdout" && !readyReported) detectToken(line)
                }
            }
        } catch (_: Exception) {
        }
    }

    @Synchronized private fun detectToken(line: String) {
        if (readyReported || stopping) return
        val match = TOKEN_URL.find(line) ?: return
        if (match.groupValues[1].toIntOrNull() != port) return
        val token = match.groupValues[2]
        readyReported = true
        startupSignal.countDown()
        val base = "http://127.0.0.1:$port"
        thread(name = "dsh-ready", isDaemon = true) {
            val deadline = System.currentTimeMillis() + 90_000
            while (System.currentTimeMillis() < deadline && !stopping && process?.isAlive == true) {
                if (canConnect(port)) {
                    listener.onReady(this, base, token)
                    return@thread
                }
                Thread.sleep(250)
            }
            if (!stopping && process?.isAlive == true) {
                failStart("token seen but port $port never accepted connections")
            }
        }
    }

    /** Reads only this app's uid. Environment values are used for ownership, never logged. */
    private val processReader = object : RuntimeProcessReader {
        override fun pids(): List<Int> = (File("/proc").listFiles() ?: throw IOException("cannot enumerate /proc"))
            .mapNotNull { it.name.toIntOrNull() }
        override fun uid(pid: Int): Int? = try {
            Os.stat("/proc/$pid").st_uid
        } catch (error: ErrnoException) {
            if (error.errno == OsConstants.ENOENT || error.errno == OsConstants.ESRCH) null else throw error
        } catch (_: NoSuchFileException) { null }
        private fun text(pid: Int, name: String): String = Files.readAllBytes(File("/proc/$pid/$name").toPath()).toString(Charsets.UTF_8)
        override fun stat(pid: Int): String = text(pid, "stat")
        override fun command(pid: Int): String = text(pid, "cmdline")
        override fun environment(pid: Int): String = text(pid, "environ")
        override fun cwd(pid: Int): String = Os.readlink("/proc/$pid/cwd")
    }

    private fun processSnapshot(known: Set<Int> = emptySet()): RuntimeProcessSnapshot =
        RuntimeProcessOwnership.scan(processReader, AndroidProcess.myUid(), AndroidProcess.myPid(), known)

    private fun pidHint(): Int? = try {
        File(rootfs, "tmp/dsh-web.pid").readText().trim().toIntOrNull()?.takeIf { it > 0 }
    } catch (_: Exception) { null }

    private fun remember(snapshot: RuntimeProcessSnapshot) {
        snapshot.retired.forEach { cleanupKnown.remove(it) }
        for (record in snapshot.records) {
            if (cleanupKnown[record.pid]?.let { it != record.started } == true) cleanupKnown.remove(record.pid)
        }
    }

    /** Reap a captured process tree before releasing #17 ownership to another launch. */
    private fun killStrays() { reapStrays() }

    private data class CleanupResult(val ownedRemaining: Boolean, val canPruneTemp: Boolean = false)

    /** Unknown ambient processes are not proof of ownership. The default-mode
     * startup policy may continue after reaping every captured identity, while
     * killStrays itself still reports the observation as incomplete. */
    @Synchronized private fun reapStrays(allowObservationGap: Boolean = false): CleanupResult = synchronized(cleanupState) {
        reapStraysLocked(allowObservationGap)
    }

    private fun reapStraysLocked(allowObservationGap: Boolean): CleanupResult {
        val myPid = AndroidProcess.myPid()
        val roots = setOf(rootfs.absolutePath, rootfs.canonicalPath)
        val hinted = pidHint()?.let { setOf(it) }.orEmpty()
        val deadline = System.nanoTime() + 2_000_000_000L
        var first = true
        while (true) {
            val snapshot = processSnapshot(cleanupKnown.keys + hinted)
            remember(snapshot)
            val owned = RuntimeProcessOwnership.owned(snapshot.records, roots, myPid, cleanupKnown)
            owned.forEach { cleanupKnown[it.pid] = it.started }
            val ownedRemaining = owned.isNotEmpty() || cleanupKnown.isNotEmpty()
            if (!ownedRemaining && snapshot.observed) {
                // This cache is shared across rootfs instances. Another engine may still use its glue.
                val temp = File(context.cacheDir, "proot-tmp").absolutePath
                val idle = snapshot.records.none { it.proot || it.marker != null || RuntimeProcessOwnership.within(it.cwd, temp) }
                return CleanupResult(ownedRemaining = false, canPruneTemp = idle)
            }
            owned.filter { it.proot }.forEach { signalOwned(it, if (first) OsConstants.SIGQUIT else OsConstants.SIGKILL) }
            owned.filterNot { it.proot }.forEach { signalOwned(it, OsConstants.SIGKILL) }
            first = false
            if ((!ownedRemaining && allowObservationGap) || System.nanoTime() >= deadline) {
                listener.onLog("[wingmate] cleanup incomplete: ${owned.size} owned process(es) remain; " +
                    "${snapshot.uncertain.size} process observation(s) unavailable" +
                    if (snapshot.enumerationFailed) "; /proc enumeration unavailable" else "", "system")
                return CleanupResult(ownedRemaining)
            }
            Thread.sleep(25)
        }
    }

    companion object {
        private const val DEFAULT_PORT = 3080
        private const val GUEST_ENV = "{GUEST_ENV}"
        // Bootstrap budget (not a device measurement); token-to-port retains its own 90 seconds.
        private const val TOKEN_TIMEOUT_MS = 180_000L
        private val TOKEN_URL = Regex("""^dsh web: http://127\.0\.0\.1:(\d+)/\?token=([A-Za-z0-9_-]+)""")

        private fun canConnect(port: Int): Boolean = try {
            Socket().use { it.connect(InetSocketAddress("127.0.0.1", port), 500); true }
        } catch (_: Exception) {
            false
        }

        private fun pickPort(): Int {
            for (candidate in DEFAULT_PORT..DEFAULT_PORT + 20) {
                try {
                    ServerSocket(candidate, 1, InetAddress.getByName("127.0.0.1")).close()
                    return candidate
                } catch (_: Exception) {
                }
            }
            return ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).use { it.localPort }
        }
    }
}

/** One sync(2): finished copies and renames are on disk before anything depends on them. */
internal fun syncFilesystems() {
    val sync = ProcessBuilder("/system/bin/sync").redirectErrorStream(true).start()
    sync.inputStream.use { it.readBytes() }
    val code = sync.waitFor()
    if (code != 0) throw IOException("sync exited with $code")
}
