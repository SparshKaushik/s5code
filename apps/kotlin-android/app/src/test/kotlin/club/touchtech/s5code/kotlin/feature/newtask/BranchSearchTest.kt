package club.touchtech.s5code.kotlin.feature.newtask

import club.touchtech.s5code.kotlin.model.BranchRef
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * `filterNewTaskBranches`: the typed query is a would-be ref name, so spaces
 * sanitize to dashes before matching — and remote refs under `origin/` are in the
 * list, so "release" finds `origin/release-2` as well as the local branch.
 */
class BranchSearchTest {

    private fun branch(name: String, remote: Boolean = false) =
        BranchRef(name = name, current = false, remote = remote, ageLabel = "")

    @Test
    fun `a blank query returns everything`() {
        val branches = listOf(branch("main"), branch("feature/x"))
        assertEquals(branches, filterNewTaskBranches(branches, "  "))
    }

    @Test
    fun `spaces match dashed branch names`() {
        val branches = listOf(branch("main"), branch("fix-flaky-test"))
        assertEquals(listOf("fix-flaky-test"), filterNewTaskBranches(branches, "flaky test").map { it.name })
    }

    @Test
    fun `remote refs participate in search`() {
        val branches = listOf(branch("main"), branch("origin/release-2", remote = true))
        assertEquals(
            listOf("origin/release-2"),
            filterNewTaskBranches(branches, "release").map { it.name },
        )
    }

    @Test
    fun `matching is case insensitive`() {
        val branches = listOf(branch("Feature/Large"))
        assertEquals(1, filterNewTaskBranches(branches, "FEATURE").size)
    }
}
