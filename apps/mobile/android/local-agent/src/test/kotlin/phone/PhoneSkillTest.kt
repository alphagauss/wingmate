package dev.wingmate.mobile.runtime.phone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class PhoneSkillTest {
    @Rule @JvmField val temporary = TemporaryFolder()

    @Test
    fun skillToolIsAvailableOnlyWithTheLocalExtensionAndPhoneControlEnabled() {
        fun names(enabled: Boolean, extra: McpServer.Tool?): List<Any?> {
            val reply = Json.parse(McpServer.list(1, enabled, extra)) as Map<*, *>
            val result = reply["result"] as Map<*, *>
            return (result["tools"] as List<*>).map { (it as Map<*, *>)["name"] }
        }
        val base = names(true, null)
        assertFalse("save_skill" in base)
        assertEquals(base + "save_skill", names(true, PhoneSkill.tool))
        assertTrue(names(false, PhoneSkill.tool).isEmpty())
        val args =
            mapOf(
                "name" to "check-transit",
                "description" to "Check transit",
                "steps" to "Open the app.",
            )
        assertFalse(McpServer.accepts("save_skill", args))
        assertTrue(McpServer.accepts("save_skill", args, PhoneSkill.tool))
        assertFalse(McpServer.accepts("save_skill", args - "steps", PhoneSkill.tool))
    }

    @Test
    fun phoneSkillRequiresApprovalAndPublishesOnlyNewNames() {
        val home = temporary.newFolder("Wingmate", ".dsh")
        val draft = PhoneSkill.draft("check-transit", "Check transit", "1. Open the transit app.")
        val skill = home.resolve("skills/phone-check-transit/SKILL.md")
        assertThrows(PhoneProblem::class.java) { PhoneSkill.draft("../outside", "Title", "Step") }
        assertThrows(PhoneProblem::class.java) { PhoneSkill.install(home, draft) { false } }
        assertFalse(checkNotNull(skill.parentFile).exists())
        PhoneSkill.install(home, draft) { true }
        val saved = skill.readText()
        assertTrue(saved.contains("disable-model-invocation: true"))
        assertTrue(saved.contains(draft.steps))
        assertThrows(PhoneProblem::class.java) {
            PhoneSkill.install(home, draft.copy(steps = "Changed")) { true }
        }
        assertEquals(saved, skill.readText())
    }
}
