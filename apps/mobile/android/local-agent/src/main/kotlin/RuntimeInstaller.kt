package dev.wingmate.mobile.runtime

import android.content.Context
import java.io.File
import java.io.FileOutputStream
import java.nio.channels.FileChannel
import java.nio.file.Files
import java.nio.file.LinkOption.NOFOLLOW_LINKS
import java.nio.file.Path
import java.nio.file.SimpleFileVisitor
import java.nio.file.FileVisitResult
import java.nio.file.StandardOpenOption
import java.nio.file.attribute.BasicFileAttributes
import java.io.IOException
import java.security.DigestInputStream
import java.security.MessageDigest
import org.json.JSONObject

/** A complete staged first install, followed by the upstream engine-only upgrade. */
internal class RuntimeInstaller(
    private val context: Context,
    private val manifest: RuntimeManifest,
) {
    private val base = File(context.filesDir, "runtime")
    val rootfs = File(base, "rootfs")
    private val staging = File(base, "staging")
    private val version = File(base, "rootfs.version")
    private val upgrade = EngineUpgrade(base, ::syncFilesystems)

    fun prepare(onProgress: (Int) -> Unit) {
        base.mkdirs()
        upgrade.recover()
        check(!Files.isSymbolicLink(rootfs.toPath()))
        if (rootfs.isDirectory && version.isFile && version.readText() == manifest.runtimeVersion) {
            clearStaging()
            return
        }
        if (rootfs.exists() && installedInfo(rootfs).let(::engineUpgradeSpec).baseId != manifest.upgrade.baseId) {
            throw RuntimeUpgradeBlocked("base")
        }

        clearStaging()
        val required = manifest.uncompressedBytes * 115 / 100
        check(base.usableSpace >= required) { "Insufficient space to unpack the runtime (${required shr 20} MiB required)" }
        val digest = MessageDigest.getInstance("SHA-256")
        val size = context.assets.openFd(manifest.archive).use { it.length }
        DigestInputStream(context.assets.open(manifest.archive), digest).use { stream ->
            val buffer = ByteArray(64 * 1024)
            while (stream.read(buffer) >= 0) Unit
        }
        check(digest.digest().joinToString("") { "%02x".format(it) } == manifest.sha256) {
            "Runtime archive checksum failed"
        }

        check(staging.mkdir())
        try {
            val decoded = ParallelXZInputStream.open(
                { AssetSeekableInputStream(context.assets.openFd(manifest.archive)) },
                { onProgress((it * 100 / size).coerceAtMost(100).toInt()) },
            )
            TarExtractor(staging).extractDecoded(decoded)
            val shipped = installedInfo(staging)
            check(shipped.getString("runtimeVersion") == manifest.runtimeVersion &&
                engineUpgradeSpec(shipped) == manifest.upgrade)
            if (rootfs.isDirectory) {
                upgrade.activate(manifest.runtimeVersion, manifest.upgrade)
            } else {
                appendAndroidGroupNames(staging, File("/proc/self/status").readText())
                FileOutputStream(version).use {
                    it.write(manifest.runtimeVersion.toByteArray())
                    it.fd.sync()
                }
                // Publish the new tree only after both its data and the version marker are durable.
                syncFilesystems()
                check(staging.renameTo(rootfs)) { "Could not activate the runtime" }
                FileChannel.open(base.toPath(), StandardOpenOption.READ).use { it.force(true) }
            }
        } finally {
            clearStaging()
        }
    }

    private fun clearStaging() {
        if (!Files.exists(staging.toPath(), NOFOLLOW_LINKS)) return
        // Files.walkFileTree leaves archive symlinks as leaf entries.
        Files.walkFileTree(staging.toPath(), object : SimpleFileVisitor<Path>() {
            override fun visitFile(file: Path, attrs: BasicFileAttributes): FileVisitResult {
                Files.delete(file)
                return FileVisitResult.CONTINUE
            }

            override fun postVisitDirectory(dir: Path, error: IOException?): FileVisitResult {
                if (error != null) throw error
                Files.delete(dir)
                return FileVisitResult.CONTINUE
            }
        })
    }
}

private fun installedInfo(tree: File): JSONObject =
    JSONObject(File(tree, "etc/dsh-app/runtime.json").readText())

internal fun engineUpgradeSpec(json: JSONObject): EngineUpgradeSpec {
    val upgrade = json.getJSONObject("upgrade")
    check(upgrade.getInt("schema") == 1)
    val paths = upgrade.getJSONArray("paths")
    return EngineUpgradeSpec(
        upgrade.getString("baseId"), upgrade.getString("ubuntuVersion"),
        List(paths.length()) { paths.getString(it) },
    ).also { it.validate() }
}
