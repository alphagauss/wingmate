package dev.wingmate.mobile

import android.app.AlertDialog
import android.content.Intent
import android.net.Uri
import android.os.PowerManager
import android.provider.Settings
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import androidx.appcompat.app.AppCompatActivity
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.runtime.LocalRuntime
import dev.wingmate.mobile.runtime.RuntimeStatus
import dev.wingmate.mobile.settings.LocalEngineSettings
import dev.wingmate.mobile.settings.SettingsUi
import dev.wingmate.mobile.theme.dp

internal fun localEngineSettings(activity: AppCompatActivity, open: (Uri) -> Unit): LocalEngineSettings? =
    EngineSettings(activity, open)

private class EngineSettings(
    private val activity: AppCompatActivity,
    private val open: (Uri) -> Unit
) : LocalEngineSettings {
    private val labels = ConnectionStrings(activity)
    private val preferences = activity.getSharedPreferences("local_engine", AppCompatActivity.MODE_PRIVATE)
    private val status get() = requireNotNull(LocalRuntime.status.value)
    private val installed get() = LocalRuntime.isInstalled(activity)

    override val openWorkbench: (() -> Unit)?
        get() = if (status is RuntimeStatus.Ready) ::openCurrentWorkbench else null
    private val autoStartOnLaunch: Boolean
        get() = preferences.getBoolean("auto_start", false)

    override fun startOnLaunch(): Boolean {
        val state = status
        if (!autoStartOnLaunch || !installed || state == RuntimeStatus.Stopping) return false
        if (state == RuntimeStatus.Stopped || state is RuntimeStatus.Failed) {
            LocalRuntime.start(activity)
        }
        return true
    }

    private fun openCurrentWorkbench() {
        val ready = status as? RuntimeStatus.Ready ?: return
        open(Uri.parse(ready.address).buildUpon().path("/").appendQueryParameter("token", ready.token).build())
    }

    override val summary: String
        get() = labels[when (status) {
            RuntimeStatus.Stopped -> if (installed) "localAgent.stopped" else "localAgent.notInstalled"
            is RuntimeStatus.Installing -> "localAgent.installing"
            RuntimeStatus.Starting -> "localAgent.starting"
            is RuntimeStatus.Ready -> "localAgent.running"
            RuntimeStatus.Stopping -> "localAgent.stopping"
            is RuntimeStatus.Failed -> "localAgent.failed"
        }]

    override fun observe(changed: () -> Unit) {
        LocalRuntime.status.observe(activity) { changed() }
    }

    override fun render(ui: SettingsUi, body: LinearLayout) = with(ui) {
        body.section {
            val state = status
            addView(row(summary, subtitle = (state as? RuntimeStatus.Failed)?.reason,
                icon = R.drawable.ic_settings_engine, maxSubtitleLines = Int.MAX_VALUE))
            if (state is RuntimeStatus.Installing) {
                addView(ProgressBar(activity, null, android.R.attr.progressBarStyleHorizontal).apply {
                    isIndeterminate = state.progress == 0
                    progress = state.progress
                }, LinearLayout.LayoutParams(-1, activity.dp(10)).apply {
                    setMargins(activity.dp(16), 0, activity.dp(16), activity.dp(12))
                })
            }
            when (state) {
                RuntimeStatus.Stopped, is RuntimeStatus.Failed -> {
                    val action = when {
                        state is RuntimeStatus.Failed -> "localAgent.retry"
                        installed -> "localAgent.start"
                        else -> "localAgent.install"
                    }
                    addView(row(labels[action], action = { LocalRuntime.start(activity) }))
                }
                is RuntimeStatus.Ready -> {
                    addView(row(labels["localAgent.open"], action = openWorkbench))
                    addView(row(labels["localAgent.restart"], action = {
                        confirm("localAgent.restart") { LocalRuntime.restart(activity) }
                    }))
                    addView(row(labels["localAgent.stop"], action = {
                        confirm("localAgent.stop") { LocalRuntime.stop(activity) }
                    }))
                }
                RuntimeStatus.Starting -> addView(row(labels["localAgent.stop"], action = {
                    confirm("localAgent.stop") { LocalRuntime.stop(activity) }
                }))
                is RuntimeStatus.Installing, RuntimeStatus.Stopping -> Unit
            }
        }
        body.section {
            addView(toggle(labels["localAgent.autoStart"], autoStartOnLaunch) {
                preferences.edit().putBoolean("auto_start", it).apply()
            })
            val exempt = activity.getSystemService(PowerManager::class.java)
                .isIgnoringBatteryOptimizations(activity.packageName)
            addView(row(labels["localAgent.background"], value = labels[
                if (exempt) "settings.on" else "localAgent.needsSetup"
            ], action = ::background))
            addView(row(labels["localAgent.logs"], action = { logs(ui) }))
        }
    }

    private fun confirm(key: String, action: () -> Unit) {
        AlertDialog.Builder(activity).setTitle(labels[key])
            .setMessage(labels["localAgent.stopConfirm"])
            .setNegativeButton(labels["common.cancel"], null)
            .setPositiveButton(labels[key]) { _, _ -> action() }.show()
    }

    private fun background() {
        AlertDialog.Builder(activity).setTitle(labels["localAgent.background"])
            .setMessage(labels["localAgent.backgroundHint"])
            .setNegativeButton(labels["common.cancel"], null)
            .setPositiveButton(labels["scan.openSettings"]) { _, _ ->
                activity.startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))
            }.show()
    }

    private fun logs(ui: SettingsUi) {
        val content = ui.colors.text(LocalRuntime.logs().ifBlank { labels["localAgent.noLogs"] }, 13f).apply {
            setPadding(activity.dp(20), activity.dp(10), activity.dp(20), activity.dp(10))
            setTextIsSelectable(true)
        }
        AlertDialog.Builder(activity).setTitle(labels["localAgent.logs"])
            .setView(ScrollView(activity).apply { addView(content) })
            .setPositiveButton(android.R.string.ok, null).show()
    }
}
