package dev.wingmate.mobile.runtime

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Environment
import java.io.File

object SharedWorkspace {
    const val guestPath = "/Workspace"

    @Suppress("DEPRECATION")
    val directory: File
        get() =
            File(
                Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOCUMENTS),
                "Wingmate/Workspaces",
            )

    private val permissions =
        listOf(
            Manifest.permission.READ_EXTERNAL_STORAGE,
            Manifest.permission.WRITE_EXTERNAL_STORAGE,
        )

    fun missingPermissions(context: Context): List<String> = permissions.filter {
        context.checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED
    }

    fun hasPermission(context: Context): Boolean = missingPermissions(context).isEmpty()

    internal fun prepare(context: Context): File {
        check(hasPermission(context)) { "Shared workspace storage permission is required" }
        val path = directory
        check((path.isDirectory || path.mkdirs()) && path.canRead() && path.canWrite()) {
            "Shared workspace is unavailable: ${path.absolutePath}"
        }
        return path
    }
}
