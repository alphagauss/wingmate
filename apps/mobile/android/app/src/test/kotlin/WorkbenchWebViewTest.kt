package dev.wingmate.mobile.web

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WorkbenchWebViewTest {
    @Test
    fun requiresWebView130OrLater() {
        assertTrue(supportsWorkbenchWebView("130.0.6723.107"))
        assertTrue(supportsWorkbenchWebView("131.0.6778.81"))
        assertFalse(supportsWorkbenchWebView("129.0.6668.100"))
        assertFalse(supportsWorkbenchWebView("104.0.5112.97"))
        assertFalse(supportsWorkbenchWebView("unknown"))
        assertFalse(supportsWorkbenchWebView(null))
    }
}
