package club.touchtech.s5code.kotlin.feature.settings

import club.touchtech.s5code.kotlin.model.ProviderCatalogEntry
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ProviderOptionChoice
import club.touchtech.s5code.kotlin.model.ProviderOptionDescriptor
import club.touchtech.s5code.kotlin.model.ProviderOptionSelection
import club.touchtech.s5code.kotlin.model.ProviderOptionValue
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ModelTraitsTest {
    private val claude = ProviderInstance("claudeAgent", "claudeAgent")
    private val codex = ProviderInstance("codex", "codex")

    private val effort =
        ProviderOptionDescriptor.Select(
            id = "effort",
            label = "Reasoning",
            options =
                listOf("low", "medium", "high", "ultrathink", "ultracode").map {
                    ProviderOptionChoice(it, it.replaceFirstChar(Char::uppercase), isDefault = it == "high")
                },
            promptInjectedValues = listOf("ultrathink"),
        )
    private val descriptors = listOf(effort)

    @Test
    fun `ultrathink goes into the prompt instead of the stored options`() {
        val change = traitSelectChange(descriptors, effort, "solve it", "ultrathink")
        assertEquals(TraitSelectChange("Ultrathink:\nsolve it"), change)
        assertTrue(promptControlledTraits(descriptors, change!!.prompt).ultrathinkPromptControlled)
        assertEquals(TraitSelectChange("Ultrathink:\n"), traitSelectChange(descriptors, effort, "", "ultrathink"))
    }

    @Test
    fun `a regular effort removes the prefix and ultracode saves as an option`() {
        assertEquals(
            TraitSelectChange(
                prompt = "solve it",
                options = listOf(ProviderOptionSelection("effort", ProviderOptionValue.Text("ultracode"))),
            ),
            traitSelectChange(descriptors, effort, "Ultrathink:\nsolve it", "ultracode"),
        )
    }

    @Test
    fun `ultrathink in the body locks the effort and slash commands stay runnable`() {
        val prompt = "please ultrathink about this"
        assertTrue(promptControlledTraits(descriptors, prompt).ultrathinkInBodyText)
        assertNull(traitSelectChange(descriptors, effort, prompt, "low"))
        assertEquals(TraitSelectChange("/review"), traitSelectChange(descriptors, effort, "/review", "ultrathink"))
    }

    @Test
    fun `a model without prompt-injected values never reads the prompt`() {
        val plain = effort.copy(promptInjectedValues = emptyList())
        assertFalse(promptControlledTraits(listOf(plain), "ultrathink").ultrathinkPromptControlled)
    }

    @Test
    fun `summary shows ultrathink and folds fast mode into the reasoning label`() {
        val fast = ProviderOptionDescriptor.Toggle("fastMode", "Fast Mode", currentValue = true)
        val withFast = listOf(effort, fast)
        assertEquals("High Fast", traitsSummaryLabel(claude, withFast, promptControlledTraits(withFast, "go")))
        assertEquals(
            "Ultrathink Fast",
            traitsSummaryLabel(claude, withFast, promptControlledTraits(withFast, "Ultrathink:\ngo")),
        )
        val onlyFast = listOf(fast.copy(currentValue = false))
        assertEquals("Normal", traitsSummaryLabel(claude, onlyFast, promptControlledTraits(onlyFast, "")))
    }

    @Test
    fun `codex service tier reads as a speed label`() {
        val tier =
            ProviderOptionDescriptor.Select(
                id = "serviceTier",
                label = "Speed",
                options = listOf(ProviderOptionChoice("default", "Normal", isDefault = true), ProviderOptionChoice("priority", "Fast")),
                currentValue = "priority",
            )
        val reasoning =
            ProviderOptionDescriptor.Select(
                id = "reasoningEffort",
                label = "Reasoning",
                options = listOf(ProviderOptionChoice("medium", "Medium", isDefault = true)),
            )
        val all = listOf(reasoning, tier)
        assertEquals("Medium Fast", traitsSummaryLabel(codex, all, promptControlledTraits(all, "")))
    }

    @Test
    fun `a locked thread keeps accounts in its continuation group`() {
        fun entry(id: String, driver: String, group: String?) =
            ProviderCatalogEntry(ProviderInstance(id, driver), models = listOf("m"), continuationGroupKey = group)
        val codexEntry = entry("codex", "codex", "shared")
        val catalog =
            listOf(
                codexEntry,
                entry("codex_work", "codex", "shared"),
                entry("codex_other", "codex", "other"),
                entry("claude", "claudeAgent", "shared"),
            )
        assertEquals(
            listOf("codex", "codex_work"),
            catalog.filter { sharesLockedProvider(codexEntry, codexEntry.instance, it) }.map { it.instance.instanceId },
        )
        val google = entry("google", "antigravity", null)
        val ungrouped = listOf(entry("codex", "codex", null), entry("codex_work", "codex", null), google, entry("google_work", "antigravity", null))
        assertEquals(
            listOf("codex", "codex_work"),
            ungrouped.filter { sharesLockedProvider(ungrouped[0], ungrouped[0].instance, it) }.map { it.instance.instanceId },
        )
        assertEquals(
            listOf("google"),
            ungrouped.filter { sharesLockedProvider(google, google.instance, it) }.map { it.instance.instanceId },
        )
    }
}
