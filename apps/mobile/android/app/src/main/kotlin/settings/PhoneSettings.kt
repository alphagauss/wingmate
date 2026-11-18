package dev.wingmate.mobile.settings

import android.widget.LinearLayout

internal interface PhoneSettings {
    val summary: String

    fun observe(changed: () -> Unit)

    fun render(ui: SettingsUi, body: LinearLayout, changed: () -> Unit)
}
