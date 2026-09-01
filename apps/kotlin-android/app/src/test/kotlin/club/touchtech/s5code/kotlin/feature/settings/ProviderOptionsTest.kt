package club.touchtech.s5code.kotlin.feature.settings

import club.touchtech.s5code.kotlin.model.ProviderCatalogEntry
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ProviderOptionChoice
import club.touchtech.s5code.kotlin.model.ProviderOptionDescriptor
import club.touchtech.s5code.kotlin.model.ProviderOptionSelection
import club.touchtech.s5code.kotlin.model.ProviderOptionValue
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ProviderOptionsTest {

    private val codex = ProviderInstance(instanceId = "codex", driver = "codex")
    private val claude = ProviderInstance(instanceId = "claudeAgent", driver = "claudeAgent")

    private fun effort(vararg ids: String) =
        ProviderOptionDescriptor.Select(
            id = "effort",
            label = "Reasoning",
            options =
                ids.mapIndexed { index, id ->
                    ProviderOptionChoice(id = id, label = id.uppercase(), isDefault = index == 1)
                },
        )

    private val fastMode = ProviderOptionDescriptor.Toggle(id = "fastMode", label = "Fast Mode")

    private val catalog =
        listOf(
            ProviderCatalogEntry(
                instance = codex,
                models = listOf("gpt-5-codex", "gpt-5"),
                optionDescriptors =
                    mapOf(
                        "gpt-5-codex" to listOf(effort("low", "medium", "high")),
                        // A second model of the same provider with a different set,
                        // which is the case a fixed effort row gets wrong.
                        "gpt-5" to listOf(effort("low", "medium"), fastMode),
                    ),
            ),
            ProviderCatalogEntry(instance = claude, models = listOf("opus")),
        )

    @Test
    fun `descriptors come from the selected model, not the provider`() {
        assertEquals(
            listOf("low", "medium", "high"),
            (providerOptionDescriptors(catalog, codex, "gpt-5-codex", emptyList()).single()
                    as ProviderOptionDescriptor.Select)
                .options
                .map { it.id },
        )
        assertEquals(
            listOf("effort", "fastMode"),
            providerOptionDescriptors(catalog, codex, "gpt-5", emptyList()).map { it.id },
        )
    }

    @Test
    fun `a model that advertises nothing gets no rows`() {
        assertTrue(providerOptionDescriptors(catalog, claude, "opus", emptyList()).isEmpty())
        // Not a fallback to some other model's knobs: an unknown slug means the
        // catalog has not caught up, and inventing rows would offer values the
        // provider never named.
        assertTrue(providerOptionDescriptors(catalog, codex, "gpt-6", emptyList()).isEmpty())
    }

    @Test
    fun `a stored value wins over the advertised default`() {
        val stored = listOf(ProviderOptionSelection("effort", ProviderOptionValue.Text("high")))
        val descriptor =
            providerOptionDescriptors(catalog, codex, "gpt-5-codex", stored).single()
                as ProviderOptionDescriptor.Select
        assertEquals("high", descriptor.effectiveValue)
        assertEquals("HIGH", descriptor.effectiveLabel)
    }

    @Test
    fun `with nothing stored the advertised default is in effect`() {
        val descriptor =
            providerOptionDescriptors(catalog, codex, "gpt-5-codex", emptyList()).single()
                as ProviderOptionDescriptor.Select
        assertEquals("medium", descriptor.effectiveValue)
    }

    @Test
    fun `a stored value the model no longer offers falls back to the default`() {
        // The thread was configured on a model with an `xhigh` effort and then moved.
        val stored = listOf(ProviderOptionSelection("effort", ProviderOptionValue.Text("xhigh")))
        val descriptor =
            providerOptionDescriptors(catalog, codex, "gpt-5-codex", stored).single()
                as ProviderOptionDescriptor.Select
        assertEquals("medium", descriptor.effectiveValue)
    }

    @Test
    fun `a stored value of the wrong type is ignored`() {
        val stored = listOf(ProviderOptionSelection("effort", ProviderOptionValue.Flag(true)))
        val descriptor =
            providerOptionDescriptors(catalog, codex, "gpt-5-codex", stored).single()
                as ProviderOptionDescriptor.Select
        assertEquals("medium", descriptor.effectiveValue)
    }

    @Test
    fun `a stored toggle is read back`() {
        val stored = listOf(ProviderOptionSelection("fastMode", ProviderOptionValue.Flag(true)))
        val toggle =
            providerOptionDescriptors(catalog, codex, "gpt-5", stored)
                .filterIsInstance<ProviderOptionDescriptor.Toggle>()
                .single()
        assertTrue(toggle.currentValue)
    }

    @Test
    fun `applying a change writes every descriptor, including defaults`() {
        // Every value is written, not just the changed one: a later change to the
        // model's default must not move a thread that was already configured.
        val descriptors = providerOptionDescriptors(catalog, codex, "gpt-5", emptyList())
        val next =
            applyProviderOption(descriptors, "effort", ProviderOptionValue.Text("low"))
                ?: error("expected the change to apply")
        assertEquals(
            listOf(
                ProviderOptionSelection("effort", ProviderOptionValue.Text("low")),
                ProviderOptionSelection("fastMode", ProviderOptionValue.Flag(false)),
            ),
            next,
        )
    }

    @Test
    fun `a value the model does not advertise is refused`() {
        val descriptors = providerOptionDescriptors(catalog, codex, "gpt-5-codex", emptyList())
        assertNull(applyProviderOption(descriptors, "effort", ProviderOptionValue.Text("ultrathink")))
    }

    @Test
    fun `an unknown descriptor id is refused`() {
        val descriptors = providerOptionDescriptors(catalog, codex, "gpt-5-codex", emptyList())
        assertNull(applyProviderOption(descriptors, "serviceTier", ProviderOptionValue.Text("fast")))
    }

    @Test
    fun `a boolean sent to a select is refused`() {
        val descriptors = providerOptionDescriptors(catalog, codex, "gpt-5-codex", emptyList())
        assertNull(applyProviderOption(descriptors, "effort", ProviderOptionValue.Flag(true)))
    }

    @Test
    fun `summary labels skip toggles that are off`() {
        val descriptors = providerOptionDescriptors(catalog, codex, "gpt-5", emptyList())
        assertEquals(listOf("MEDIUM"), providerOptionSummaryLabels(descriptors))

        val on =
            providerOptionDescriptors(
                catalog,
                codex,
                "gpt-5",
                listOf(ProviderOptionSelection("fastMode", ProviderOptionValue.Flag(true))),
            )
        assertEquals(listOf("MEDIUM", "Fast Mode"), providerOptionSummaryLabels(on))
    }

    @Test
    fun `a select with no default contributes no summary label`() {
        val catalog =
            listOf(
                ProviderCatalogEntry(
                    instance = codex,
                    models = listOf("gpt-5"),
                    optionDescriptors =
                        mapOf(
                            "gpt-5" to
                                listOf(
                                    ProviderOptionDescriptor.Select(
                                        id = "serviceTier",
                                        label = "Service Tier",
                                        options = listOf(ProviderOptionChoice("fast", "Fast")),
                                    )
                                )
                        ),
                )
            )
        val descriptors = providerOptionDescriptors(catalog, codex, "gpt-5", emptyList())
        assertTrue(providerOptionSummaryLabels(descriptors).isEmpty())
        // And applying still works, because the user picking is what sets it.
        assertEquals(
            listOf(ProviderOptionSelection("serviceTier", ProviderOptionValue.Text("fast"))),
            applyProviderOption(descriptors, "serviceTier", ProviderOptionValue.Text("fast")),
        )
    }
}
