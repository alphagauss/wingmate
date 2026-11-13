package dev.wingmate.mobile.settings

import android.widget.LinearLayout

internal interface LocalEngineSettings {
    val summary: String
    val openWorkbench: (() -> Unit)?
    fun observe(changed: () -> Unit)
    fun render(ui: SettingsUi, body: LinearLayout)
}
