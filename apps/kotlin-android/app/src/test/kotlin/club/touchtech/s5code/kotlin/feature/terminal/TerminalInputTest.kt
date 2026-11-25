package club.touchtech.s5code.kotlin.feature.terminal

import club.touchtech.s5code.kotlin.data.DEFAULT_TERMINAL_ID
import club.touchtech.s5code.kotlin.model.TerminalStatus
import club.touchtech.s5code.kotlin.model.TerminalSummary
import org.junit.Test
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue

/** Terminal input encoding and session-menu rules, ported from the RN client. */
class TerminalInputTest {
    @Test
    fun `ctrl modifier maps letters and punctuation to control bytes`() {
        assertEquals("\u0003", applyCtrlModifier("c"))
        assertEquals("\u0003", applyCtrlModifier("C"))
        assertEquals("\u001b", applyCtrlModifier("["))
        assertEquals("\u007f", applyCtrlModifier("?"))
        // Unmapped input passes through unchanged.
        assertEquals("5", applyCtrlModifier("5"))
    }

    @Test
    fun `the host paste chord reads the clipboard instead of writing a byte`() {
        assertEquals(
            ModifiedTerminalInput.Paste,
            resolveModifiedTerminalInput("v", PendingModifier.Ctrl, HostPlatform.Linux),
        )
        assertEquals(
            ModifiedTerminalInput.Paste,
            resolveModifiedTerminalInput("v", PendingModifier.Meta, HostPlatform.Mac),
        )
        // ctrl+v on a mac is still a control byte; cmd+v elsewhere is too.
        assertEquals(
            ModifiedTerminalInput.Write("\u0016"),
            resolveModifiedTerminalInput("v", PendingModifier.Ctrl, HostPlatform.Mac),
        )
        assertEquals(
            ModifiedTerminalInput.Write("\u001bv"),
            resolveModifiedTerminalInput("v", PendingModifier.Meta, HostPlatform.Linux),
        )
    }

    @Test
    fun `paste encoding defuses control bytes and normalizes newlines`() {
        assertEquals("ls -la\rpwd", encodeTerminalPaste("ls -la\npwd"))
        assertEquals(" x", encodeTerminalPaste("\u001bx"))
        // An embedded bracketed-paste end marker cannot forge a paste end.
        assertEquals(" [0m", encodeTerminalPaste("\u001b[0m"))
    }

    @Test
    fun `host platform prefers the descriptor and falls back to the label`() {
        assertEquals(HostPlatform.Mac, hostPlatformOf("darwin", "prod"))
        assertEquals(HostPlatform.Windows, hostPlatformOf("windows", "prod"))
        assertEquals(HostPlatform.Mac, hostPlatformOf(null, "My MacBook"))
        assertEquals(HostPlatform.Unknown, hostPlatformOf(null, "prod"))
    }

    @Test
    fun `terminal ids allocate the lowest free term-N`() {
        assertEquals("term-1", nextTerminalId(emptyList()))
        assertEquals("term-2", nextTerminalId(listOf("term-1")))
        assertEquals("term-2", nextTerminalId(listOf("term-1", "term-3")))
        // The mounted route counts as occupied so a fresh list still advances.
        assertEquals("term-3", nextOpenTerminalId(listOf("term-1"), "term-2"))
        assertEquals("term-2", nextOpenTerminalId(listOf("term-1"), DEFAULT_TERMINAL_ID))
    }

    private fun summary(
        terminalId: String,
        status: TerminalStatus = TerminalStatus.Running,
        label: String = "",
    ) = TerminalSummary(
        terminalId = terminalId,
        threadId = "t1",
        cwd = "/w",
        worktreePath = null,
        status = status,
        hasRunningSubprocess = false,
        label = label,
        updatedAt = null,
    )

    @Test
    fun `the menu lists live sessions plus the attached one, sorted numerically`() {
        val sessions =
            buildTerminalMenuSessions(
                knownSessions =
                    listOf(
                        summary("term-10"),
                        summary("term-2"),
                        summary("term-9", status = TerminalStatus.Exited),
                    ),
                threadId = "t1",
                workspaceRoot = "/w",
                current = null,
            )
        // term-9 is dead and not attached, so only the live ones appear, in
        // numeric order rather than lexicographic.
        assertEquals(listOf("term-2", "term-10"), sessions.map { it.terminalId })
    }

    @Test
    fun `a bare route follows the running session, preferring the default`() {
        val sessions =
            buildTerminalMenuSessions(
                knownSessions =
                    listOf(summary("term-2"), summary("term-1"), summary("term-9", TerminalStatus.Exited)),
                threadId = "t1",
                workspaceRoot = null,
                current = null,
            )
        assertEquals("term-1", pickRunningTerminalSession(sessions)?.terminalId)
        assertNull(pickRunningTerminalSession(emptyList()))
    }

    @Test
    fun `exit falls back to the nearest live session or nothing`() {
        val sessions =
            buildTerminalMenuSessions(
                knownSessions =
                    listOf(summary("term-1"), summary("term-3"), summary("term-4")),
                threadId = "t1",
                workspaceRoot = null,
                current = null,
            )
        assertEquals("term-1", previousLiveTerminalId(sessions, "term-3"))
        // Nothing below the first id: the next live one wins.
        assertEquals("term-3", previousLiveTerminalId(sessions, "term-1"))
        // The last live session leaves nothing to fall back to.
        assertNull(
            previousLiveTerminalId(
                buildTerminalMenuSessions(
                    knownSessions = listOf(summary("term-1")),
                    threadId = "t1",
                    workspaceRoot = null,
                    current = null,
                ),
                "term-1",
            )
        )
    }

    @Test
    fun `server labels win over derived ones in the menu`() {
        val sessions =
            buildTerminalMenuSessions(
                knownSessions = listOf(summary("term-1", label = " Build ")),
                threadId = "t1",
                workspaceRoot = null,
                current = null,
            )
        assertEquals("Build", sessions[0].displayLabel)
    }
}
