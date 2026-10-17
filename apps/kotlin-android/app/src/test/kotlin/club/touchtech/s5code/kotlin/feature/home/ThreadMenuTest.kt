package club.touchtech.s5code.kotlin.feature.home

import club.touchtech.s5code.kotlin.model.ThreadStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ThreadMenuTest {

    private val thread = HomeFixture.threads.first()

    private fun options(
        thread: club.touchtech.s5code.kotlin.model.ThreadSummary = this.thread,
        settlement: Boolean = true,
        snooze: Boolean = true,
        pinning: Boolean = true,
        titleRegeneration: Boolean = false,
    ) =
        threadMenuOptions(
            thread = thread,
            settlementSupported = settlement,
            snoozeSupported = snooze,
            pinningSupported = pinning,
            titleRegenerationSupported = titleRegeneration,
        )

    @Test
    fun `title regeneration is omitted when the environment does not support it`() {
        assertTrue(options(titleRegeneration = false).none { it.id == "regenerate-title" })
    }

    @Test
    fun `supported title regeneration is actionable at rest`() {
        val option = options(titleRegeneration = true).single { it.id == "regenerate-title" }
        assertEquals("Regenerate title", option.label)
        assertTrue(option.enabled)
    }

    @Test
    fun `in-flight title regeneration is labelled and disabled`() {
        val option =
            options(
                    thread = thread.copy(titleRegenerating = true, status = ThreadStatus.Working),
                    titleRegeneration = true,
                )
                .single { it.id == "regenerate-title" }
        assertEquals("Regenerating…", option.label)
        assertFalse(option.enabled)
    }

    @Test
    fun `snooze offers submenu presets on an active thread`() {
        val workingThread = thread.copy(status = ThreadStatus.Working)
        val snooze = options(thread = workingThread).single { it.id == "snooze" }
        assertEquals("Snooze", snooze.label)
        assertTrue(snooze.enabled)
        assertTrue(snooze.children.isNotEmpty())
        assertTrue(snooze.children.any { it.label == "In 1 hour" })
        assertTrue(snooze.children.any { it.label == "Tomorrow" })
        assertTrue(snooze.children.any { it.label == "Next week" })
        assertTrue(snooze.children.all { it.id.startsWith("snooze:") })
    }

    @Test
    fun `snooze is absent when the server cannot snooze or the thread is blocked`() {
        assertTrue(options(snooze = false).none { it.id == "snooze" })
        val approvalThread = thread.copy(status = ThreadStatus.AwaitingApproval)
        assertTrue(options(thread = approvalThread).none { it.id == "snooze" })
    }

    @Test
    fun `snoozed thread offers wake without lifecycle items`() {
        val snoozedThread = thread.copy(status = ThreadStatus.Snoozed)
        val menu = options(thread = snoozedThread)
        assertTrue(menu.none { it.id == "snooze" || it.id == "settle" || it.id == "pin" })
        val wake = menu.single { it.id == "unsnooze" }
        assertEquals("Wake thread", wake.label)
        assertTrue(wake.enabled)
        assertTrue(wake.children.isEmpty())
    }

    @Test
    fun `a pre-settlement server gets archive instead of lifecycle items`() {
        val menu = options(settlement = false)
        assertTrue(menu.none { it.id == "settle" || it.id == "snooze" })
        assertTrue(menu.any { it.id == "archive" })
    }

    @Test
    fun `a settled row un-settles on a modern server`() {
        val settledThread = thread.copy(status = ThreadStatus.Settled)
        val menu = options(thread = settledThread)
        val settle = menu.single { it.id == "settle" }
        assertEquals("Un-settle", settle.label)
        assertTrue(menu.none { it.id == "archive" })
    }

    @Test
    fun `pin requires the pinning capability`() {
        assertTrue(options(pinning = true).any { it.id == "pin" })
        assertTrue(options(pinning = false).none { it.id == "pin" })
    }
}
