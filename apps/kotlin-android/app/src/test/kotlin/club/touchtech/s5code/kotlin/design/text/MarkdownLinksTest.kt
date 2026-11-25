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
        assertEquals("App.kt · L42:C7", relative.label)
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

    @Test
    fun `expands home links against the workspace home`() {
        assertEquals("/home/maria/.agents/AGENTS.md", resolveFileLinkPath("/home/maria/project", "~/.agents/AGENTS.md"))
        assertEquals("/Users/maria/notes.md", resolveFileLinkPath("/Users/maria/project", "~/notes.md"))
        assertEquals(
            "C:\\Users\\maria\\.agents\\AGENTS.md",
            resolveFileLinkPath("C:\\Users\\maria\\project", "~\\.agents\\AGENTS.md"),
        )
        // An unknown home never becomes a workspace path.
        assertNull(resolveFileLinkPath("/repo", "~/notes.md"))
        assertNull(resolveFileLinkPath(null, "~/notes.md"))
        assertNull(resolveFileLinkPath("/repo", "../notes.md"))
    }

    @Test
    fun `home links inside the workspace resolve to workspace paths`() {
        assertEquals("src/main.ts", resolveWorkspaceRelativeFilePath("/home/maria/project", "~/project/src/main.ts"))
        assertNull(resolveWorkspaceRelativeFilePath("/home/maria/project", "~/.agents/AGENTS.md"))
    }

    @Test
    fun `relative directory links are workspace links`() {
        assertEquals("src/", (resolveMarkdownLinkPresentation("src/") as MarkdownLinkPresentation.File).path)
        assertEquals(
            "docs/My Folder/",
            (resolveMarkdownLinkPresentation("docs/My Folder/") as MarkdownLinkPresentation.File).path,
        )
    }

    @Test
    fun `links a network path only inside the project's share`() {
        val share = resolveMarkdownLinkPresentation("file://server/share/workspace-image.svg") as MarkdownLinkPresentation.File
        assertEquals("\\\\server\\share\\workspace-image.svg", share.path)
        assertEquals("workspace-image.svg", resolveWorkspaceRelativeFilePath("\\\\server\\share", share.path))
        // Opening these sent the Windows user's NTLM credentials to attacker.test.
        for (href in listOf("file://attacker.test/s/x.ts", "\\\\attacker.test\\s\\x.ts", "/\\attacker.test\\s\\x.ts", "//attacker.test/s/x.ts")) {
            val path = (resolveMarkdownLinkPresentation(href) as MarkdownLinkPresentation.File).path
            assertNull(href, resolveWorkspaceRelativeFilePath("\\\\server\\share", path))
            assertNull(href, resolveWorkspaceRelativeFilePath("/repo", path))
        }
        assertNull(resolveWorkspaceRelativeFilePath("\\\\server\\share", "\\\\server\\other\\x.ts"))
        assertEquals("/repo/a.ts", (resolveMarkdownLinkPresentation("file://localhost/repo/a.ts") as MarkdownLinkPresentation.File).path)
        assertTrue(isNetworkPath("/\\h\\x") && isNetworkPath("//h/x") && !isNetworkPath("/h/x"))
    }

    @Test
    fun `relative links with non-ASCII names open as files`() {
        fun path(href: String) = (resolveMarkdownLinkPresentation(href) as? MarkdownLinkPresentation.File)?.path
        assertEquals("pastexam/線性代數/exam_易志偉.pdf", path("pastexam/線性代數/exam_易志偉.pdf"))
        assertEquals("Résumé final.pdf", path("Résumé%20final.pdf"))
        assertEquals("資料.備份", path("資料.備份"))
        assertEquals("docs/My Folder/", path("docs/My%20Folder/"))
    }
}
