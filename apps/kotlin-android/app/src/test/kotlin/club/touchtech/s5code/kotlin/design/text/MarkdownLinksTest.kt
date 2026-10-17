package club.touchtech.s5code.kotlin.design.text

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class MarkdownLinksTest {
    @Test
    fun `recognizes relative absolute and file url workspace links`() {
        val relative = resolveMarkdownLinkPresentation("src/main/App.kt:42:7") as MarkdownLinkPresentation.File
        assertEquals("src/main/App.kt", relative.path)
        assertEquals(42, relative.line)
        assertEquals(7, relative.column)
        assertEquals("App.kt:42:7", relative.label)
        assertEquals(MarkdownFileIcon.Default, relative.icon)

        val absolute = resolveMarkdownLinkPresentation("/home/dev/repo/README.md#L9") as MarkdownLinkPresentation.File
        assertEquals("/home/dev/repo/README.md", absolute.path)
        assertEquals(9, absolute.line)
        assertEquals(MarkdownFileIcon.Readme, absolute.icon)

        val fileUrl = resolveMarkdownLinkPresentation("file:///home/dev/repo/package.json") as MarkdownLinkPresentation.File
        assertEquals("/home/dev/repo/package.json", fileUrl.path)
        assertEquals(MarkdownFileIcon.Package, fileUrl.icon)
    }

    @Test
    fun `classifies external and unsafe links separately`() {
        val external = resolveMarkdownLinkPresentation("https://s5.dev/docs") as MarkdownLinkPresentation.External
        assertEquals("s5.dev", external.host)
        assertEquals("mailto:hello@s5.dev", (resolveMarkdownLinkPresentation("mailto:hello@s5.dev") as MarkdownLinkPresentation.Link).href)
        assertNull((resolveMarkdownLinkPresentation("javascript:alert(1)") as MarkdownLinkPresentation.Link).href)
    }

    @Test
    fun `uses Pierre mappings for exact names configs and compound extensions`() {
        assertEquals(MarkdownFileIcon.Agents, resolveMarkdownFileIcon("AGENTS.md"))
        assertEquals(MarkdownFileIcon.Docker, resolveMarkdownFileIcon("infra/Dockerfile"))
        assertEquals(MarkdownFileIcon.Tsconfig, resolveMarkdownFileIcon("tsconfig.app.json"))
        assertEquals(MarkdownFileIcon.React, resolveMarkdownFileIcon("View.tsx"))
        assertEquals(MarkdownFileIcon.Markdown, resolveMarkdownFileIcon("guide.mdx.tsx"))
        assertEquals(MarkdownFileIcon.Tailwind, resolveMarkdownFileIcon("tailwind.config.ts"))
        assertEquals(MarkdownFileIcon.Table, resolveMarkdownFileIcon("report.csv"))
    }

    @Test
    fun `normalizes relative targets and rejects paths outside workspace`() {
        assertEquals("src/main.kt", resolveWorkspaceRelativeFilePath("/repo", "./src/../src/main.kt"))
        assertEquals("src/main.kt", resolveWorkspaceRelativeFilePath("/repo", "/repo/src/main.kt"))
        assertEquals("src/main.kt", resolveWorkspaceRelativeFilePath("C:\\repo", "c:\\repo\\src\\main.kt"))
        assertNull(resolveWorkspaceRelativeFilePath("/repo", "/other/main.kt"))
        assertNull(resolveWorkspaceRelativeFilePath("/repo", "../other/main.kt"))
        assertNull(resolveWorkspaceRelativeFilePath(null, "/repo/main.kt"))
        assertTrue(resolveWorkspaceRelativeFilePath("/repo", "src/main.kt")!!.isNotBlank())
    }
}
