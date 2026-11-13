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
import java.util.concurrent.Callable
import java.util.concurrent.ExecutorCompletionService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

internal data class ConnectionTarget(val origin: String, val link: String) {
    companion object {
        fun parse(raw: String, manual: Boolean = false): ConnectionTarget? {
            if (raw.length > 8192) return null
            val input = raw.trim()
            val address = if (input.startsWith('{')) {
                (runCatching { JSONObject(input).opt("url") }.getOrNull() as? String)?.trim() ?: return null
            } else input
            if (address.length > 8192 || address.any { it.isWhitespace() || it.isISOControl() }) return null

            // The gateway serves LAN IPv4 over HTTP and public hostnames over HTTPS.
            val prefix = when {
                "://" in address -> ""
                address.substringBefore('/').substringBefore('?').matches(Regex("[A-Za-z0-9_.-]+:[0-9]+")) -> "http://"
                manual -> "https://"
                else -> return null
            }
            val url = runCatching { URI(prefix + address) }.getOrNull() ?: return null
            val host = url.host?.lowercase() ?: return null
            if (url.rawUserInfo != null || url.rawFragment != null || ':' in host ||
                url.rawPath.orEmpty().removePrefix("/").isNotEmpty() ||
                url.port !in -1..65535 || url.port == 0
            ) return null
            val lan = host.split('.').let { parts -> parts.size == 4 && parts.all { it.toIntOrNull() in 0..255 } }
            val scheme = if (lan) "http" else "https"
            if (!scheme.equals(url.scheme, ignoreCase = true)) return null
            val query = url.rawQuery
            if (query != null && (!query.startsWith("auth=") || query.length == 5 || '&' in query)) return null
            val defaultPort = if (lan) 80 else 443
            val origin = if (url.port == -1 || url.port == defaultPort) "$scheme://$host" else "$scheme://$host:${url.port}"
            return ConnectionTarget(origin, "$origin/" + if (query == null) "" else "?$query")
        }
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

internal class NoLanNetwork : Exception()

internal fun discoverConnection(
    context: Context,
    history: List<SavedConnection>,
    historyOnly: Boolean
): String? {
    if (historyOnly) {
        return history.firstOrNull()?.origin?.takeIf { probeConnection(it) }
    }
    val parallel = 40
    val workers = Executors.newFixedThreadPool(parallel)
    try {
        val known = workers.invokeAll(history.map { entry ->
            Callable { entry.origin.takeIf { probeConnection(it) } }
        }, 3, TimeUnit.SECONDS)
        known.firstNotNullOfOrNull { if (it.isCancelled) null else it.get() }?.let { return it }
        val network = context.getSystemService(ConnectivityManager::class.java)
        val active = network.activeNetwork ?: throw NoLanNetwork()
        val capabilities = network.getNetworkCapabilities(active) ?: throw NoLanNetwork()
        if (!capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) &&
            !capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
        ) throw NoLanNetwork()
        val ip = network.getLinkProperties(active)?.linkAddresses
            ?.firstOrNull { it.address is Inet4Address }?.address?.hostAddress ?: throw NoLanNetwork()
        val ports = history.map { URI(it.origin) }.filter { it.scheme == "http" }
            .map { it.port.takeIf { port -> port > 0 } ?: 80 }
        val subnet = ip.substringBeforeLast('.')
        val addresses = (ports + listOf(3088, 3080)).distinct().asSequence().flatMap { port ->
            (1..254).asSequence().map { "$subnet.$it" }.filterNot { it == ip }.map { "http://$it:$port" }
        }.iterator()
        val results = ExecutorCompletionService<String?>(workers)
        fun probeNext() {
            val address = addresses.next()
            results.submit(Callable { address.takeIf { probeConnection(it, 600) } })
        }
        repeat(parallel) { probeNext() }
        var inFlight = parallel
        val started = System.nanoTime()
        val budget = TimeUnit.SECONDS.toNanos(17)
        while (inFlight > 0) {
            val remaining = budget - (System.nanoTime() - started)
            if (remaining <= 0) break
            val completed = results.poll(remaining, TimeUnit.NANOSECONDS) ?: break
            completed.get()?.let { return it }
            if (addresses.hasNext()) probeNext() else inFlight--
        }
        return null
    } finally {
        workers.shutdownNow()
    }
}
