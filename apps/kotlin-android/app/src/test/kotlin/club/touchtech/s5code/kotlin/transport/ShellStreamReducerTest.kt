package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.transport.wire.ProjectShellDto
import club.touchtech.s5code.kotlin.transport.wire.RepositoryIdentityDto
import club.touchtech.s5code.kotlin.transport.wire.ShellSnapshotDto
import club.touchtech.s5code.kotlin.transport.wire.ShellStreamItemDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadShellDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * The two `kind: "snapshot"` flavors on `orchestration.subscribeShell`, matching
 * `shellReducer.ts`/`ShellStream.ts`. An enrichment refresh carries only the
 * projects whose repository identity resolved — its thread list is empty by
 * contract — and must merge in place; replacing the shell with it empties the
 * home list and poisons the persisted cache.
 */
class ShellStreamReducerTest {

    private val threadA = ThreadShellDto(id = "thread-a", projectId = "project-1", status = "idle")
    private val project1 =
        ProjectShellDto(id = "project-1", title = "App", workspaceRoot = "/work/app")
    private val project2 =
        ProjectShellDto(id = "project-2", title = "Lib", workspaceRoot = "/work/lib")
    private val shell =
        ShellSnapshotDto(
            snapshotSequence = 10,
            projects = listOf(project1, project2),
            threads = listOf(threadA),
        )

    @Test
    fun `enrichment snapshot patches identity without touching threads or sequence`() {
        val resolved = RepositoryIdentityDto(canonicalKey = "gh:org/app", owner = "org", name = "app")
        val item =
            ShellStreamItemDto(
                kind = "snapshot",
                snapshot =
                    ShellSnapshotDto(
                        snapshotSequence = 10,
                        projects = listOf(project1.copy(repositoryIdentity = resolved)),
                    ),
                resolvedRepositoryIdentityRoots = listOf("/work/app"),
            )

        val next = applyShellStreamItem(shell, item)

        assertEquals(listOf(threadA), next.threads)
        assertEquals(10, next.snapshotSequence)
        assertEquals(resolved, next.projects.first { it.id == "project-1" }.repositoryIdentity)
        assertNull(next.projects.first { it.id == "project-2" }.repositoryIdentity)
    }

    @Test
    fun `authoritative snapshot replaces structure but retains a resolved identity`() {
        val resolved = RepositoryIdentityDto(canonicalKey = "gh:org/app", owner = "org", name = "app")
        val resolvedShell =
            shell.copy(projects = listOf(project1.copy(repositoryIdentity = resolved), project2))
        val item =
            ShellStreamItemDto(
                kind = "snapshot",
                snapshot =
                    ShellSnapshotDto(
                        snapshotSequence = 42,
                        projects = listOf(project1, project2),
                        threads = emptyList(),
                    ),
            )

        val next = applyShellStreamItem(resolvedShell, item)

        assertEquals(emptyList<ThreadShellDto>(), next.threads)
        assertEquals(42, next.snapshotSequence)
        assertEquals(resolved, next.projects.first { it.id == "project-1" }.repositoryIdentity)
    }

    @Test
    fun `thread deltas move rows between active and archive halves`() {
        val moved =
            applyShellStreamItem(
                shell,
                ShellStreamItemDto(
                    kind = "thread.updated",
                    sequence = 11,
                    location = "archived",
                    thread = threadA.copy(archivedAt = "2026-10-05T00:00:00Z"),
                ),
            )
        assertEquals(emptyList<ThreadShellDto>(), moved.threads)
        assertEquals(11, moved.snapshotSequence)

        val removed =
            applyShellStreamItem(
                shell.copy(archivedThreads = listOf(threadA)),
                ShellStreamItemDto(kind = "thread.removed", sequence = 12, threadId = "thread-a"),
            )
        assertEquals(emptyList<ThreadShellDto>(), removed.threads)
        assertEquals(emptyList<ThreadShellDto>(), removed.archivedThreads)
    }

    @Test
    fun `wire decode carries the enrichment marker and delta location`() {
        val element =
            TransportJson.parseToJsonElement(
                """{"kind":"snapshot","snapshot":{"schemaVersion":2,"snapshotSequence":3,"threads":[],"archivedThreads":[],"projects":[]},"resolvedRepositoryIdentityRoots":["/work/app"]}"""
            )
        val frame = TransportJson.decodeFromJsonElement(ShellStreamItemDto.serializer(), element)
        assertEquals(listOf("/work/app"), frame.resolvedRepositoryIdentityRoots)

        val delta =
            TransportJson.decodeFromJsonElement(
                ShellStreamItemDto.serializer(),
                TransportJson.parseToJsonElement(
                    """{"kind":"thread.updated","sequence":9,"location":"archived","thread":{"id":"t"}}"""
                ),
            )
        assertEquals("archived", delta.location)
        assertEquals("t", delta.thread?.id)
    }
}
