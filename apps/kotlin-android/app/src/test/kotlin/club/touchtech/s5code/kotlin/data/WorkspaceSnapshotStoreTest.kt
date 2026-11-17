package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.transport.wire.MessageDto
import club.touchtech.s5code.kotlin.transport.wire.ModelSelectionDto
import club.touchtech.s5code.kotlin.transport.wire.ProjectShellDto
import club.touchtech.s5code.kotlin.transport.wire.ShellSnapshotDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadDetailPageDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadShellDto
import java.nio.file.Files
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class WorkspaceSnapshotStoreTest {
    @Test
    fun `shell and opened thread survive a new store instance`() = runTest {
        val root = Files.createTempDirectory("workspace-snapshots").toFile()
        val environmentId = EnvironmentId("env/with unsafe path")
        try {
            val shell =
                ShellSnapshotDto(
                    snapshotSequence = 42,
                    projects = listOf(ProjectShellDto(id = "project-1", title = "S5 Code")),
                    threads =
                        listOf(
                            ThreadShellDto(
                                id = "thread-1",
                                projectId = "project-1",
                                title = "Cached conversation",
                                modelSelection = ModelSelectionDto("codex", "gpt-5.4"),
                            )
                        ),
                )
            val thread =
                ThreadDto(
                    id = "thread-1",
                    projectId = "project-1",
                    title = "Cached conversation",
                    modelSelection = ModelSelectionDto("codex", "gpt-5.4"),
                    messages = listOf(MessageDto(id = "message-1", role = "user", text = "Hello")),
                )

            val page = ThreadDetailPageDto(beforeCursor = "older-1", hasMore = true)
            WorkspaceSnapshotStore.forRoot(root).apply {
                saveShell(environmentId, shell)
                saveThread(environmentId, thread, page)
            }

            val restored = WorkspaceSnapshotStore.forRoot(root)
            assertEquals(shell, restored.loadShell(environmentId))
            assertEquals(thread, restored.loadThread(environmentId, "thread-1")?.thread)
            assertEquals(page, restored.loadThread(environmentId, "thread-1")?.page)
        } finally {
            root.deleteRecursively()
        }
    }

    @Test
    fun `removing a thread does not remove the cached shell`() = runTest {
        val root = Files.createTempDirectory("workspace-snapshots").toFile()
        val environmentId = EnvironmentId("env-1")
        try {
            val shell = ShellSnapshotDto(snapshotSequence = 7)
            val thread = ThreadDto(id = "thread-1")
            val store = WorkspaceSnapshotStore.forRoot(root)
            store.saveShell(environmentId, shell)
            store.saveThread(environmentId, thread)

            assertEquals(thread, store.loadThread(environmentId, "thread-1")?.thread)
            store.removeThread(environmentId, "thread-1")

            assertNull(store.loadThread(environmentId, "thread-1"))
            assertEquals(shell, store.loadShell(environmentId))
        } finally {
            root.deleteRecursively()
        }
    }

    @Test
    fun `corrupt snapshots fail open and are discarded`() = runTest {
        val root = Files.createTempDirectory("workspace-snapshots").toFile()
        val environmentId = EnvironmentId("env-1")
        try {
            val store = WorkspaceSnapshotStore.forRoot(root)
            store.saveShell(environmentId, ShellSnapshotDto(snapshotSequence = 1))
            root.walkTopDown().first { it.name == "shell.json" }.writeText("not json")

            assertNull(store.loadShell(environmentId))
            assertNull(store.loadShell(environmentId))
        } finally {
            root.deleteRecursively()
        }
    }
}
