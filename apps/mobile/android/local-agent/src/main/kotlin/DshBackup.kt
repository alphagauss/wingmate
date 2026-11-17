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

/** The old DSH home stays on the same filesystem until its replacement is durable. */
internal class DshBackup(guestHome: File, private val sync: () -> Unit = ::syncFilesystems) {
    private val home = guestHome.resolve(".dsh")
    private val rollback = guestHome.resolve(".dsh-restore-rollback")
    private val previous = guestHome.resolve(".dsh-before-restore")
    private val linkStore = guestHome.resolve(".l2s")

    fun hasData(): Boolean = home.isDirectory || rollback.isDirectory || previous.isDirectory

    fun moveInto(destination: DshBackup) {
        recover()
        if (home.isDirectory) destination.restore(home)
    }

    fun recover() {
        if (Files.exists(home.toPath(), NOFOLLOW_LINKS)) {
            if (Files.exists(rollback.toPath(), NOFOLLOW_LINKS)) {
                sync()
                deleteRuntimeTree(rollback)
            }
            return
        }
        val saved =
            when {
                Files.exists(rollback.toPath(), NOFOLLOW_LINKS) -> rollback
                Files.isDirectory(previous.toPath(), NOFOLLOW_LINKS) -> previous
                else -> return
            }
        Files.move(saved.toPath(), home.toPath(), ATOMIC_MOVE)
        sync()
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
        val link = if (Files.isSymbolicLink(path)) Files.readSymbolicLink(path) else null
        val contents = link?.let(::prootBackingFile) ?: path
        val regular = Files.isRegularFile(contents, NOFOLLOW_LINKS)
        val entry =
            when {
                directory -> TarArchiveEntry("$name/")
                regular -> TarArchiveEntry(name).apply { size = Files.size(contents) }
                link != null -> {
                    checkLink(home.toPath(), path, link.toString())
                    TarArchiveEntry(name, TarConstants.LF_SYMLINK).apply {
                        linkName = link.toString()
                    }
                }
                else -> error("Unsupported DSH backup entry")
            }
        val permissions = posix(contents)?.readAttributes()?.permissions().orEmpty()
        entry.mode =
            if (permissions.isEmpty()) {
                if (directory) 448 else 384 // 0700 and 0600
            } else {
                PosixFilePermission.values().foldIndexed(0) { index, mode, permission ->
                    if (permission in permissions) mode or (256 shr index) else mode
                }
            }
        tar.putArchiveEntry(entry)
        if (regular) Files.newInputStream(contents, NOFOLLOW_LINKS).use { it.copyTo(tar) }
        tar.closeArchiveEntry()
        if (directory) {
            for (child in checkNotNull(file.listFiles())) write(tar, child, "$name/${child.name}")
        }
    }

    private fun prootBackingFile(link: Path): Path? {
        // PRoot emulates hard links using absolute symlinks into its private .l2s directory.
        if (!link.isAbsolute || !Files.isDirectory(linkStore.toPath(), NOFOLLOW_LINKS)) return null
        val store = linkStore.toPath().toRealPath()
        if (link.parent != store || !link.fileName.toString().startsWith(".l2s.")) return null
        val target = link.toRealPath()
        check(
            target.parent == store &&
                target.fileName.toString().startsWith(".l2s.") &&
                Files.isRegularFile(target, NOFOLLOW_LINKS),
        ) {
            "Invalid PRoot backup backing file"
        }
        return target
    }

    fun restore(staged: File) {
        check(Files.isDirectory(staged.toPath(), NOFOLLOW_LINKS)) { "Staged DSH backup is missing" }
        recover()
        sync()
        if (Files.exists(home.toPath(), NOFOLLOW_LINKS)) {
            Files.move(home.toPath(), rollback.toPath(), ATOMIC_MOVE)
        }
        try {
            Files.move(staged.toPath(), home.toPath(), ATOMIC_MOVE)
        } catch (error: Exception) {
            recover()
            throw error
        }
        sync()
        deleteRuntimeTree(rollback)
    }

    companion object {
        /** Expands the selected archive into an empty private directory before confirmation. */
        fun unpack(input: InputStream, staging: File): File {
            val root = staging.resolve(".dsh").toPath()
            val directories = linkedMapOf<Path, Int>()
            GZIPInputStream(input, 64 * 1024).use { gzip ->
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
