package club.touchtech.s5code.kotlin.feature.newtask

import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.ProjectId
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AwaitCreatedProjectTest {
    private val env = EnvironmentId("env-1")

    private fun project(id: String, environmentId: EnvironmentId = env) =
        Project(
            id = ProjectId(id),
            environmentId = environmentId,
            title = id,
            workspaceRoot = "/work/$id",
            repository = null,
            branch = "",
        )

    @Test
    fun `waits for the created project to reach the store`() = runTest {
        val projects = MutableStateFlow(listOf(project("existing")))
        val arrival = async { awaitCreatedProject(projects, env, "new") }
        runCurrent()
        assertFalse(arrival.isCompleted)
        projects.value = projects.value + project("new")
        assertTrue(arrival.await())
    }

    @Test
    fun `a same-id project in another environment does not count`() = runTest {
        val projects = MutableStateFlow(listOf(project("new", EnvironmentId("env-2"))))
        assertFalse(awaitCreatedProject(projects, env, "new"))
    }

    @Test
    fun `gives up when the project never arrives`() = runTest {
        val projects = MutableStateFlow(emptyList<Project>())
        assertFalse(awaitCreatedProject(projects, env, "new"))
    }
}
