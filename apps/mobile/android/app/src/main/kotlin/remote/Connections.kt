package dev.wingmate.mobile.remote

import android.content.Context
import android.content.pm.ApplicationInfo
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.webkit.CookieManager
import java.net.HttpURLConnection
import java.net.Inet4Address
import java.net.Proxy
import java.net.URI
import java.util.concurrent.Callable
import java.util.concurrent.ExecutorCompletionService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import org.json.JSONArray
import org.json.JSONObject

internal data class ConnectionTarget(val origin: String, val link: String) {
    companion object {
        fun parse(raw: String, manual: Boolean = false): ConnectionTarget? {
            if (raw.length > 8192) return null
            val input = raw.trim()
            val address =
                if (input.startsWith('{')) {
                    (runCatching { JSONObject(input).opt("url") }.getOrNull() as? String)?.trim()
                        ?: return null
                } else input
            if (address.length > 8192 || address.any { it.isWhitespace() || it.isISOControl() })
                return null

            // The gateway serves LAN IPv4 over HTTP and public hostnames over HTTPS.
            val prefix =
                when {
                    "://" in address -> ""
                    address
                        .substringBefore('/')
                        .substringBefore('?')
                        .matches(Regex("[A-Za-z0-9_.-]+:[0-9]+")) -> "http://"
                    manual -> "https://"
                    else -> return null
                }
            val url = runCatching { URI(prefix + address) }.getOrNull() ?: return null
            val host = url.host?.lowercase() ?: return null
            if (
                url.rawUserInfo != null ||
                    url.rawFragment != null ||
                    ':' in host ||
                    url.rawPath.orEmpty().removePrefix("/").isNotEmpty() ||
                    url.port !in -1..65535 ||
                    url.port == 0
            )
                return null
            val lan =
                host.split('.').let { parts ->
                    parts.size == 4 && parts.all { it.toIntOrNull() in 0..255 }
                }
            val scheme = if (lan) "http" else "https"
            if (!scheme.equals(url.scheme, ignoreCase = true)) return null
            val query = url.rawQuery
            if (query != null && (!query.startsWith("auth=") || query.length == 5 || '&' in query))
                return null
            val defaultPort = if (lan) 80 else 443
            val origin =
                if (url.port == -1 || url.port == defaultPort) "$scheme://$host"
                else "$scheme://$host:${url.port}"
            return ConnectionTarget(origin, "$origin/" + if (query == null) "" else "?$query")
        }
    }
}

internal data class SavedConnection(val origin: String, val name: String = "") {
    val label: String
        get() = name.ifEmpty { URI(origin).rawAuthority }
}

internal class ConnectionStore(context: Context) {
    private val preferences = context.getSharedPreferences("connection", Context.MODE_PRIVATE)

    fun entries(): List<SavedConnection> =
        synchronized(preferences) {
            val saved = JSONArray(preferences.getString("history", "[]"))
            List(saved.length()) { index ->
                val entry = saved.getJSONObject(index)
                SavedConnection(entry.getString("origin"), entry.optString("name"))
            }
        }

    fun remember(origin: String, previousOrigin: String) = update { entries ->
        val entry =
            entries.firstOrNull { it.origin == previousOrigin && it.name.isNotBlank() }
                ?: entries.firstOrNull { it.origin == origin }
        listOf(SavedConnection(origin, entry?.name.orEmpty())) +
            entries.filterNot { it.origin == origin || it.origin == previousOrigin }
    }

    fun rename(origin: String, name: String) = update { entries ->
        entries.map { if (it.origin == origin) it.copy(name = name.trim()) else it }
    }

    fun forget(origin: String) = update { entries -> entries.filterNot { it.origin == origin } }

    private fun update(change: (List<SavedConnection>) -> List<SavedConnection>) =
        synchronized(preferences) {
            val updated = JSONArray()
            change(entries()).take(20).forEach {
                updated.put(JSONObject().put("origin", it.origin).put("name", it.name))
            }
            preferences.edit().putString("history", updated.toString()).apply()
        }
}

internal data class HostIdentity(val name: String, val password: Boolean)

internal data class DiscoveredHost(
    val origin: String,
    val identity: HostIdentity,
    val previousOrigin: String = origin,
) {
    val target: ConnectionTarget
        get() = ConnectionTarget(origin, "$origin/")
}

internal fun inspectConnection(origin: String, timeout: Int = 3000): HostIdentity? = runCatching {
    val target = URI(origin)
    val lan = target.scheme == "http"
    val endpoint = target.resolve("/_wingmate").toURL()
    val connection =
        (if (lan) endpoint.openConnection(Proxy.NO_PROXY) else endpoint.openConnection())
            as HttpURLConnection
    try {
        connection.instanceFollowRedirects = false
        connection.connectTimeout = timeout
        connection.readTimeout = timeout
        if (connection.responseCode != HttpURLConnection.HTTP_OK) return null
        val body =
            connection.inputStream.bufferedReader(Charsets.UTF_8).use { reader ->
                val buffer = CharArray(1024)
                var size = 0
                while (size < buffer.size) {
                    val read = reader.read(buffer, size, buffer.size - size)
                    if (read < 0) break
                    size += read
                }
                String(buffer, 0, size)
            }
        val identity = JSONObject(body)
        if (
            identity.optString("product") != "wingmate" ||
                identity.optString("transport") != (if (lan) "lan" else "public")
        )
            return null
        HostIdentity(identity.optString("name"), lan && identity.optBoolean("password"))
    } finally {
        connection.disconnect()
    }
}
    .getOrNull()

internal fun probeConnection(origin: String, timeout: Int = 3000) =
    inspectConnection(origin, timeout) != null

internal fun resolveConnection(
    context: Context,
    target: ConnectionTarget,
    timeout: Int = 3000,
): DiscoveredHost? {
    inspectConnection(target.origin, timeout)?.let {
        return DiscoveredHost(target.origin, it)
    }
    if (target.link != "${target.origin}/") return null
    val uri = URI(target.origin)
    if (uri.scheme != "http" || !privateIpv4(uri.host)) return null
    val debug = context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0
    for (port in lanPorts(debug)) {
        if (port == uri.port) continue
        val origin = "http://${uri.host}:$port"
        inspectConnection(origin, 650)?.let {
            return DiscoveredHost(origin, it, target.origin)
        }
    }
    return null
}

private fun lanRequest(origin: String, path: String): HttpURLConnection =
    (URI(origin).resolve(path).toURL().openConnection(Proxy.NO_PROXY) as HttpURLConnection).apply {
        instanceFollowRedirects = false
        connectTimeout = 3500
        readTimeout = 4500
    }

internal fun sessionCookieName(origin: String): String {
    val uri = URI(origin)
    return if (uri.scheme == "https") "wingmate_public"
    else "wingmate_share_${uri.port.takeIf { it > 0 } ?: 80}"
}

internal fun hasLanSession(origin: String): Boolean {
    val expected = "${sessionCookieName(origin)}="
    val cookie =
        CookieManager.getInstance().getCookie(origin)?.split(';')?.map(String::trim)?.firstOrNull {
            it.startsWith(expected)
        } ?: return false
    val connection = lanRequest(origin, "/_wingmate/session")
    return try {
        connection.setRequestProperty("Cookie", cookie)
        connection.responseCode == HttpURLConnection.HTTP_NO_CONTENT
    } finally {
        connection.disconnect()
    }
}

internal data class PasswordReply(val status: Int, val cookie: String?)

internal fun submitLanPassword(origin: String, password: String): PasswordReply {
    val connection = lanRequest(origin, "/_wingmate/login")
    return try {
        val body = password.toByteArray(Charsets.UTF_8)
        connection.requestMethod = "POST"
        connection.setRequestProperty("Origin", origin)
        connection.doOutput = true
        connection.setFixedLengthStreamingMode(body.size)
        connection.outputStream.use { it.write(body) }
        val expected = "${sessionCookieName(origin)}="
        val cookie =
            connection
                .getHeaderField("Set-Cookie")
                ?.substringBefore(';')
                ?.takeIf {
                    it.startsWith(expected) &&
                        it.removePrefix(expected).matches(Regex("[a-fA-F0-9-]{36}"))
                }
                ?.let { "$it; Path=/; Max-Age=2592000; HttpOnly; SameSite=Strict" }
        PasswordReply(connection.responseCode, cookie)
    } finally {
        connection.disconnect()
    }
}

internal class NoLanNetwork : Exception()

private fun lanPorts(debug: Boolean) = if (debug) 8746..8749 else 8746..8748

private fun privateIpv4(host: String): Boolean {
    val octets = host.split('.')
    val first = octets[0].toIntOrNull()
    val second = octets.getOrNull(1)?.toIntOrNull()
    return first == 10 ||
        (first == 172 && second != null && second in 16..31) ||
        (first == 192 && second == 168)
}

internal fun lanCandidates(ip: String, debug: Boolean): Sequence<String> {
    val subnet = ip.substringBeforeLast('.')
    return (1..254)
        .asSequence()
        .map { "$subnet.$it" }
        .filterNot { it == ip }
        .flatMap { host ->
            lanPorts(debug).asSequence().map { "http://$host:$it" }
        }
}

internal fun discoverConnections(
    context: Context,
    history: List<SavedConnection>,
    historyOnly: Boolean,
    onFound: (DiscoveredHost) -> Unit = {},
): List<DiscoveredHost> {
    if (historyOnly) {
        val target = history.firstOrNull()?.origin?.let { ConnectionTarget.parse(it) }
        return listOfNotNull(target?.let { resolveConnection(context, it) })
    }
    val parallel = 40
    val workers = Executors.newFixedThreadPool(parallel)
    try {
        val historyAddresses = history.map { it.origin }.filter { URI(it).scheme == "http" }.toSet()
        val known =
            workers.invokeAll(
                historyAddresses.map { origin ->
                    Callable {
                        inspectConnection(origin)?.let { DiscoveredHost(origin, it).also(onFound) }
                    }
                },
                3,
                TimeUnit.SECONDS,
            )
        val found = known.mapNotNull { if (it.isCancelled) null else it.get() }.toMutableList()
        val network = context.getSystemService(ConnectivityManager::class.java)
        val active =
            network.activeNetwork?.takeIf { candidate ->
                network.getNetworkCapabilities(candidate)?.let {
                    it.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) ||
                        it.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
                } == true
            }
        val ip =
            active
                ?.let { network.getLinkProperties(it) }
                ?.linkAddresses
                ?.firstOrNull { it.address is Inet4Address }
                ?.address
                ?.hostAddress
        if (ip == null) return found.ifEmpty { throw NoLanNetwork() }
        val debug = context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0
        val addresses =
            lanCandidates(ip, debug).filterNot { it in historyAddresses }.withIndex().iterator()
        val results = ExecutorCompletionService<IndexedValue<DiscoveredHost>?>(workers)
        fun probeNext() {
            val address = addresses.next()
            results.submit(
                Callable {
                    inspectConnection(address.value, 600)?.let {
                        IndexedValue(address.index, DiscoveredHost(address.value, it))
                    }
                },
            )
        }
        var inFlight = 0
        while (inFlight < parallel && addresses.hasNext()) {
            probeNext()
            inFlight++
        }
        val discovered = mutableListOf<IndexedValue<DiscoveredHost>>()
        val started = System.nanoTime()
        val budget = TimeUnit.SECONDS.toNanos(17)
        while (inFlight > 0) {
            val remaining = budget - (System.nanoTime() - started)
            if (remaining <= 0) break
            val completed = results.poll(remaining, TimeUnit.NANOSECONDS) ?: break
            completed.get()?.let {
                discovered.add(it)
                onFound(it.value)
            }
            if (addresses.hasNext()) probeNext() else inFlight--
        }
        found.addAll(discovered.sortedBy { it.index }.map { it.value })
        return found
    } finally {
        workers.shutdownNow()
    }
}
