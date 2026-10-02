package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.transport.wire.ProjectShellDto
import club.touchtech.s5code.kotlin.transport.wire.ShellSnapshotDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadShellDto
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ShellSnapshotCacheTest {
    @Test
    fun `synthetic empty reducer seed is not persisted over a real cache`() {
        assertFalse(ShellSnapshotDto().hasCacheableWorkspaceContent())
    }

    @Test
    fun `sequenced empty and populated snapshots are cacheable`() {
        assertTrue(ShellSnapshotDto(snapshotSequence = 3).hasCacheableWorkspaceContent())
        assertTrue(
            ShellSnapshotDto(projects = listOf(ProjectShellDto(id = "project-1")))
                .hasCacheableWorkspaceContent()
        )
        assertTrue(
            ShellSnapshotDto(threads = listOf(ThreadShellDto(id = "thread-1")))
                .hasCacheableWorkspaceContent()
        )
    }
}
