package dev.wingmate.mobile.runtime

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Environment
import java.io.File

object SharedStorage {
    const val guestWorkspace = "/root/workspace"
    const val guestDshHome = "/root/.dsh"

    @Suppress("DEPRECATION")
    private val directory: File
        get() =
            File(
                Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOCUMENTS),
                "Wingmate",
            )

    val workspace: File
        get() = directory.resolve("workspace")

    val dshHome: File
        get() = directory.resolve(".dsh")

    private val permissions =
        listOf(
            Manifest.permission.READ_EXTERNAL_STORAGE,
            Manifest.permission.WRITE_EXTERNAL_STORAGE,
        )

    fun missingPermissions(context: Context): List<String> = permissions.filter {
        context.checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED
    }

    fun hasPermission(context: Context): Boolean = missingPermissions(context).isEmpty()

    internal fun prepare(context: Context) {
        check(hasPermission(context)) { "Shared storage permission is required" }
        for (path in listOf(workspace, dshHome)) {
            path.mkdirs()
            check(path.isDirectory && path.canRead() && path.canWrite()) {
                "Shared storage is unavailable: ${path.absolutePath}"
            }
        }
    }
}
