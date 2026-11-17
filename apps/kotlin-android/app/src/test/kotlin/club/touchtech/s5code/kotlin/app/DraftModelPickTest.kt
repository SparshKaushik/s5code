package club.touchtech.s5code.kotlin.app

import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ProviderOptionSelection
import club.touchtech.s5code.kotlin.model.ProviderOptionValue
import club.touchtech.s5code.kotlin.model.ThreadSettings
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** `draftModelPickMatchesThread` from `packages/shared/src/model.ts`. */
class DraftModelPickTest {

    private val codex = ProviderInstance("codex", "codex")
    private val thread =
        ThreadSettings(
            provider = codex,
            model = "gpt-5.5",
            options = listOf(ProviderOptionSelection("reasoningEffort", ProviderOptionValue.Text("high"))),
        )

    @Test
    fun `matches the same instance, model, and options in any order`() {
        val reordered =
            thread.copy(
                options = listOf(
                    ProviderOptionSelection("verbosity", ProviderOptionValue.Text("low")),
                    ProviderOptionSelection("reasoningEffort", ProviderOptionValue.Text("high")),
                ),
            )
        val threadWithBoth = thread.copy(options = reordered.options.reversed())
        assertTrue(draftModelPickMatchesThread(reordered, threadWithBoth))
        // A display-name difference is not a different instance.
        assertTrue(draftModelPickMatchesThread(thread.copy(provider = codex.copy(displayName = "Work")), thread))
    }

    @Test
    fun `fast mode off counts as unset, on does not`() {
        val fastOff =
            thread.copy(options = thread.options + ProviderOptionSelection("fastMode", ProviderOptionValue.Flag(false)))
        val fastOn =
            thread.copy(options = thread.options + ProviderOptionSelection("fastMode", ProviderOptionValue.Flag(true)))
        assertTrue(draftModelPickMatchesThread(fastOff, thread))
        assertTrue(draftModelPickMatchesThread(thread, fastOff))
        assertFalse(draftModelPickMatchesThread(fastOn, thread))
    }

    @Test
    fun `a different instance, model, or option value is a real pick`() {
        assertFalse(draftModelPickMatchesThread(thread.copy(provider = ProviderInstance("codex_work", "codex")), thread))
        assertFalse(draftModelPickMatchesThread(thread.copy(model = "gpt-5.4"), thread))
        assertFalse(
            draftModelPickMatchesThread(
                thread.copy(options = listOf(ProviderOptionSelection("reasoningEffort", ProviderOptionValue.Text("low")))),
                thread,
            ),
        )
    }
}
