package dev.wingmate.mobile.i18n

import android.content.Context
import dev.wingmate.mobile.settings.AppPreferences
import java.util.concurrent.ConcurrentHashMap
import org.json.JSONObject

internal class ConnectionStrings(private val context: Context) {
    private val preferences = AppPreferences(context)
    val language: String
        get() =
            preferences.language.ifEmpty {
                when (context.resources.configuration.locales[0]?.language) {
                    "zh" -> "zh-CN"
                    "ko" -> "ko-KR"
                    "ru" -> "ru-RU"
                    else -> "en-US"
                }
            }

    val messages: JSONObject
        get() {
            val code = language
            return translations.getOrPut(code) {
                context.assets.open("locales/$code.json").bufferedReader().use {
                    JSONObject(it.readText())
                }
            }
        }

    operator fun get(key: String): String = messages.getString(key)

    companion object {
        private val translations = ConcurrentHashMap<String, JSONObject>()
    }
}
