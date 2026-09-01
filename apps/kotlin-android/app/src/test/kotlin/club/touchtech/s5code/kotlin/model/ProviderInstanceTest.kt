package club.touchtech.s5code.kotlin.model

import club.touchtech.s5code.kotlin.data.providerInstanceForId
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Provider naming.
 *
 * The bug this guards against shipped: driver kinds are open on the wire, the
 * client held a closed enum, and every slug it did not know collapsed onto the
 * first entry — so `pi` threads were labeled "Codex". The tests that matter here
 * are the unknown-driver ones.
 */
class ProviderInstanceTest {

    @Test
    fun `known drivers use the shared display names`() {
        assertEquals("Codex", formatProviderDriverName("codex"))
        assertEquals("Claude", formatProviderDriverName("claudeAgent"))
        assertEquals("Claude", formatProviderDriverName("claude"))
        assertEquals("Cursor", formatProviderDriverName("cursor"))
        assertEquals("Grok", formatProviderDriverName("grok"))
        assertEquals("OpenCode", formatProviderDriverName("opencode"))
    }

    @Test
    fun `pi keeps its lowercase brand`() {
        assertEquals("pi", formatProviderDriverName("pi"))
    }

    @Test
    fun `an unknown driver is title-cased rather than mislabeled`() {
        assertEquals("Gemini", formatProviderDriverName("gemini"))
        // The `Agent` suffix is an internal naming convention, not part of the name.
        assertEquals("Mistral", formatProviderDriverName("mistralAgent"))
    }

    @Test
    fun `a missing driver reads as an agent, never as Codex`() {
        assertEquals("This agent", formatProviderDriverName(null))
        assertEquals("This agent", formatProviderDriverName(""))
    }

    @Test
    fun `an instance label prefers the user's own name`() {
        assertEquals(
            "Work Codex",
            ProviderInstance(instanceId = "codex-work", driver = "codex", displayName = "Work Codex")
                .label,
        )
        // Whitespace is not a name.
        assertEquals(
            "Codex",
            ProviderInstance(instanceId = "codex-work", driver = "codex", displayName = "   ").label,
        )
    }

    @Test
    fun `an instance with no describable driver falls back to its id`() {
        assertEquals("codex-work", ProviderInstance(instanceId = "codex-work", driver = "").label)
    }

    @Test
    fun `an unexplained instance id guesses only the driver`() {
        val instance = providerInstanceForId("pi")
        assertEquals("pi", instance.instanceId)
        assertEquals("pi", instance.driver)
        assertEquals("pi", instance.label)
    }

    @Test
    fun `an unexplained instance id is never rewritten`() {
        // The id is the routing key: guessing a driver for the glyph is fine,
        // rewriting the id would send turns to a different agent.
        val instance = providerInstanceForId("my-own-runtime")
        assertEquals("my-own-runtime", instance.instanceId)
        assertEquals("My-own-runtime", instance.label)
    }
}
