package dev.wingmate.mobile.runtime.phone

import android.net.LocalServerSocket
import android.net.LocalSocket
import android.os.Process
import android.system.Os
import android.system.OsConstants
import dev.wingmate.mobile.runtime.phone.PhoneControl.Request
import java.io.ByteArrayOutputStream
import java.io.Closeable
import java.io.File
import java.io.InputStream
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread

// The local socket and first-line credential exchange come from dsh-Go's PhoneBridge (AGPLv3).
internal class PhoneBridge(private val control: PhoneControl, private val dshHome: File) :
    Closeable, PhoneControl.ToolsListener {
    val name = "wingmate.phone.${UUID.randomUUID()}"
    val token = UUID.randomUUID().toString()
    private val server = LocalServerSocket(name)
    private val peers = mutableSetOf<Peer>()
    private var closed = false

    fun start() {
        thread(name = "wingmate-phone-accept", isDaemon = true) {
            while (true) {
                val socket = runCatching { server.accept() }.getOrNull() ?: break
                val peer =
                    synchronized(peers) {
                        if (closed || peers.size >= 4) null else Peer(socket).also(peers::add)
                    }
                if (peer == null) socket.close()
                else thread(name = "wingmate-phone-read", isDaemon = true, block = peer::read)
            }
        }
    }

    override fun toolsChanged() {
        synchronized(peers) { peers.toList() }
            .forEach { peer ->
                if (peer.initialized) peer.send(McpServer.LIST_CHANGED)
            }
    }

    override fun close() {
        val active =
            synchronized(peers) {
                if (closed) return
                closed = true
                peers.toList()
            }
        runCatching { Os.shutdown(server.fileDescriptor, OsConstants.SHUT_RDWR) }
        runCatching { server.close() }
        active.forEach(Peer::close)
    }

    private inner class Peer(private val socket: LocalSocket) {
        private val alive = AtomicBoolean(true)
        private val replies = ArrayBlockingQueue<String>(32)
        private val requests = ConcurrentHashMap<Any, Request>()
        @Volatile
        var initialized = false
            private set

        fun read() {
            try {
                if (socket.peerCredentials.uid != Process.myUid()) return
                socket.soTimeout = 3000
                val input = socket.inputStream.buffered()
                val hello = readLine(input, 512)?.let { Json.parseStrict(it) as? Map<*, *> }
                val credential = hello?.get("token") as? String ?: return
                if (
                    hello["dshPhone"] != 1L ||
                        !MessageDigest.isEqual(token.toByteArray(), credential.toByteArray())
                )
                    return
                socket.soTimeout = 0
                thread(name = "wingmate-phone-write", isDaemon = true) {
                    try {
                        val output = socket.outputStream
                        while (alive.get()) {
                            val line = replies.take()
                            if (!alive.get()) break
                            output.write((line + "\n").toByteArray(Charsets.UTF_8))
                        }
                    } catch (_: Exception) {
                        close()
                    }
                }
                while (alive.get()) {
                    val line = readLine(input, 32 * 1024) ?: break
                    dispatch(line)
                }
            } catch (_: Exception) {
                // The relay can exit or send malformed data while the engine is shutting down.
            } finally {
                close()
            }
        }

        private fun dispatch(line: String) {
            val message = runCatching { Json.parseStrict(line) as? Map<*, *> }.getOrNull()
            if (message == null || message["jsonrpc"] != "2.0") {
                send(McpServer.error(null, -32600, "Invalid request"))
                return
            }
            val id = message["id"]?.takeIf { it is String || it is Long }
            val params = message["params"] as? Map<*, *> ?: emptyMap<String, Any>()
            when (val method = message["method"]) {
                "notifications/initialized" -> initialized = true
                "notifications/cancelled" ->
                    requests.remove(params["requestId"] ?: return)?.let(control::cancel)
                else -> {
                    if (id == null) return
                    when (method) {
                        "initialize" -> send(McpServer.initialize(id, params))
                        "ping" -> send(McpServer.result(id, emptyMap()))
                        "tools/list" -> send(McpServer.list(id, control.enabled, PhoneSkill.tool))
                        "tools/call" -> call(id, params)
                        else -> send(McpServer.error(id, -32601, "Method not found"))
                    }
                }
            }
        }

        private fun call(id: Any, params: Map<*, *>) {
            val tool = params["name"] as? String
            val args =
                if (params.containsKey("arguments")) params["arguments"] as? Map<*, *>
                else emptyMap<String, Any>()
            if (tool == null || args == null || !McpServer.accepts(tool, args, PhoneSkill.tool)) {
                send(McpServer.error(id, -32602, "Unknown phone tool or invalid arguments"))
                return
            }
            lateinit var request: Request
            request =
                Request(
                    McpServer.Call(id, tool, args),
                    if (tool == PhoneSkill.tool.name) {
                        { context -> PhoneSkill.prepare(context, dshHome, args) }
                    } else null,
                ) { result ->
                    if (requests.remove(id, request)) send(McpServer.output(id, result))
                }
            if (requests.size >= 8 || requests.putIfAbsent(id, request) != null) {
                send(McpServer.error(id, -32600, "Duplicate request ID or too many requests"))
                return
            }
            if (alive.get()) control.call(request) else control.cancel(request)
        }

        fun send(line: String) {
            if (alive.get() && !replies.offer(line)) close()
        }

        fun close() {
            if (!alive.compareAndSet(true, false)) return
            synchronized(peers) { peers.remove(this) }
            requests.values.forEach(control::cancel)
            requests.clear()
            replies.clear()
            replies.offer("")
            runCatching { socket.shutdownInput() }
            runCatching { socket.shutdownOutput() }
            runCatching { socket.close() }
        }
    }

    private fun readLine(input: InputStream, limit: Int): String? {
        val bytes = ByteArrayOutputStream()
        while (bytes.size() <= limit) {
            when (val byte = input.read()) {
                -1 -> return null
                '\n'.code -> return bytes.toString("UTF-8")
                else -> bytes.write(byte)
            }
        }
        error("Phone bridge line too long")
    }
}
