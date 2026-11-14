package dev.wingmate.mobile.web

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WorkbenchWebViewTest {
    @Test
    fun requiresWebView104OrLater() {
        assertTrue(supportsWorkbenchWebView("104.0.5112.97"))
        assertTrue(supportsWorkbenchWebView("105.0.5195.68"))
        assertFalse(supportsWorkbenchWebView("103.0.5060.129"))
        assertFalse(supportsWorkbenchWebView("96.0.4664.104"))
        assertFalse(supportsWorkbenchWebView("unknown"))
        assertFalse(supportsWorkbenchWebView(null))
    }
}
