package dev.wingmate.mobile.runtime.phone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PhoneToolsTest {
    @Test
    fun backgroundRequiresAndroid14() {
        assertFalse(BackgroundPolicy.supported(33))
        assertTrue(BackgroundPolicy.supported(34))
    }

    @Test
    fun openAndScreenDefaultToMainAndAcceptAnExplicitBackgroundFlag() {
        val requests =
            mapOf("open" to mapOf("app" to "dev.example.app"), "screen" to mapOf("image" to true))
        for ((tool, args) in requests) {
            assertTrue(McpServer.accepts(tool, args))
            assertFalse(McpServer.Call(1, tool, args).background)
            val main = args + ("background" to false)
            assertTrue(McpServer.accepts(tool, main))
            assertFalse(McpServer.Call(1, tool, main).background)
            val background = args + ("background" to true)
            assertTrue(McpServer.accepts(tool, background))
            assertTrue(McpServer.Call(1, tool, background).background)
            assertFalse(McpServer.accepts(tool, args + ("background" to "true")))
            assertFalse(McpServer.accepts(tool, background + ("displayId" to 0L)))
        }
        assertFalse(McpServer.accepts("background", mapOf("action" to "close")))
    }

    @Test
    fun actionsUseTheDisplayOfTheirObservation() {
        val requests =
            mapOf(
                "tap" to mapOf("id" to 1L),
                "tap_xy" to mapOf("x" to 0L, "y" to 0L),
                "scroll" to mapOf("direction" to "down"),
                "type" to mapOf("id" to 1L, "text" to "draft"),
            )
        for ((tool, args) in requests) {
            val screen = if (tool == "tap_xy") "observation" else "screen"
            val main = args + (screen to "s1")
            val background = args + (screen to "b1")
            assertTrue(McpServer.accepts(tool, main))
            assertTrue(McpServer.accepts(tool, background))
            assertFalse(McpServer.Call(1, tool, main).background)
            assertTrue(McpServer.Call(1, tool, background).background)
            assertFalse(McpServer.accepts(tool, args + ("background" to true)))
        }
        assertFalse(
            McpServer.Call(1, "key", mapOf("key" to "back", "screen" to "b1")).background,
        )
    }

    @Test
    fun listsPhoneToolsOnlyWhenEnabled() {
        fun names(enabled: Boolean): List<Any?> {
            val reply = Json.parse(McpServer.list(1, enabled)) as Map<*, *>
            val result = reply["result"] as Map<*, *>
            return (result["tools"] as List<*>).map { (it as Map<*, *>)["name"] }
        }
        assertEquals(emptyList<String>(), names(false))
        assertEquals(
            listOf(
                "apps",
                "open",
                "screen",
                "tap",
                "tap_xy",
                "scroll",
                "type",
                "key",
            ),
            names(true),
        )
    }

    @Test
    fun acceptsOnlyDeclaredActionsAndArguments() {
        assertTrue(McpServer.accepts("tap", mapOf("screen" to "s1", "id" to 4L)))
        assertFalse(McpServer.accepts("tap", mapOf("screen" to "s1", "id" to -1L)))
        assertFalse(
            McpServer.accepts(
                "type",
                mapOf("screen" to "s1", "id" to 4L, "text" to "hi", "submit" to true),
            ),
        )
        assertTrue(McpServer.accepts("screen", mapOf("image" to true)))
        assertTrue(McpServer.accepts("tap_xy", mapOf("observation" to "s1", "x" to 0L, "y" to 1L)))
        assertFalse(
            McpServer.accepts("tap_xy", mapOf("observation" to "s1", "x" to -1L, "y" to 1L)),
        )
        assertFalse(McpServer.accepts("key", mapOf("key" to "notifications")))
    }

    @Test
    fun returnsScreenshotAsMcpImageContent() {
        val reply =
            Json.parse(McpServer.output(1, McpServer.Output("screen", jpeg = "AA=="))) as Map<*, *>
        val content = (reply["result"] as Map<*, *>)["content"] as List<*>
        assertEquals(mapOf("type" to "text", "text" to "screen"), content[0])
        assertEquals(
            mapOf("type" to "image", "mimeType" to "image/jpeg", "data" to "AA=="),
            content[1],
        )
    }

    @Test
    fun recognizesProtectedFieldsAndPackages() {
        assertTrue(PhoneSafety.blocked("dev.wingmate.mobile.local"))
        assertTrue(PhoneSafety.blocked("com.android.settings"))
        assertTrue(PhoneSafety.blocked("com.eg.android.AlipayGphone"))
        assertFalse(PhoneSafety.blocked("dev.example.notes"))
        assertTrue(PhoneSafety.secretHit(null, null, "dev.example.notes:id/smsCode") != null)
        assertFalse(ScreenMask.mask("card 4111 1111 1111 1111").contains("4111 1111 1111 1111"))
    }
}
