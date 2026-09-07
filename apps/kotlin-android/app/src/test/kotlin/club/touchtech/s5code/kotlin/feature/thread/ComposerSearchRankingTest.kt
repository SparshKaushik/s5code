package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.model.SlashCommand
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ComposerSearchRankingTest {
    @Test
    fun `query scoring keeps exact prefix boundary include and fuzzy tiers ordered`() {
        val exact = scoreQueryMatch("model", "model", exactBase = 0, prefixBase = 2)
        val prefix = scoreQueryMatch("models", "model", exactBase = 0, prefixBase = 2)
        val boundary =
            scoreQueryMatch(
                "switch-model",
                "model",
                exactBase = 0,
                prefixBase = 2,
                boundaryBase = 4,
            )
        val include =
            scoreQueryMatch(
                "remodel",
                "model",
                exactBase = 0,
                prefixBase = 2,
                boundaryBase = 4,
                includesBase = 40,
            )
        val fuzzy =
            scoreQueryMatch(
                "message-of-day-end-label",
                "model",
                exactBase = 0,
                prefixBase = 2,
                boundaryBase = 4,
                includesBase = 40,
                fuzzyBase = 100,
            )

        assertTrue(exact!! < prefix!!)
        assertTrue(prefix < boundary!!)
        assertTrue(boundary < include!!)
        assertTrue(include < fuzzy!!)
        assertNull(scoreQueryMatch("plan", "xyz", exactBase = 0, fuzzyBase = 100))
    }

    @Test
    fun `slash commands match fuzzy names and descriptions with stable rank`() {
        val commands =
            listOf(
                SlashCommand("/default", "Switch interaction mode"),
                SlashCommand("/model", "Switch model"),
                SlashCommand("/review", "Inspect model output"),
                SlashCommand("/mode-long", "Unrelated"),
            )

        assertEquals(
            listOf("/model", "/mode-long"),
            rankSlashCommands(commands, "/modl").take(2).map { it.name },
        )
        assertEquals("/review", rankSlashCommands(commands, "/inspect").first().name)
    }

    @Test
    fun `paths rank leaf names before loose full-path subsequences`() {
        val paths =
            listOf(
                "docs/android/message-delivery.md",
                "apps/mobile/src/MessageDrawer.tsx",
                "apps/kotlin-android/app/src/main/Model.kt",
            )

        assertEquals(
            listOf(
                "apps/mobile/src/MessageDrawer.tsx",
                "docs/android/message-delivery.md",
            ),
            rankComposerPaths(paths, "@msgd").take(2),
        )
    }
}
