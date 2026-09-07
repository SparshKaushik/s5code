package club.touchtech.s5code.kotlin.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WorkspacePanesTest {
    @Test
    fun `default inspector follows the RN proportional and capped width`() {
        assertEquals(280f, WorkspacePaneSizing.defaultInspectorWidth(1_000f))
        assertEquals(320f, WorkspacePaneSizing.defaultInspectorWidth(2_000f))
        assertEquals(260f, WorkspacePaneSizing.defaultInspectorWidth(820f))
    }

    @Test
    fun `inspector resizing preserves a usable chat width`() {
        assertEquals(260f, WorkspacePaneSizing.constrainInspectorWidth(100f, 1_000f))
        assertEquals(440f, WorkspacePaneSizing.constrainInspectorWidth(600f, 1_000f))
        assertEquals(480f, WorkspacePaneSizing.constrainInspectorWidth(600f, 1_400f))
        assertEquals(260f, WorkspacePaneSizing.constrainInspectorWidth(Float.NaN, Float.NaN))
    }

    @Test
    fun `thread workspace tools are inspector routes`() {
        listOf(
                Routes.ThreadFiles,
                Routes.ThreadFile,
                Routes.ThreadFileMarkdown,
                Routes.ThreadFileImage,
                Routes.ThreadFileWeb,
                Routes.ThreadReview,
                Routes.ThreadReviewComment,
                Routes.ThreadTerminal,
                Routes.Git,
                Routes.GitCommit,
                Routes.GitBranches,
                Routes.SourceControl,
                Routes.PullRequests,
            )
            .forEach { route -> assertTrue(route, isWorkspaceInspectorRoute(route)) }
        assertFalse(isWorkspaceInspectorRoute(Routes.ThreadRewind))
        assertFalse(isWorkspaceInspectorRoute(Routes.Thread))
        assertFalse(isWorkspaceInspectorRoute(Routes.Settings))
    }

    @Test
    fun `query-bearing file and review suffixes remain inspector destinations`() {
        assertTrue(isWorkspaceInspectorSuffix("files/source?path=README.md"))
        assertTrue(isWorkspaceInspectorSuffix("review-comment?filePath=a.kt&startIndex=1&endIndex=2"))
        assertTrue(isWorkspaceInspectorSuffix("terminal"))
        assertFalse(isWorkspaceInspectorSuffix("rewind"))
    }
}
