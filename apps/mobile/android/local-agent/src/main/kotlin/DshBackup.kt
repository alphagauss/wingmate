package dev.wingmate.mobile.runtime

import java.io.File
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.nio.file.Files
import java.nio.file.LinkOption.NOFOLLOW_LINKS
import java.nio.file.Path
import java.nio.file.StandardCopyOption.ATOMIC_MOVE
import java.nio.file.StandardOpenOption.CREATE_NEW
import java.nio.file.attribute.PosixFileAttributeView
import java.nio.file.attribute.PosixFilePermission
import java.util.zip.GZIPInputStream
import java.util.zip.GZIPOutputStream
import org.apache.commons.compress.archivers.tar.TarArchiveEntry
import org.apache.commons.compress.archivers.tar.TarArchiveInputStream
import org.apache.commons.compress.archivers.tar.TarArchiveOutputStream
import org.apache.commons.compress.archivers.tar.TarConstants

/** Scoped to the DSH home and its previous revision on the same filesystem. */
internal class DshBackup(private val guestHome: File) {
    private val home = guestHome.resolve(".dsh")
    private val previous = guestHome.resolve(".dsh-before-restore")

    fun recover() {
        if (
            !Files.exists(home.toPath(), NOFOLLOW_LINKS) &&
                Files.isDirectory(previous.toPath(), NOFOLLOW_LINKS)
        ) {
            Files.move(previous.toPath(), home.toPath(), ATOMIC_MOVE)
        }
    }

    fun export(output: OutputStream) {
        recover()
        check(Files.isDirectory(home.toPath(), NOFOLLOW_LINKS)) { "DSH home is missing" }
        TarArchiveOutputStream(GZIPOutputStream(output.buffered()), Charsets.UTF_8.name()).use { tar
            ->
            tar.setLongFileMode(TarArchiveOutputStream.LONGFILE_POSIX)
            tar.setBigNumberMode(TarArchiveOutputStream.BIGNUMBER_POSIX)
            tar.setAddPaxHeadersForNonAsciiNames(true)
            write(tar, home, home.name)
        }
    }

    private fun write(tar: TarArchiveOutputStream, file: File, name: String) {
        val path = file.toPath()
        val directory = Files.isDirectory(path, NOFOLLOW_LINKS)
        val regular = Files.isRegularFile(path, NOFOLLOW_LINKS)
        val entry =
            when {
                Files.isSymbolicLink(path) -> {
                    val link = Files.readSymbolicLink(path).toString()
                    checkLink(home.toPath(), path, link)
                    TarArchiveEntry(name, TarConstants.LF_SYMLINK).apply { linkName = link }
                }
                directory -> TarArchiveEntry("$name/")
                regular -> TarArchiveEntry(name).apply { size = Files.size(path) }
                else -> error("Unsupported DSH backup entry")
            }
        val permissions = posix(path)?.readAttributes()?.permissions().orEmpty()
        entry.mode =
            if (permissions.isEmpty()) {
                if (directory) 448 else 384 // 0700 and 0600
            } else {
                PosixFilePermission.values().foldIndexed(0) { index, mode, permission ->
                    if (permission in permissions) mode or (256 shr index) else mode
                }
            }
        tar.putArchiveEntry(entry)
        if (regular) file.inputStream().use { it.copyTo(tar) }
        tar.closeArchiveEntry()
        if (directory) {
            for (child in checkNotNull(file.listFiles())) write(tar, child, "$name/${child.name}")
        }
    }

    fun restore(staged: File, sync: () -> Unit) {
        check(Files.isDirectory(staged.toPath(), NOFOLLOW_LINKS)) { "Staged DSH backup is missing" }
        recover()
        deleteRuntimeTree(previous)
        sync()
        if (Files.exists(home.toPath(), NOFOLLOW_LINKS)) {
            Files.move(home.toPath(), previous.toPath(), ATOMIC_MOVE)
        }
        try {
            Files.move(staged.toPath(), home.toPath(), ATOMIC_MOVE)
        } catch (error: Exception) {
            recover()
            throw error
        }
        sync()
    }

    companion object {
        /** Expands the selected archive into an empty private directory before confirmation. */
        fun unpack(input: InputStream, staging: File): File {
            val root = staging.resolve(".dsh").toPath()
            val directories = linkedMapOf<Path, Int>()
            GZIPInputStream(input.buffered()).use { gzip ->
                DshTarInputStream(gzip).use { tar ->
                    while (true) {
                        val entry = tar.nextEntry ?: break
                        val parts = entry.name.trimEnd('/').split('/')
                        require(
                            entry.isCheckSumOK &&
                                parts.first() == ".dsh" &&
                                parts.none {
                                    it.isEmpty() || it == "." || it == ".." || '\\' in it
                                },
                        ) {
                            "Invalid DSH backup path"
                        }
                        val target = parts.fold(staging.toPath(), Path::resolve)
                        require(
                            target.startsWith(root) &&
                                (if (target == root) entry.linkFlag == TarConstants.LF_DIR
                                else target.parent in directories),
                        ) {
                            "Invalid DSH backup directory"
                        }
                        when (entry.linkFlag) {
                            TarConstants.LF_DIR -> {
                                Files.createDirectory(target)
                                directories[target] = if (target == root) 448 else entry.mode
                            }
                            TarConstants.LF_SYMLINK -> {
                                val link = checkLink(root, target, entry.linkName)
                                Files.createSymbolicLink(target, link)
                            }
                            TarConstants.LF_NORMAL,
                            TarConstants.LF_OLDNORM -> {
                                require(!entry.isSparse) { "Unsupported sparse DSH backup entry" }
                                Files.newOutputStream(target, CREATE_NEW).use {
                                    check(tar.copyTo(it) == entry.size) {
                                        "Truncated DSH backup entry"
                                    }
                                }
                                setMode(
                                    target,
                                    if (target == root.resolve(".credentials.yaml")) 384
                                    else entry.mode,
                                )
                            }
                            else -> error("Unsupported DSH backup entry")
                        }
                    }
                    // Read through the gzip trailer so CRC/truncation errors surface before
                    // restore.
                    val buffer = ByteArray(8192)
                    while (gzip.read(buffer) != -1) {}
                }
            }
            require(Files.isDirectory(root, NOFOLLOW_LINKS)) { "DSH backup is missing its root" }
            for ((path, mode) in directories.entries.toList().asReversed()) setMode(
                path,
                mode or 448,
            )
            return root.toFile()
        }

        private fun checkLink(root: Path, path: Path, name: String): Path {
            require(name.isNotEmpty() && '\\' !in name) { "Invalid DSH backup link" }
            val link = path.fileSystem.getPath(name)
            require(
                !link.isAbsolute &&
                    link == link.normalize() &&
                    path.parent.resolve(link).normalize().startsWith(root),
            ) {
                "DSH backup link leaves the DSH directory"
            }
            return link
        }

        private fun posix(path: Path) =
            Files.getFileAttributeView(path, PosixFileAttributeView::class.java, NOFOLLOW_LINKS)

        private fun setMode(path: Path, mode: Int) {
            posix(path)
                ?.setPermissions(
                    PosixFilePermission.values()
                        .filterIndexed { index, _ -> mode and (256 shr index) != 0 }
                        .toSet(),
                )
        }
    }
}

private class DshTarInputStream(input: InputStream) :
    TarArchiveInputStream(input, Charsets.UTF_8.name()) {
    override fun isEOFRecord(record: ByteArray?): Boolean {
        // Commons Compress normally accepts physical EOF or a single zero record.
        // Wingmate exports always include both zero records.
        if (record == null) throw IOException("Truncated DSH tar archive")
        val zeroRecord = super.isEOFRecord(record)
        if (isAtEOF && !zeroRecord) throw IOException("Invalid DSH tar trailer")
        return zeroRecord
    }
}
