package dev.wingmate.mobile.runtime

import org.junit.Assert.assertEquals
import org.junit.Test

class LogRedactorTest {
    @Test fun stripsWorkbenchLoginTokenFromTheStartupLog() {
        assertEquals(
            "dsh web: http://127.0.0.1:3080/?token=•••",
            LogRedactor.redact("dsh web: http://127.0.0.1:3080/?token=secret-token"),
        )
    }
}
