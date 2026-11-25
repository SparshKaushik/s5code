package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.BranchRef
import club.touchtech.s5code.kotlin.transport.wire.VcsListRefsResultDto
import club.touchtech.s5code.kotlin.transport.wire.VcsRefDto
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NewTaskBranchCheckoutTest {

    private val feature = BranchRef(name = "feature/a", current = false, remote = false, ageLabel = "")

    private fun pages(vararg pages: VcsListRefsResultDto): suspend (String, Int?) -> VcsListRefsResultDto {
        return { _, cursor -> pages[cursor ?: 0] }
    }

    private val refuseSwitch: suspend (String) -> String? = { error("An occupied branch must not trigger checkout") }

    @Test
    fun `worktree mode and the current branch need no checkout`() = runTest {
        val noRefs: suspend (String, Int?) -> VcsListRefsResultDto = { _, _ -> error("no listing") }
        assertEquals(feature, checkoutNewTaskBranch(feature, newWorktree = true, noRefs, refuseSwitch))
        val current = feature.copy(current = true, worktreePath = "/repo")
        assertNull(checkoutNewTaskBranch(current, newWorktree = false, noRefs, refuseSwitch).worktreePath)
    }

    @Test
    fun `a branch another worktree holds is reused there, found beyond the first page`() = runTest {
        val queries = mutableListOf<Pair<String, Int?>>()
        val listing = pages(
            VcsListRefsResultDto(refs = listOf(VcsRefDto(name = "a-feature/a", worktreePath = "/decoy")), nextCursor = 1, isAtRepositoryRoot = true),
            VcsListRefsResultDto(refs = listOf(VcsRefDto(name = "feature/a", worktreePath = "/wt")), isAtRepositoryRoot = true),
        )
        val result = checkoutNewTaskBranch(feature, newWorktree = false, { query, cursor ->
            queries += query to cursor
            listing(query, cursor)
        }, refuseSwitch)
        assertEquals("/wt", result.worktreePath)
        assertEquals("feature/a", result.name)
        assertEquals(listOf("feature/a" to null, "feature/a" to 1), queries)
    }

    @Test
    fun `reuse is refused when the project is not provably the repository root`() = runTest {
        val held = VcsRefDto(name = "feature/a", worktreePath = "/wt")
        val unknown = runCatching {
            checkoutNewTaskBranch(feature, false, pages(VcsListRefsResultDto(refs = listOf(held))), refuseSwitch)
        }.exceptionOrNull()
        assertTrue(unknown?.message.orEmpty().startsWith("The server did not report"))
        val nested = runCatching {
            checkoutNewTaskBranch(
                feature, false,
                pages(VcsListRefsResultDto(refs = listOf(held), isAtRepositoryRoot = false)), refuseSwitch,
            )
        }.exceptionOrNull()
        assertTrue(nested?.message.orEmpty().startsWith("This project is a repository subdirectory."))
    }

    @Test
    fun `a free branch is switched to in the project checkout`() = runTest {
        val switched = mutableListOf<String>()
        val result = checkoutNewTaskBranch(
            feature.copy(worktreePath = "/stale"), false,
            pages(VcsListRefsResultDto(refs = listOf(VcsRefDto(name = "feature/a")), isAtRepositoryRoot = true)),
        ) { refName -> switched += refName; refName }
        assertEquals(listOf("feature/a"), switched)
        assertTrue(result.current)
        assertNull(result.worktreePath)
    }

    @Test
    fun `a remote pick reuses only a worktree tracking that remote branch`() = runTest {
        val remote = BranchRef(name = "upstream/feature/a", current = false, remote = true, ageLabel = "")
        val tracking = VcsRefDto(name = "feature/a", worktreePath = "/wt", upstreamRef = "refs/remotes/upstream/feature/a")
        val reused = checkoutNewTaskBranch(
            remote, false, pages(VcsListRefsResultDto(refs = listOf(tracking), isAtRepositoryRoot = true)), refuseSwitch,
        )
        assertEquals("/wt", reused.worktreePath)
        assertEquals(false, reused.remote)

        val other = tracking.copy(upstreamRef = "refs/remotes/origin/feature/a")
        val switched = checkoutNewTaskBranch(
            remote, false, pages(VcsListRefsResultDto(refs = listOf(other), isAtRepositoryRoot = true)),
        ) { "feature/a" }
        assertNull(switched.worktreePath)
        assertEquals("feature/a", switched.name)
    }

    @Test
    fun `the draft never records the project checkout as an existing worktree`() {
        assertNull(newTaskBranchWorktreePath(false, "/repo", "/repo"))
        assertNull(newTaskBranchWorktreePath(true, "/repo", "/wt"))
        assertEquals("/wt", newTaskBranchWorktreePath(false, "/repo", "/wt"))
    }
}
