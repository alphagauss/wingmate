package dev.wingmate.mobile.runtime.phone

import java.io.File
import java.nio.file.Files
import java.nio.file.LinkOption.NOFOLLOW_LINKS
import java.nio.file.StandardCopyOption.ATOMIC_MOVE
import java.nio.file.StandardCopyOption.REPLACE_EXISTING
import java.nio.file.attribute.PosixFilePermissions

// Adapted from dsh-Go's PhoneGuest (AGPLv3).
internal object PhoneGuest {
    private const val DIR = "/opt/wingmate-phone"
    private const val WEB_PATCH = "--patch /etc/dsh-app/dsh-web.patch.yml"

    fun launchArg(arg: String) = arg.replace(WEB_PATCH, "$WEB_PATCH --patch $DIR/phone.patch.yml")

    fun write(rootfs: File, socket: String, token: String) {
        val opt = rootfs.resolve("opt").toPath()
        check(Files.isDirectory(opt, NOFOLLOW_LINKS))
        val dir = opt.resolve("wingmate-phone")
        if (!Files.exists(dir, NOFOLLOW_LINKS)) Files.createDirectory(dir)
        check(Files.isDirectory(dir, NOFOLLOW_LINKS))
        fun write(name: String, value: String) {
            val temp =
                Files.createTempFile(
                    dir,
                    ".phone-",
                    null,
                    PosixFilePermissions.asFileAttribute(
                        PosixFilePermissions.fromString("rw-------"),
                    ),
                )
            try {
                Files.write(temp, value.toByteArray(Charsets.UTF_8))
                Files.move(temp, dir.resolve(name), ATOMIC_MOVE, REPLACE_EXISTING)
            } finally {
                Files.deleteIfExists(temp)
            }
        }

        write("relay.cjs", relay)
        write("phone.patch.yml", patch)
        write("bridge.json", Json.write(mapOf("socket" to socket, "token" to token)))
    }

    fun clear(rootfs: File) {
        val opt = rootfs.resolve("opt").toPath()
        val dir = opt.resolve("wingmate-phone")
        if (Files.isDirectory(opt, NOFOLLOW_LINKS) && Files.isDirectory(dir, NOFOLLOW_LINKS))
            Files.deleteIfExists(dir.resolve("bridge.json"))
    }

    private val patch =
        """
        - insert:
            - id: wingmate-phone
              name: '@deepseek-ai/dsh-mcp-client'
              config:
                serverName: phone
                transport: stdio
                command: !!js process.execPath
                args: [$DIR/relay.cjs]
                # Allow 30 seconds for approval and 12 seconds for background screen startup.
                toolCallTimeoutMs: 45000
    """
            .trimIndent() + "\n"

    private val relay =
        """
        'use strict'
        const fs = require('node:fs')
        const net = require('node:net')
        const config = JSON.parse(fs.readFileSync('$DIR/bridge.json', 'utf8'))
        const socket = net.connect({ path: '\0' + config.socket })
        const timeout = setTimeout(() => socket.destroy(new Error('connect timeout')), 3000)
        socket.once('connect', () => {
          clearTimeout(timeout)
          socket.write(JSON.stringify({ dshPhone: 1, token: config.token }) + '\n')
          process.stdin.pipe(socket)
          socket.pipe(process.stdout)
        })
        socket.on('error', error => {
          process.stderr.write('Phone bridge: ' + (error.code || error.message) + '\n')
          process.exitCode = 1
        })
        socket.on('close', () => {
          clearTimeout(timeout)
          process.stdin.pause()
        })
        process.stdout.on('error', error => socket.destroy(error))
    """
            .trimIndent() + "\n"
}
