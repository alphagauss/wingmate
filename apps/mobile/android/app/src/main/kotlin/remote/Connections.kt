package dev.wingmate.mobile.remote

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.Inet4Address
import java.net.Proxy
import java.net.URI
import java.util.concurrent.ExecutorCompletionService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

internal data class ConnectionTarget(val origin: String, val link: String) {
    companion object {
        private val shortAddress = Regex("""[\w.-]+:\d+(?:[/?].*)?""")
        private val ipv4 = Regex("""(?:\d{1,3}\.){3}\d{1,3}""")

        // Ported from apps/native/src/utils/bridge-protocol.ts with Wingmate's gateway rules.
        fun parse(raw: String, manual: Boolean = false): ConnectionTarget? = runCatching {
            if (raw.length > 8192) return null
            var text = raw.trim()
            if (text.startsWith('{')) text = JSONObject(text).getString("url").trim()
            if (shortAddress.matches(text)) text = "http://$text"
            else if (manual && "://" !in text) text = "https://$text"
            if (text.any { it.isWhitespace() || it == '\\' || it.isISOControl() }) return null
            val uri = URI(text)
            val scheme = uri.scheme?.lowercase()
            val host = uri.host?.lowercase() ?: return null
            val numeric = ipv4.matches(host)
            if ((scheme != "http" && scheme != "https") || (scheme == "http") != numeric ||
                host.startsWith('[') || uri.userInfo != null || uri.rawFragment != null ||
                uri.rawPath !in listOf("", "/") || uri.port == 0 || uri.port > 65535 ||
                uri.rawQuery?.matches(Regex("auth=[^&]+")) == false
            ) return null
            val defaultPort = if (scheme == "http") 80 else 443
            val port = uri.port.takeIf { it != -1 && it != defaultPort }
            val origin = "$scheme://$host" + if (port == null) "" else ":$port"
            ConnectionTarget(origin, "$origin/" + (uri.rawQuery?.let { "?$it" } ?: ""))
        }.getOrNull()
    }
}

internal data class SavedConnection(val origin: String, val name: String = "") {
    val label: String get() = name.ifEmpty { URI(origin).rawAuthority }
}

internal class ConnectionStore(context: Context) {
    private val preferences = context.getSharedPreferences("connection", Context.MODE_PRIVATE)

    fun entries(): List<SavedConnection> = synchronized(preferences) {
        val saved = JSONArray(preferences.getString("history", "[]"))
        List(saved.length()) { index ->
            val entry = saved.getJSONObject(index)
            SavedConnection(entry.getString("origin"), entry.optString("name"))
        }
    }

    fun remember(origin: String) = update { entries ->
        listOf(entries.firstOrNull { it.origin == origin } ?: SavedConnection(origin)) +
            entries.filterNot { it.origin == origin }
    }

    fun rename(origin: String, name: String) = update { entries ->
        entries.map { if (it.origin == origin) it.copy(name = name.trim()) else it }
    }

    fun forget(origin: String) = update { entries -> entries.filterNot { it.origin == origin } }

    fun migrate(origins: JSONArray) = update { entries ->
        (entries + (0 until origins.length()).mapNotNull {
            ConnectionTarget.parse(origins.optString(it))?.let { target -> SavedConnection(target.origin) }
        }).distinctBy { it.origin }
    }

    private fun update(change: (List<SavedConnection>) -> List<SavedConnection>) = synchronized(preferences) {
        val updated = JSONArray()
        change(entries()).take(20).forEach {
            updated.put(JSONObject().put("origin", it.origin).put("name", it.name))
        }
        preferences.edit().putString("history", updated.toString()).apply()
    }
}

internal fun probeConnection(origin: String, timeout: Int = 3000): Boolean = runCatching {
    val target = URI(origin)
    val lan = target.scheme == "http"
    val endpoint = target.resolve("/_wingmate/identity").toURL()
    val connection = (if (lan) endpoint.openConnection(Proxy.NO_PROXY) else endpoint.openConnection()) as HttpURLConnection
    try {
        connection.instanceFollowRedirects = false
        connection.connectTimeout = timeout
        connection.readTimeout = timeout
        if (connection.responseCode != HttpURLConnection.HTTP_OK) return false
        val body = connection.inputStream.reader(Charsets.UTF_8).use { reader ->
            val buffer = CharArray(512)
            var size = 0
            while (size < buffer.size) {
                val read = reader.read(buffer, size, buffer.size - size)
                if (read < 0) break
                size += read
            }
            String(buffer, 0, size)
        }
        val identity = JSONObject(body)
        identity.optString("product") == "wingmate" &&
            identity.optString("transport") == if (lan) "lan" else "public"
    } finally {
        connection.disconnect()
    }
}.getOrDefault(false)

// Same /24 range, default ports, concurrency and deadline as upstream discovery.
internal class NoLanNetwork : Exception()

internal fun discoverConnection(
    context: Context,
    history: List<SavedConnection>,
    historyOnly: Boolean,
    skip: String?
): String? {
    val workers = Executors.newFixedThreadPool(40)
    try {
        val previous = history.filterNot { it.origin == skip }
        val known = workers.invokeAll(previous.map { entry ->
            java.util.concurrent.Callable { entry.origin.takeIf { probeConnection(it) } }
        }, 3, TimeUnit.SECONDS)
        known.firstNotNullOfOrNull { if (it.isCancelled) null else it.get() }?.let { return it }
        if (historyOnly) return null

        val network = context.getSystemService(ConnectivityManager::class.java)
        val active = network.activeNetwork ?: throw NoLanNetwork()
        val capabilities = network.getNetworkCapabilities(active) ?: throw NoLanNetwork()
        if (!capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) &&
            !capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
        ) throw NoLanNetwork()
        val ip = network.getLinkProperties(active)?.linkAddresses
            ?.firstOrNull { it.address is Inet4Address }?.address?.hostAddress ?: throw NoLanNetwork()
        val subnet = ip.substringBeforeLast('.')
        val ports = (history.map { URI(it.origin) }.filter { it.scheme == "http" }.map { it.port } +
            listOf(3088, 3080)).filter { it > 0 }.distinct()
        val candidates = ports.flatMap { port ->
            (1..254).map { "$subnet.$it" }.filterNot { it == ip }.map { "http://$it:$port" }
        }.filterNot { it == skip }
        val cursor = AtomicInteger()
        val results = ExecutorCompletionService<String?>(workers)
        repeat(minOf(40, candidates.size)) {
            results.submit(java.util.concurrent.Callable {
                while (!Thread.currentThread().isInterrupted) {
                    val origin = candidates.getOrNull(cursor.getAndIncrement()) ?: break
                    if (probeConnection(origin, 600)) return@Callable origin
                }
                null
            })
        }
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(17)
        repeat(minOf(40, candidates.size)) {
            val result = results.poll(deadline - System.nanoTime(), TimeUnit.NANOSECONDS) ?: return null
            result.get()?.let { return it }
        }
        return null
    } finally {
        workers.shutdownNow()
    }
}
