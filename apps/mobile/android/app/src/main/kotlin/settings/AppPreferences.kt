package dev.wingmate.mobile.settings

import android.content.Context

internal class AppPreferences(context: Context) {
    private val values = context.getSharedPreferences("connection", Context.MODE_PRIVATE)

    var language: String
        get() = values.getString("app_language", "").orEmpty()
        set(value) { values.edit().putString("app_language", value).apply() }

    var taskNotifications: Boolean
        get() = values.getBoolean("task_notifications", true)
        set(value) { values.edit().putBoolean("task_notifications", value).apply() }

    fun autoConnect(default: Boolean) = values.getBoolean("auto_connect", default)

    fun setAutoConnect(value: Boolean) {
        values.edit().putBoolean("auto_connect", value).apply()
    }
}
