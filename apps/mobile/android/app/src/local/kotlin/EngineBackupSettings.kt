package dev.wingmate.mobile

import android.app.AlertDialog
import android.content.ActivityNotFoundException
import android.net.Uri
import android.widget.ProgressBar
import android.widget.Toast
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.runtime.LocalRuntime
import dev.wingmate.mobile.runtime.RuntimeStatus
import java.time.LocalDate

internal class EngineBackupSettings(
    private val activity: AppCompatActivity,
    private val labels: ConnectionStrings,
    private val startEngine: () -> Unit,
) {
    private val exportPicker =
        activity.registerForActivityResult(
            ActivityResultContracts.CreateDocument("application/gzip"),
        ) {
            if (it != null) exportTo(it)
        }
    private val importPicker =
        activity.registerForActivityResult(ActivityResultContracts.OpenDocument()) {
            if (it != null) validate(it)
        }

    fun pickExport() = openPicker { exportPicker.launch("wingmate-dsh-${LocalDate.now()}.tar.gz") }

    fun pickImport() = openPicker { importPicker.launch(arrayOf("*/*")) }

    private fun openPicker(action: () -> Unit) {
        try {
            action()
        } catch (_: ActivityNotFoundException) {
            Toast.makeText(
                    activity,
                    labels["localAgent.backupPickerUnavailable"],
                    Toast.LENGTH_LONG,
                )
                .show()
        }
    }

    private val engineRunning: Boolean
        get() =
            when (LocalRuntime.status.value) {
                RuntimeStatus.Stopped,
                is RuntimeStatus.Failed -> false
                else -> true
            }

    private fun exportTo(uri: Uri) {
        val save = {
            val progress = progress("localAgent.backupExporting")
            LocalRuntime.exportBackup(activity, uri) { result ->
                if (result.isFailure) LocalRuntime.discardExport(activity, uri)
                if (!canShow()) return@exportBackup
                progress.dismiss()
                if (result.isSuccess) {
                    alert("localAgent.backupSaved", result.getOrNull())
                } else {
                    alert("localAgent.backupExportFailed")
                }
            }
        }
        if (!engineRunning) {
            save()
            return
        }
        var confirmed = false
        AlertDialog.Builder(activity)
            .setTitle(labels["localAgent.backupExport"])
            .setMessage(labels["localAgent.backupStopHint"])
            .setNegativeButton(labels["common.cancel"], null)
            .setPositiveButton(labels["localAgent.backupStopContinue"]) { _, _ ->
                confirmed = true
                save()
            }
            .setOnDismissListener {
                if (!confirmed) LocalRuntime.discardExport(activity, uri)
            }
            .show()
    }

    private fun validate(uri: Uri) {
        val progress = progress("localAgent.backupValidating")
        LocalRuntime.prepareBackup(activity, uri) { result ->
            if (!canShow()) {
                result.getOrNull()?.let(LocalRuntime::discardBackup)
                return@prepareBackup
            }
            progress.dismiss()
            result.fold(::confirmRestore) { alert("localAgent.backupInvalid") }
        }
    }

    private fun confirmRestore(backup: LocalRuntime.PreparedBackup) {
        var confirmed = false
        val installed = LocalRuntime.isInstalled(activity)
        val message =
            labels[
                if (installed) "localAgent.backupReplaceHint" else "localAgent.backupInstallHint"] +
                if (engineRunning) "\n\n" + labels["localAgent.backupStopHint"] else ""
        AlertDialog.Builder(activity)
            .setTitle(labels["localAgent.backupImport"])
            .setMessage(message)
            .setNegativeButton(labels["common.cancel"], null)
            .setPositiveButton(
                labels[if (installed) "localAgent.backupReplace" else "localAgent.backupRestore"],
            ) { _, _ ->
                confirmed = true
                val progress = progress("localAgent.backupRestoring")
                LocalRuntime.restoreBackup(activity, backup) { result ->
                    if (!canShow()) return@restoreBackup
                    progress.dismiss()
                    if (result.isSuccess) {
                        AlertDialog.Builder(activity)
                            .setTitle(labels["localAgent.backupRestored"])
                            .setNegativeButton(android.R.string.ok, null)
                            .setPositiveButton(labels["localAgent.start"]) { _, _ -> startEngine() }
                            .show()
                    } else {
                        alert("localAgent.backupRestoreFailed")
                    }
                }
            }
            .setOnDismissListener {
                if (!confirmed) LocalRuntime.discardBackup(backup)
            }
            .show()
    }

    private fun progress(title: String): AlertDialog =
        AlertDialog.Builder(activity)
            .setTitle(labels[title])
            .setView(ProgressBar(activity))
            .setCancelable(false)
            .show()

    private fun alert(title: String, message: String? = null) {
        AlertDialog.Builder(activity)
            .setTitle(labels[title])
            .setMessage(message)
            .setPositiveButton(android.R.string.ok, null)
            .show()
    }

    private fun canShow() = !activity.isFinishing && !activity.isDestroyed
}
