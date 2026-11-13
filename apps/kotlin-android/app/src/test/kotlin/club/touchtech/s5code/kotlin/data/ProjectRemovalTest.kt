package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.ProjectId
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadStatus
import club.touchtech.s5code.kotlin.model.ThreadSummary
import org.junit.Assert.assertEquals
import org.junit.Test

class ProjectRemovalTest {

    private fun project(env: String, id: String, root: String) =
        Project(ProjectId(id), EnvironmentId(env), "t3code", root, repository = null, branch = "main")

    private fun thread(env: String, projectId: String, id: String) =
        ThreadSummary(
            id = ThreadId(id),
            environmentId = EnvironmentId(env),
            projectId = ProjectId(projectId),
            title = id,
            status = ThreadStatus.Waiting,
            provider = ProviderInstance.Default,
            model = "m",
            branch = null,
            updatedLabel = "",
        )

    private val laptop = project("laptop", "p1", "/work/t3code")
    private val server = project("server", "p1", "/srv/t3code")
    private val labels = mapOf("laptop" to "Laptop", "server" to "Server")

    @Test
    fun `one checkout of a group is a checkout removal counting only its own threads`() {
        val plan =
            planProjectRemoval(
                members = listOf(server),
                groupMemberCount = 2,
                displayName = "t3code",
                // Same project id on another environment is a different checkout.
                threads = listOf(thread("server", "p1", "a"), thread("laptop", "p1", "b")),
                environmentLabel = { labels[it.value] },
            )
        assertEquals(ProjectRemovalTarget.Checkout, plan.target)
        assertEquals(
            listOf(
                "Remove checkout \"t3code\" and delete its 1 thread?",
                "Path: /srv/t3code",
                "Environment: Server",
                "This permanently clears conversation history for those threads and any archived threads.",
                "Other entries in this grouped project are unaffected.",
                "This action cannot be undone.",
            ).joinToString("\n"),
            plan.confirmationMessage,
        )
    }

    @Test
    fun `the whole group is a project removal that leaves files on disk`() {
        val plan =
            planProjectRemoval(
                members = listOf(laptop, server),
                groupMemberCount = 2,
                displayName = "T3 Code",
                threads = emptyList(),
                environmentLabel = { labels[it.value] },
            )
        assertEquals(ProjectRemovalTarget.Project, plan.target)
        assertEquals(
            listOf(
                "Remove project \"T3 Code\"?",
                "This removes 2 grouped project entries.",
                "This permanently clears any archived conversation history.",
                "This removes only the project entries, not the files on disk.",
                "This action cannot be undone.",
            ).joinToString("\n"),
            plan.confirmationMessage,
        )
    }
}
