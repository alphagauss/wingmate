package dev.wingmate.mobile.remote

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ConnectionTargetTest {
    @Test
    fun scansThePrimaryAndBackupPortsForEachHost() {
        val release = lanCandidates("192.168.1.8", debug = false).toList()
        assertEquals(
            listOf(
                "http://192.168.1.1:8746",
                "http://192.168.1.1:8747",
                "http://192.168.1.1:8748",
                "http://192.168.1.2:8746",
            ),
            release.take(4),
        )
        assertEquals(3 * 253, release.size)
        assertEquals(false, release.any { it.startsWith("http://192.168.1.8:") })
        assertEquals("http://192.168.1.254:8749", lanCandidates("192.168.1.8", debug = true).last())
    }

    @Test
    fun identifiesGatewayCookiesByTransportAndPort() {
        assertEquals("wingmate_share_8747", sessionCookieName("http://192.168.1.8:8747"))
        assertEquals("wingmate_share_80", sessionCookieName("http://192.168.1.8"))
        assertEquals("wingmate_public", sessionCookieName("https://example.org"))
    }

    @Test
    fun acceptsGatewayLinksAndManualAddresses() {
        val lan = "http://192.168.1.8:3210"
        val tunnel = "https://wingmate-test.trycloudflare.com"
        val link = "$lan/?auth=keep%2Bthis"
        assertEquals(ConnectionTarget(lan, link), ConnectionTarget.parse("""{"url":"$link"}"""))
        assertEquals(link, ConnectionTarget.parse("192.168.1.8:3210/?auth=keep%2Bthis")?.link)
        assertEquals(
            "$tunnel/?auth=code",
            ConnectionTarget.parse(" ${tunnel.uppercase()}:443/?auth=code ")?.link,
        )
        assertEquals(
            "$tunnel/",
            ConnectionTarget.parse(tunnel.removePrefix("https://"), manual = true)?.link,
        )
    }

    @Test
    fun rejectsLinksOutsideTheGatewayProtocol() {
        for (link in
            listOf(
                "javascript:alert(1)",
                "file:///sdcard/a.html",
                "http://example.com",
                "https://192.168.1.8/",
                "http://[::1]/",
                "http://192.168.1.8:65536/",
                "http://192.168.1.8/elsewhere?auth=code",
                "https://example.com//",
                "https://example.com/?token=code",
                "https://example.com/?auth=",
                "https://example.com/?auth=abc&auth=xyz",
                "https://example.com/?auth=abc#other",
                "https://user:pass@example.com/",
                """{"url":null}""",
            )) assertNull(link, ConnectionTarget.parse(link, manual = true))
    }
}
