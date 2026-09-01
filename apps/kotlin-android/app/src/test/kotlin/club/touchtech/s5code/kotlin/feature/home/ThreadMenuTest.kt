package club.touchtech.s5code.kotlin.feature.home

import club.touchtech.s5code.kotlin.model.ThreadStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ThreadMenuTest {

    private val thread = HomeFixture.threads.first()

    @Test
    fun `title regeneration is omitted when the environment does not support it`() {
        assertTrue(threadMenuOptions(thread, titleRegenerationSupported = false).none {
            it.id == "regenerate-title"
        })
    }

    @Test
    fun `supported title regeneration is actionable at rest`() {
        val option =
            threadMenuOptions(thread, titleRegenerationSupported = true)
                .single { it.id == "regenerate-title" }
        assertEquals("Regenerate title", option.label)
        assertTrue(option.enabled)
    }

    @Test
    fun `in-flight title regeneration is labelled and disabled`() {
        val option =
            threadMenuOptions(
                    thread.copy(titleRegenerating = true, status = ThreadStatus.Working),
                    titleRegenerationSupported = true,
                )
                .single { it.id == "regenerate-title" }
        assertEquals("Regenerating…", option.label)
        assertFalse(option.enabled)
    }
}
