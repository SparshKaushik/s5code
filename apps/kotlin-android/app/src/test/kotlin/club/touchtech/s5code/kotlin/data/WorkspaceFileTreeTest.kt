package club.touchtech.s5code.kotlin.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WorkspaceFileTreeTest {
    @Test
    fun `flat recursive entries become a hierarchical sorted tree`() {
        val root =
            buildFileTree(
                rootPath = "",
                rootName = "repo",
                entries =
                    listOf(
                        "src/ui/App.kt" to false,
                        "README.md" to false,
                        "src" to true,
                        "src/ui" to true,
                        "assets/logo.png" to false,
                    ),
            )

        assertEquals(listOf("assets", "src", "README.md"), root.children.map { it.path })
        val src = root.children.first { it.path == "src" }
        assertTrue(src.isDirectory)
        assertEquals("src/ui/App.kt", src.children.single().children.single().path)
        assertFalse(src.children.single().children.single().isDirectory)
    }

    @Test
    fun `truncation is retained on the reconstructed root`() {
        val root =
            buildFileTree(
                rootPath = "",
                rootName = "repo",
                entries = listOf("README.md" to false),
                truncated = true,
            )

        assertTrue(root.truncated)
        assertFalse(root.children.single().truncated)
    }

    @Test
    fun `missing directory records are inferred from file paths`() {
        val root =
            buildFileTree(
                rootPath = "",
                rootName = "repo",
                entries = listOf("packages/client/src/index.ts" to false),
            )

        assertEquals("packages", root.children.single().name)
        assertTrue(root.children.single().isDirectory)
        assertEquals("packages/client/src/index.ts", root.children.single().children.single().children.single().children.single().path)
    }
}
