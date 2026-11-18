package dev.wingmate.mobile

import android.accessibilityservice.AccessibilityServiceInfo
import android.content.Intent
import android.provider.Settings
import android.view.accessibility.AccessibilityManager
import android.widget.LinearLayout
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.runtime.phone.BackgroundPhoneStatus
import dev.wingmate.mobile.runtime.phone.PhoneAccessibility
import dev.wingmate.mobile.runtime.phone.PhoneControl
import dev.wingmate.mobile.settings.PhoneSettings
import dev.wingmate.mobile.settings.SettingsUi

internal fun phoneSettings(activity: AppCompatActivity): PhoneSettings? =
    LocalPhoneSettings(activity)

private class LocalPhoneSettings(private val activity: AppCompatActivity) : PhoneSettings {
    private val labels = ConnectionStrings(activity)
    private val control = PhoneControl.get(activity)

    override val summary: String
        get() =
            labels[
                when {
                    !control.enabled -> "settings.off"
                    control.connected.value == true -> "settings.on"
                    else -> "phone.setupNeeded"
                }]

    override fun observe(changed: () -> Unit) {
        control.connected.observe(activity) { changed() }
        control.backgroundChanged.observe(activity) { changed() }
    }

    override fun render(
        ui: SettingsUi,
        body: LinearLayout,
        changed: () -> Unit,
    ) =
        with(ui) {
            body.section {
                addView(
                    toggle(labels["phone.enable"], control.enabled) { enabled ->
                        if (!enabled) {
                            control.setEnabled(false)
                            changed()
                        } else {
                            changed()
                            AlertDialog.Builder(activity)
                                .setTitle(labels["phone.enable"])
                                .setMessage(labels["phone.disclosure"])
                                .setNegativeButton(labels["common.cancel"], null)
                                .setPositiveButton(labels["phone.enable"]) { _, _ ->
                                    control.setEnabled(true)
                                    changed()
                                }
                                .show()
                        }
                    },
                )
            }
            body.section(labels["phone.setup"]) {
                val status =
                    when {
                        control.connected.value == true -> "phone.connected"
                        accessibilityEnabled() -> "phone.waiting"
                        else -> "settings.off"
                    }
                addView(
                    row(
                        labels["phone.accessibility"],
                        subtitle = labels["phone.accessibilityHint"],
                        value = labels[status],
                        action = {
                            activity.startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))
                        },
                    ),
                )
                addView(
                    row(
                        labels["phone.testScreen"],
                        subtitle = labels["phone.testScreenHint"],
                        action = ::preview,
                    ),
                )
            }
            body.section {
                val status = control.backgroundStatus
                val title = labels["phone.background"]
                val entry =
                    when (status) {
                        BackgroundPhoneStatus.UNSUPPORTED ->
                            row(
                                title,
                                labels["phone.backgroundRequiresVersion"],
                                labels["settings.off"],
                            )
                        BackgroundPhoneStatus.SHIZUKU_UNAVAILABLE ->
                            row(
                                title,
                                labels["phone.backgroundRequiresShizuku"],
                                labels["settings.off"],
                            )
                        BackgroundPhoneStatus.PERMISSION_REQUIRED ->
                            row(
                                title,
                                labels["phone.backgroundPermission"],
                                labels["settings.off"],
                                action = control::requestBackgroundPermission,
                            )
                        BackgroundPhoneStatus.READY ->
                            row(title, labels["phone.backgroundReady"], labels["settings.off"])
                        BackgroundPhoneStatus.OPENING,
                        BackgroundPhoneStatus.OPEN ->
                            row(
                                title,
                                labels["phone.backgroundStop"],
                                labels[
                                    if (status == BackgroundPhoneStatus.OPEN) "settings.on"
                                    else "phone.backgroundOpening"],
                                action = control::stopBackground,
                            )
                        BackgroundPhoneStatus.CLOSING ->
                            row(title, value = labels["phone.backgroundClosing"])
                    }
                addView(entry)
            }
        }

    private fun accessibilityEnabled(): Boolean =
        activity
            .getSystemService(AccessibilityManager::class.java)
            .getEnabledAccessibilityServiceList(AccessibilityServiceInfo.FEEDBACK_ALL_MASK)
            .any { service ->
                val info = service.resolveInfo.serviceInfo
                info.packageName == activity.packageName &&
                    info.name == PhoneAccessibility::class.java.name
            }

    private fun preview() {
        val report = control.preview().getOrNull() ?: labels["phone.testUnavailable"]
        AlertDialog.Builder(activity)
            .setTitle(labels["phone.testScreen"])
            .setMessage(report)
            .setPositiveButton(android.R.string.ok, null)
            .show()
    }
}
