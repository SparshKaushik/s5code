package club.touchtech.s5code.kotlin.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class FileLinkTargetsTest {
    @Test
    fun `only bare file names need a workspace lookup`() {
        assertTrue(needsWorkspaceBasenameLookup("AGENTS.md"))
        assertFalse(needsWorkspaceBasenameLookup("docs/AGENTS.md"))
        assertFalse(needsWorkspaceBasenameLookup("docs\\AGENTS.md"))
        assertFalse(needsWorkspaceBasenameLookup(".."))
        assertFalse(needsWorkspaceBasenameLookup("  "))
    }

    @Test
    fun `prefers an exact file name then a unique case-insensitive one`() {
        val entries =
            listOf(
                WorkspaceEntryCandidate("agents.md", "directory"),
                WorkspaceEntryCandidate("docs/agents.md", "file"),
                WorkspaceEntryCandidate("apps/web/AGENTS.md", "file"),
            )
        assertEquals("apps/web/AGENTS.md", pickWorkspaceBasenameMatch("AGENTS.md", entries))
        assertEquals("docs/agents.md", pickWorkspaceBasenameMatch("Agents.MD", entries.take(2)))
        // Two folded matches are ambiguous, and a directory never counts.
        assertNull(pickWorkspaceBasenameMatch("Agents.MD", entries))
        assertNull(pickWorkspaceBasenameMatch("agents.md", entries.take(1)))
    }
}
