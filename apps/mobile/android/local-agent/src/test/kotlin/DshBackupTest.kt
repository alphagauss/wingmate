package dev.wingmate.mobile.runtime

import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.nio.file.attribute.PosixFilePermissions
import java.util.zip.GZIPInputStream
import java.util.zip.GZIPOutputStream
import org.apache.commons.compress.archivers.tar.TarArchiveEntry
import org.apache.commons.compress.archivers.tar.TarArchiveOutputStream
import org.apache.commons.compress.archivers.tar.TarConstants
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Test

class DshBackupTest {
    private val base = Files.createTempDirectory("dsh-backup").toFile()
    private val live = File(base, "root/.dsh").apply { mkdirs() }
    private val rollback = File(live.parentFile, ".dsh-restore-rollback")
    private val previous = File(live.parentFile, ".dsh-before-restore")
    private val staging = File(base, "staging").apply { mkdir() }
    private val backup = DshBackup(live.parentFile!!) {}

    @After
    fun cleanup() {
        deleteRuntimeTree(base)
    }

    @Test
    fun roundTripRestoresHiddenCredentialsAndSessionsAndRemovesThePreviousHome() {
        val credentials = File(live, ".credentials.yaml").apply { writeText("version: 1") }
        val permissions = PosixFilePermissions.fromString("rw-------")
        val posix = Files.getFileStore(live.toPath()).supportsFileAttributeView("posix")
        if (posix) Files.setPosixFilePermissions(credentials.toPath(), permissions)
        val relative = "会话/${"项目".repeat(40)}.jsonl"
        val contents = byteArrayOf(0, 1, -1)
        File(live, relative).apply {
            parentFile!!.mkdir()
            writeBytes(contents)
        }
        val bytes = ByteArrayOutputStream().also(backup::export).toByteArray()
        credentials.writeText("before restore")
        val prepared = DshBackup.unpack(bytes.inputStream(), staging)

        assertEquals("before restore", credentials.readText())
        backup.restore(prepared)
        assertEquals("version: 1", credentials.readText())
        assertArrayEquals(contents, File(live, relative).readBytes())
        assertFalse(rollback.exists())
        if (posix) assertEquals(permissions, Files.getPosixFilePermissions(credentials.toPath()))
    }

    @Test
    fun rejectsPathsOutsideTheDshTreeAndLeavesLiveDataIntact() {
        val credentials = File(live, ".credentials.yaml").apply { writeText("original") }
        val bytes = archive(TarArchiveEntry(".dsh/../escape"))

        assertThrows(IllegalArgumentException::class.java) {
            DshBackup.unpack(bytes.inputStream(), staging)
        }

        assertFalse(File(staging, "escape").exists())
        assertEquals("original", credentials.readText())
    }

    @Test
    fun rejectsALinkToAPathOutsideTheDshTree() {
        val link =
            TarArchiveEntry(".dsh/link", TarConstants.LF_SYMLINK).apply {
                linkName = "../outside"
            }

        assertThrows(IllegalArgumentException::class.java) {
            DshBackup.unpack(archive(link).inputStream(), staging)
        }

        assertFalse(File(staging, "outside").exists())
    }

    @Test
    fun rejectsAnArchiveWithADamagedGzipTrailer() {
        val bytes = archive(TarArchiveEntry(".dsh/settings.yml"))
        bytes[bytes.size - 8] = (bytes[bytes.size - 8].toInt() xor 1).toByte()

        assertThrows(IOException::class.java) {
            DshBackup.unpack(bytes.inputStream(), staging)
        }
    }

    @Test
    fun rejectsATruncatedTarEvenWhenItsGzipChecksumIsValid() {
        val fullTar =
            GZIPInputStream(archive(TarArchiveEntry(".dsh/settings.yml")).inputStream()).use {
                it.readBytes()
            }
        // Two entry headers followed by only one of the two end-of-archive records.
        val bytes =
            ByteArrayOutputStream().also { output ->
                GZIPOutputStream(output).use { it.write(fullTar, 0, 3 * 512) }
            }

        assertThrows(IOException::class.java) {
            DshBackup.unpack(bytes.toByteArray().inputStream(), staging)
        }
    }

    @Test
    fun rollsBackIfTheStagedHomeDisappearsBeforeItCanBeMoved() {
        val current = File(live, "settings.yml").apply { writeText("original") }
        val prepared = File(staging, ".dsh").apply { mkdir() }
        val restore = DshBackup(live.parentFile!!) { deleteRuntimeTree(prepared) }

        assertThrows(IOException::class.java) { restore.restore(prepared) }

        assertEquals("original", current.readText())
        assertFalse(rollback.exists())
    }

    @Test
    fun recoversThePreviousHomeIfRestoreWasInterruptedBeforeInstallingTheReplacement() {
        File(live, "settings.yml").writeText("original")
        Files.move(live.toPath(), rollback.toPath())

        backup.recover()

        assertEquals("original", File(live, "settings.yml").readText())
        assertFalse(rollback.exists())
    }

    @Test
    fun removesThePreviousHomeIfRestoreWasInterruptedAfterInstallingTheReplacement() {
        File(live, "settings.yml").writeText("replacement")
        rollback.mkdir()
        File(rollback, "settings.yml").writeText("original")

        backup.recover()

        assertEquals("replacement", File(live, "settings.yml").readText())
        assertFalse(rollback.exists())
    }

    @Test
    fun leavesTheBackupSavedByAnEarlierVersionWhenThereIsALiveHome() {
        File(live, "settings.yml").writeText("current")
        previous.mkdir()
        val saved = File(previous, "settings.yml").apply { writeText("saved") }

        backup.recover()

        assertEquals("current", File(live, "settings.yml").readText())
        assertEquals("saved", saved.readText())
    }

    @Test
    fun recoversThePreviousHomeIfAnEarlierVersionWasInterruptedBeforeReplacement() {
        File(live, "settings.yml").writeText("original")
        Files.move(live.toPath(), previous.toPath())

        backup.recover()

        assertEquals("original", File(live, "settings.yml").readText())
        assertFalse(previous.exists())
    }

    private fun archive(entry: TarArchiveEntry): ByteArray {
        val bytes = ByteArrayOutputStream()
        TarArchiveOutputStream(GZIPOutputStream(bytes)).use { tar ->
            tar.putArchiveEntry(TarArchiveEntry(".dsh/"))
            tar.closeArchiveEntry()
            tar.putArchiveEntry(entry)
            tar.closeArchiveEntry()
        }
        return bytes.toByteArray()
    }
}
