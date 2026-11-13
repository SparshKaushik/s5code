package club.touchtech.s5code.kotlin.feature.files

import club.touchtech.s5code.kotlin.model.FileNode
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class FileTreeTest {
    private val tree =
        FileNode(
            path = "",
            name = "workspace",
            isDirectory = true,
            children =
                listOf(
                    FileNode(
                        path = "src",
                        name = "src",
                        isDirectory = true,
                        children =
                            listOf(
                                FileNode("src/WorkspaceFileImagePreview.kt", "WorkspaceFileImagePreview.kt", false),
                                FileNode("src/api-client.ts", "api-client.ts", false),
                            ),
                    ),
                    FileNode("README.md", "README.md", false),
                ),
        )

    @Test
    fun `collapsed tree only shows root entries`() {
        assertEquals(listOf("src", "README.md"), flattenFileTree(tree, emptySet()).map { it.node.path })
    }

    @Test
    fun `search exposes matching descendants and their ancestors`() {
        val rows = flattenFileTree(tree, emptySet(), "image preview")

        assertEquals(
            listOf("src", "src/WorkspaceFileImagePreview.kt"),
            rows.map { it.node.path },
        )
        assertTrue(rows.first().expanded)
    }

    @Test
    fun `search matches fuzzy words and path tokens`() {
        assertEquals(
            listOf("src", "src/WorkspaceFileImagePreview.kt"),
            flattenFileTree(tree, emptySet(), "wspc preview").map { it.node.path },
        )
        assertEquals(
            listOf("src", "src/api-client.ts"),
            flattenFileTree(tree, emptySet(), "src api").map { it.node.path },
        )
    }

    @Test
    fun `non matching search is empty`() {
        assertTrue(flattenFileTree(tree, setOf("src"), "terminal").isEmpty())
    }

    @Test
    fun `file rows preserve file kind for Pierre icon rendering`() {
        val row = flattenFileTree(tree, setOf("src")).first { it.node.path.endsWith(".ts") }
        assertFalse(row.node.isDirectory)
    }
}
