package dev.wingmate.mobile.i18n

import android.content.Context
import org.json.JSONObject
import java.util.concurrent.ConcurrentHashMap

internal class ConnectionStrings(context: Context) {
    val language = when (context.resources.configuration.locales[0]?.language) {
        "zh" -> "zh-CN"
        "ko" -> "ko-KR"
        "ru" -> "ru-RU"
        else -> "en-US"
    }
    val messages = translations.getOrPut(language) {
        context.assets.open("locales/$language.json").bufferedReader().use { JSONObject(it.readText()) }
    }

    operator fun get(key: String): String = messages.getString(key)

    companion object {
        private val translations = ConcurrentHashMap<String, JSONObject>()
    }
}
