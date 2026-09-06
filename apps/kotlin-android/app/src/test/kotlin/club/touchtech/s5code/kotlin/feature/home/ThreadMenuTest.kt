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

    @Test
    fun `snooze offers submenu presets on an active thread`() {
        val workingThread = thread.copy(status = ThreadStatus.Working)
        val snooze = threadMenuOptions(workingThread, titleRegenerationSupported = false).single { it.id == "snooze" }
        assertEquals("Snooze", snooze.label)
        assertTrue(snooze.enabled)
        assertTrue(snooze.children.isNotEmpty())
        assertTrue(snooze.children.any { it.label == "In 1 hour" })
        assertTrue(snooze.children.any { it.label == "Tomorrow" })
        assertTrue(snooze.children.any { it.label == "Next week" })
        assertTrue(snooze.children.all { it.id.startsWith("snooze:") })
    }

    @Test
    fun `snooze is disabled when thread is awaiting approval`() {
        val approvalThread = thread.copy(status = ThreadStatus.AwaitingApproval)
        val snooze = threadMenuOptions(approvalThread, titleRegenerationSupported = false).single { it.id == "snooze" }
        assertEquals("Snooze", snooze.label)
        assertFalse(snooze.enabled)
        assertTrue(snooze.children.isEmpty())
    }

    @Test
    fun `snoozed thread offers unsnooze without children`() {
        val snoozedThread = thread.copy(status = ThreadStatus.Snoozed)
        val options = threadMenuOptions(snoozedThread, titleRegenerationSupported = false)
        assertTrue(options.none { it.id == "snooze" })
        val unsnooze = options.single { it.id == "unsnooze" }
        assertEquals("Unsnooze", unsnooze.label)
        assertTrue(unsnooze.enabled)
        assertTrue(unsnooze.children.isEmpty())
    }
}
