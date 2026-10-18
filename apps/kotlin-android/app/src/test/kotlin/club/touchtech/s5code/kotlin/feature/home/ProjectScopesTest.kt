package club.touchtech.s5code.kotlin.feature.home

import club.touchtech.s5code.kotlin.app.HomeUiState
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.HomeListItem
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.ProjectGrouping
import club.touchtech.s5code.kotlin.model.ProjectId
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadSort
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The home project filter: one "No project" across machines, and saved keys that outlive config changes. */
class ProjectScopesTest {

    private val laptop = HomeFixture.laptop
    private val devbox = HomeFixture.devbox

    private fun scratch(id: String, environmentId: EnvironmentId, root: String, isScratch: Boolean = true) =
        Project(
            id = ProjectId(id),
            environmentId = environmentId,
            title = "No project",
            workspaceRoot = root,
            repository = null,
            branch = "main",
            isScratch = isScratch,
        )

    private val laptopScratch = scratch("scratch-laptop", laptop, "/home/me/.s5/scratch")
    private val devboxScratch = scratch("scratch-devbox", devbox, "/root/.s5/scratch")
    private val projects = HomeFixture.projects + laptopScratch + devboxScratch

    @Test
    fun `every environment's scratch project is one home scope`() {
        val scopes = homeProjectScopes(projects, environmentId = null, ProjectGrouping.Separate)
        val noProject = scopes.single { it.key == SCRATCH_PROJECT_SCOPE_KEY }
        assertEquals(listOf("scratch-laptop", "scratch-devbox"), noProject.projects.map { it.id.value })
    }

    @Test
    fun `an environment filter keeps only that machine's scratch project`() {
        val scopes = homeProjectScopes(projects, environmentId = devbox.value, ProjectGrouping.Separate)
        val noProject = scopes.single { it.key == SCRATCH_PROJECT_SCOPE_KEY }
        assertEquals(listOf("scratch-devbox"), noProject.projects.map { it.id.value })
    }

    @Test
    fun `the new-task picker does not merge scratch projects`() {
        val keys = buildProjectScopes(projects, ProjectGrouping.Separate).map { it.key }
        assertEquals(false, SCRATCH_PROJECT_SCOPE_KEY in keys)
    }

    @Test
    fun `a key saved before the project was known as scratch moves to no project`() {
        val physicalKey = "${laptop.value}:/home/me/.s5/scratch"
        assertEquals(
            SCRATCH_PROJECT_SCOPE_KEY,
            resolveScratchProjectScopeKey(physicalKey, projects, ProjectGrouping.Separate),
        )
    }

    @Test
    fun `other keys resolve unchanged`() {
        val relayKey = "${laptop.value}:~/code/s5-relay"
        assertEquals(relayKey, resolveScratchProjectScopeKey(relayKey, projects, ProjectGrouping.Separate))
        assertEquals("gone", resolveScratchProjectScopeKey("gone", projects, ProjectGrouping.Separate))
        assertNull(resolveScratchProjectScopeKey(null, projects, ProjectGrouping.Separate))
    }

    @Test
    fun `filtering on no project lists scratch threads from every machine`() {
        val template = HomeFixture.threads.first()
        val threads =
            HomeFixture.threads +
                template.copy(id = ThreadId("thr-scratch-laptop"), environmentId = laptop, projectId = laptopScratch.id) +
                template.copy(id = ThreadId("thr-scratch-devbox"), environmentId = devbox, projectId = devboxScratch.id)
        val items =
            homeListItems(
                threads,
                projects,
                HomeFixture.environments,
                HomeUiState(projectKey = SCRATCH_PROJECT_SCOPE_KEY),
                ProjectGrouping.Separate,
                ThreadSort.Recent,
            )
        assertEquals(
            setOf("thr-scratch-laptop", "thr-scratch-devbox"),
            items.filterIsInstance<HomeListItem.Thread>().mapTo(mutableSetOf()) { it.thread.id.value },
        )
    }

    @Test
    fun `a saved environment that was removed stops filtering`() {
        val saved = HomeUiState(environmentId = EnvironmentId("env-forgotten"), projectKey = "k")
        assertEquals(
            HomeUiState(projectKey = "k"),
            saved.withAvailableEnvironment(HomeFixture.environments),
        )
        val kept = HomeUiState(environmentId = devbox)
        assertEquals(kept, kept.withAvailableEnvironment(HomeFixture.environments))
    }
}
