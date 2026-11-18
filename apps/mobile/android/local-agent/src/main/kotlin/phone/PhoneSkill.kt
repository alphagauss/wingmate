package dev.wingmate.mobile.runtime.phone

import android.content.Context
import android.os.Handler
import android.os.Looper
import dev.wingmate.mobile.runtime.R
import dev.wingmate.mobile.runtime.phone.McpServer.field
import java.io.File
import java.nio.file.FileAlreadyExistsException
import java.nio.file.Files
import java.nio.file.LinkOption.NOFOLLOW_LINKS
import java.nio.file.StandardCopyOption.ATOMIC_MOVE
import kotlin.concurrent.thread

// Local skill publication adapted from dsh-Go's PhoneSkillDraft and PhoneSkillInstaller.
internal object PhoneSkill {
    val tool =
        McpServer.Tool(
            "save_skill",
            "When the user asks to save a proven phone workflow for reuse, propose a local DSH skill and wait for their approval on the phone. Write reusable steps with visible target names and success checks. Omit personal values, credentials, and old screen IDs, node numbers or pixel coordinates.",
            mapOf(
                "name" to
                    field("string", "Short lowercase ASCII name with hyphen-separated words."),
                "description" to field("string", "One-line purpose of the workflow."),
                "steps" to
                    field(
                        "string",
                        "Markdown steps, up to 2000 characters. Use placeholders for user input.",
                    ),
            ),
            "name",
            "description",
            "steps",
        )

    fun prepare(context: Context, rootfs: File, args: Map<*, *>): PhoneControl.Action {
        val draft =
            draft(args["name"] as String, args["description"] as String, args["steps"] as String)
        val summary =
            context.getString(
                R.string.phone_control_save_skill,
                draft.name,
                draft.description,
                draft.steps,
            )
        val output =
            McpServer.Output(
                "Saved the local DSH skill ${draft.name}. " +
                    "Choose it from the skills list when you want to run the workflow.",
            )
        return PhoneControl.Action(summary, output) { live, complete ->
            thread(name = "wingmate-phone-skill", isDaemon = true) {
                val result = runCatching {
                    phoneCheck(live(), "The phone skill request was cancelled.")
                    install(rootfs, draft, live)
                    true
                }
                Handler(Looper.getMainLooper()).post { complete(result) }
            }
        }
    }

    data class Draft(val name: String, val description: String, val steps: String) {
        fun markdown(): String =
            "---\nname: $name\n" +
                "description: ${Json.write(description)}\n" +
                "disable-model-invocation: true\nuser-invocable: true\n---\n\n" +
                "$steps\n\n" +
                "Use the phone MCP tools and read the current screen before each action. " +
                "Resolve element numbers from that observation. Ask the user for any missing input " +
                "and follow the phone's approval prompts. If a target or outcome is unclear, stop and ask.\n"
    }

    fun draft(name: String, description: String, steps: String): Draft {
        phoneCheck(
            name.length <= 44 && name.matches(Regex("[a-z0-9]+(?:-[a-z0-9]+)*")),
            "Use a lowercase skill name such as check-transit, up to 44 characters.",
        )
        val summary = description.trim()
        val body = steps.trim()
        phoneCheck(
            summary.length in 1..140 && summary.none(Char::isISOControl),
            "Provide a one-line skill description of up to 140 characters.",
        )
        phoneCheck(
            body.isNotEmpty() && body.none { it.isISOControl() && it != '\n' && it != '\t' },
            "Provide readable Markdown steps for the skill.",
        )
        return Draft("phone-$name", summary, body)
    }

    fun install(rootfs: File, draft: Draft, approved: () -> Boolean) {
        val root = rootfs.toPath()
        val home = root.resolve("root/.dsh")
        phoneCheck(
            listOf(root.parent, root, root.resolve("root"), home).all {
                Files.isDirectory(it, NOFOLLOW_LINKS)
            },
            "Start the local DSH engine before saving a phone skill.",
        )
        val skills = home.resolve("skills")
        Files.createDirectories(skills)
        phoneCheck(
            Files.isDirectory(skills, NOFOLLOW_LINKS),
            "The local DSH skills directory is unavailable.",
        )
        val target = skills.resolve(draft.name)
        try {
            Files.createDirectory(target)
        } catch (_: FileAlreadyExistsException) {
            throw PhoneProblem("That phone skill already exists. Choose another name.")
        }
        var temporary: java.nio.file.Path? = null
        try {
            val staged = Files.createTempFile(target, ".skill-", ".tmp")
            temporary = staged
            Files.write(staged, draft.markdown().toByteArray(Charsets.UTF_8))
            phoneCheck(approved(), "The phone skill request was cancelled.")
            Files.move(staged, target.resolve("SKILL.md"), ATOMIC_MOVE)
        } catch (error: Exception) {
            temporary?.let { runCatching { Files.deleteIfExists(it) } }
            runCatching { Files.deleteIfExists(target) }
            throw error
        }
    }
}
