package club.touchtech.s5code.kotlin.app

import club.touchtech.s5code.kotlin.model.ComposerContextRecord
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.WorkspaceMode
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NewTaskDraftTest {
    private val prUrl = "https://github.com/o/r/pull/7"
    private val prDraft =
        NewTaskDraft(
            environmentId = EnvironmentId("env"),
            projectKey = "project",
            prompt = "Review this",
            branch = "feature",
            workspaceMode = WorkspaceMode.NewWorktree,
            pullRequestReference = prUrl,
            contextRecords =
                listOf(
                    ComposerContextRecord(
                        kind = "review-comment",
                        contextId = "o/r#7",
                        label = "#7",
                        sectionId = "",
                        sectionTitle = "",
                        filePath = "",
                        rangeLabel = "",
                        text = "",
                        pullRequestNumber = 7,
                        pullRequestTitle = "Fix",
                        pullRequestUrl = "https://github.com/o/r/pull/7",
                        pullRequestHeadBranch = "feature",
                        pullRequestBaseBranch = "main",
                        pullRequestState = "open",
                        pullRequestIsDraft = false,
                    )
                ),
        )

    @Test
    fun `discarding drops the pull request checkout with the content`() {
        val discarded = prDraft.discarded()
        assertNull(discarded.pullRequestReference)
        assertEquals("", discarded.prompt)
        assertTrue(discarded.contextRecords.isEmpty())
        assertFalse(discarded.hasContent)
        // The picked workspace stays for the next task.
        assertEquals("project", discarded.projectKey)
        assertEquals("feature", discarded.branch)
    }

    @Test
    fun `context records or attachments alone would be replaced`() {
        val contextOnly = prDraft.copy(prompt = "", pullRequestReference = null)
        assertTrue(newTaskDraftWouldBeReplaced(contextOnly, "env", prUrl))
        assertFalse(newTaskDraftWouldBeReplaced(NewTaskDraft(), "env", prUrl))
    }

    @Test
    fun `the same pull request continues only on the same environment`() {
        assertFalse(newTaskDraftWouldBeReplaced(prDraft, "env", prUrl))
        assertTrue(newTaskDraftWouldBeReplaced(prDraft, "other", prUrl))
        assertTrue(newTaskDraftWouldBeReplaced(prDraft, "env", "https://github.com/o/r/pull/8"))
    }
}
