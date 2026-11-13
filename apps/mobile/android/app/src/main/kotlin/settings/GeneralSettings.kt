package dev.wingmate.mobile.settings

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.core.app.NotificationManagerCompat
import androidx.webkit.WebViewCompat
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.theme.dp

internal class GeneralSettings(
    private val activity: AppCompatActivity,
    private val languageChanged: () -> Unit
) {
    private val labels = ConnectionStrings(activity)
    private val preferences = AppPreferences(activity)
    private val packageInfo by lazy { activity.packageManager.getPackageInfo(activity.packageName, 0) }
    @Suppress("DEPRECATION")
    val version: String get() = "${packageInfo.versionName} (${packageInfo.versionCode})"
    val languageName: String get() = if (preferences.language.isEmpty()) labels["settings.system"]
        else languages.getValue(preferences.language)
    val notificationsEnabled: Boolean
        get() = preferences.taskNotifications && NotificationManagerCompat.from(activity).areNotificationsEnabled()

    fun language() {
        val choices = listOf(labels["settings.system"]) + languages.values
        val codes = listOf("") + languages.keys
        AlertDialog.Builder(activity)
            .setTitle(labels["settings.language"])
            .setSingleChoiceItems(choices.toTypedArray(), codes.indexOf(preferences.language)) { dialog, index ->
                preferences.language = codes[index]
                dialog.dismiss()
                languageChanged()
            }
            .setNegativeButton(labels["common.cancel"], null)
            .show()
    }

    fun notifications(ui: SettingsUi, body: LinearLayout) = with(ui) {
        body.section {
            addView(toggle(labels["settings.taskNotifications"], preferences.taskNotifications) {
                preferences.taskNotifications = it
            })
            addView(row(labels["settings.systemNotifications"], value = state(
                NotificationManagerCompat.from(activity).areNotificationsEnabled()
            ), action = ::openNotificationSettings))
        }
    }

    fun permissions(ui: SettingsUi, body: LinearLayout) = with(ui) {
        body.section {
            addView(row(labels["settings.camera"], value = state(
                activity.checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED
            ), action = ::openAppPermissions))
            addView(row(labels["settings.systemNotifications"], value = state(
                NotificationManagerCompat.from(activity).areNotificationsEnabled()
            ), action = ::openNotificationSettings))
        }
    }

    fun diagnostics(ui: SettingsUi, body: LinearLayout, current: String?) = with(ui) {
        body.section {
            addView(row(labels["settings.version"], subtitle = version))
            addView(row("Android", subtitle = Build.VERSION.RELEASE))
            addView(row("WebView", subtitle = WebViewCompat.getCurrentWebViewPackage(activity)?.versionName))
            addView(row(labels["connection.currentConnection"], subtitle = current ?: labels["connection.noConnection"]))
        }
    }

    fun about(ui: SettingsUi, body: LinearLayout) = with(ui) {
        body.section {
            addView(row(labels["settings.version"], value = version))
            addView(row(labels["settings.licenses"], action = ::showLicenses))
        }
    }

    fun state(enabled: Boolean) = labels[if (enabled) "settings.on" else "settings.off"]

    private fun openAppPermissions() {
        activity.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${activity.packageName}")))
    }

    private fun openNotificationSettings() {
        if (Build.VERSION.SDK_INT < 26) return openAppPermissions()
        activity.startActivity(Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
            .putExtra(Settings.EXTRA_APP_PACKAGE, activity.packageName))
    }

    private fun showLicenses() {
        val paths = listOf("licenses/LICENSE.wingmate") +
            activity.assets.list("runtime/licenses").orEmpty().sorted().map { "runtime/licenses/$it" }
        val text = paths.joinToString("\n\n") { path ->
            activity.assets.open(path).bufferedReader().use { it.readText() }
        }
        val content = TextView(activity).apply {
            this.text = text
            setTextIsSelectable(true)
            setPadding(activity.dp(20), activity.dp(8), activity.dp(20), activity.dp(8))
        }
        AlertDialog.Builder(activity).setTitle(labels["settings.licenses"])
            .setView(ScrollView(activity).apply { addView(content) })
            .setPositiveButton(android.R.string.ok, null).show()
    }

    private companion object {
        val languages = linkedMapOf(
            "zh-CN" to "简体中文", "en-US" to "English", "ko-KR" to "한국어", "ru-RU" to "Русский"
        )
    }
}
