package club.touchtech.s5code.kotlin.feature.newtask

import club.touchtech.s5code.kotlin.data.StoredPreferences
import club.touchtech.s5code.kotlin.data.ThreadProjectRef
import club.touchtech.s5code.kotlin.data.toRuntime
import club.touchtech.s5code.kotlin.data.toStored
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.ProjectId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** `newTaskDestination`: New task starts in the open thread's project, then the last one used. */
class NewTaskDestinationTest {

    private fun project(environmentId: String, id: String) =
        Project(
            id = ProjectId(id),
            environmentId = EnvironmentId(environmentId),
            title = id,
            workspaceRoot = "/$id",
            repository = null,
            branch = "main",
        )

    private val mac = project("mac", "t3code")
    private val server = project("server", "t3code")
    private val projects = listOf(mac, server)

    @Test
    fun `the open thread's project wins over the last one used`() {
        assertEquals(
            server,
            newTaskDestinationProject(projects, true, ThreadProjectRef("server", "t3code"), ThreadProjectRef("mac", "t3code")),
        )
        assertEquals(mac, newTaskDestinationProject(projects, true, null, ThreadProjectRef("mac", "t3code")))
    }

    @Test
    fun `a project that is gone, or the preference off, opens the picker`() {
        assertEquals(mac, newTaskDestinationProject(projects, true, ThreadProjectRef("mac", "removed"), ThreadProjectRef("mac", "t3code")))
        assertNull(newTaskDestinationProject(projects, true, null, ThreadProjectRef("laptop", "t3code")))
        assertNull(newTaskDestinationProject(projects, true, null, null))
        assertNull(newTaskDestinationProject(projects, false, ThreadProjectRef("mac", "t3code"), ThreadProjectRef("mac", "t3code")))
    }

    @Test
    fun `the preference is on by default and the last project survives a restart`() {
        assertEquals(true, StoredPreferences().toRuntime().useLastProjectForNewThreads)
        val runtime = StoredPreferences().toRuntime().copy(
            useLastProjectForNewThreads = false,
            lastThreadProject = ThreadProjectRef("mac", "t3code"),
        )
        assertEquals(runtime, runtime.toStored().toRuntime())
        assertNull(StoredPreferences(lastThreadProject = ThreadProjectRef("", "t3code")).toRuntime().lastThreadProject)
    }
}
