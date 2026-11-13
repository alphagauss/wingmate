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
    private var statusBeforeStart: RuntimeStatus? = null

    override val openWorkbench: (() -> Unit)?
        get() = if (status is RuntimeStatus.Ready) ::openCurrentWorkbench else null
    private val autoStartOnLaunch: Boolean
        get() = preferences.getBoolean("auto_start", false)
    override val shouldStartOnLaunch: Boolean
        get() = autoStartOnLaunch && installed && status != RuntimeStatus.Stopping

    override val launchPending: Boolean
        get() = status === statusBeforeStart || status is RuntimeStatus.Installing || status == RuntimeStatus.Starting

    override fun startOnLaunch() {
        val state = status
        if (state == RuntimeStatus.Stopped || state is RuntimeStatus.Failed) {
            statusBeforeStart = state
            LocalRuntime.start(activity)
        }
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
        LocalRuntime.status.observe(activity) {
            if (it !== statusBeforeStart) statusBeforeStart = null
            changed()
        }
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
            val controls = LinearLayout(activity).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(activity.dp(16), activity.dp(12), activity.dp(16), 0)
            }
            when (state) {
                RuntimeStatus.Stopped, is RuntimeStatus.Failed -> {
                    val action = when {
                        state is RuntimeStatus.Failed -> "localAgent.retry"
                        installed -> "localAgent.start"
                        else -> "localAgent.install"
                    }
                    controls.addView(actionButton(labels[action], R.drawable.ic_settings_start, primary = true) {
                        LocalRuntime.start(activity)
                    }, -1, -2)
                }
                is RuntimeStatus.Ready -> {
                    controls.addView(actionButton(labels["localAgent.open"], R.drawable.ic_settings_host,
                        primary = true, action = ::openCurrentWorkbench), -1, -2)
                    controls.addView(LinearLayout(activity).apply {
                        val restart = actionButton(labels["localAgent.restart"], R.drawable.ic_refresh) {
                            confirm("localAgent.restart") { LocalRuntime.restart(activity) }
                        }
                        addView(restart, LinearLayout.LayoutParams(0, -2, 1f).apply {
                            marginEnd = activity.dp(8)
                        })
                        addView(stopButton(ui), LinearLayout.LayoutParams(0, -2, 1f))
                    }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = activity.dp(8) })
                }
                RuntimeStatus.Starting -> controls.addView(stopButton(ui), -1, -2)
                is RuntimeStatus.Installing, RuntimeStatus.Stopping -> Unit
            }
            if (controls.childCount > 0) addView(controls, -1, -2)
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

    private fun stopButton(ui: SettingsUi) = ui.actionButton(labels["localAgent.stop"], R.drawable.ic_settings_stop) {
        confirm("localAgent.stop") { LocalRuntime.stop(activity) }
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
        val content = ui.text(LocalRuntime.logs().ifBlank { labels["localAgent.noLogs"] }, 13f).apply {
            setPadding(activity.dp(20), activity.dp(10), activity.dp(20), activity.dp(10))
            setTextIsSelectable(true)
        }
        AlertDialog.Builder(activity).setTitle(labels["localAgent.logs"])
            .setView(ScrollView(activity).apply { addView(content) })
            .setPositiveButton(android.R.string.ok, null).show()
    }
}
