package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ProviderOptionDescriptor
import club.touchtech.s5code.kotlin.model.ProviderOptionSelection
import club.touchtech.s5code.kotlin.model.ProviderOptionValue
import club.touchtech.s5code.kotlin.transport.wire.ModelCapabilitiesDto
import club.touchtech.s5code.kotlin.transport.wire.ProviderOptionChoiceDto
import club.touchtech.s5code.kotlin.transport.wire.ProviderOptionDescriptorDto
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.addJsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Decoding the provider-option half of the wire.
 *
 * Both stored shapes have to read: the contract still accepts the pre-migration
 * `{ effort: "max" }` object, and a server that has not run migration 026 would
 * otherwise render every knob at its default while the thread runs with something
 * else.
 */
class ProviderOptionWireTest {

    @Test
    fun `the canonical array shape decodes`() {
        val options = buildJsonArray {
            addJsonObject {
                put("id", "effort")
                put("value", "max")
            }
            addJsonObject {
                put("id", "fastMode")
                put("value", true)
            }
        }
        assertEquals(
            listOf(
                ProviderOptionSelection("effort", ProviderOptionValue.Text("max")),
                ProviderOptionSelection("fastMode", ProviderOptionValue.Flag(true)),
            ),
            providerOptionSelections(options),
        )
    }

    @Test
    fun `the legacy object shape decodes`() {
        val options = buildJsonObject {
            put("effort", "max")
            put("fastMode", false)
        }
        assertEquals(
            listOf(
                ProviderOptionSelection("effort", ProviderOptionValue.Text("max")),
                ProviderOptionSelection("fastMode", ProviderOptionValue.Flag(false)),
            ),
            providerOptionSelections(options),
        )
    }

    @Test
    fun `an absent or malformed options block is empty, not an error`() {
        assertTrue(providerOptionSelections(null).isEmpty())
        assertTrue(providerOptionSelections(JsonPrimitive("max")).isEmpty())
        // An array entry with no id is skipped rather than taking the list with it.
        val partial = buildJsonArray {
            addJsonObject { put("value", "max") }
            addJsonObject {
                put("id", "effort")
                put("value", "low")
            }
        }
        assertEquals(
            listOf(ProviderOptionSelection("effort", ProviderOptionValue.Text("low"))),
            providerOptionSelections(partial),
        )
    }

    @Test
    fun `descriptors decode with their choices and current value`() {
        val caps =
            ModelCapabilitiesDto(
                optionDescriptors =
                    listOf(
                        ProviderOptionDescriptorDto(
                            id = "effort",
                            label = "Reasoning",
                            type = "select",
                            options =
                                listOf(
                                    ProviderOptionChoiceDto(id = "low", label = "Low"),
                                    ProviderOptionChoiceDto(
                                        id = "high",
                                        label = "High",
                                        description = "Slower",
                                        isDefault = true,
                                    ),
                                ),
                            currentValue = JsonPrimitive("high"),
                        ),
                        ProviderOptionDescriptorDto(
                            id = "fastMode",
                            label = "Fast Mode",
                            type = "boolean",
                            currentValue = JsonPrimitive(true),
                        ),
                    )
            )
        val descriptors = optionDescriptorsFrom(caps)
        val select = descriptors.first() as ProviderOptionDescriptor.Select
        assertEquals("high", select.effectiveValue)
        assertEquals("Slower", select.options.last().description)
        assertTrue((descriptors.last() as ProviderOptionDescriptor.Toggle).currentValue)
    }

    @Test
    fun `unknown descriptor types and empty selects are dropped`() {
        val caps =
            ModelCapabilitiesDto(
                optionDescriptors =
                    listOf(
                        // A shape a later contract adds. Rendered as a select it
                        // would be a row with nothing to pick.
                        ProviderOptionDescriptorDto(id = "budget", label = "Budget", type = "number"),
                        ProviderOptionDescriptorDto(id = "tier", label = "Tier", type = "select"),
                        ProviderOptionDescriptorDto(id = "", label = "Nameless", type = "boolean"),
                    )
            )
        assertTrue(optionDescriptorsFrom(caps).isEmpty())
    }

    @Test
    fun `a model with no capabilities block has no knobs`() {
        assertTrue(optionDescriptorsFrom(null).isEmpty())
        assertTrue(optionDescriptorsFrom(ModelCapabilitiesDto()).isEmpty())
    }
}
