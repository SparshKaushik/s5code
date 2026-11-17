package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.DiffLineKind
import org.junit.Assert.assertEquals
import org.junit.Test

class UnifiedDiffTest {
    @Test
    fun `the diff text's final newline is not an empty context line`() {
        val patch =
            """
            diff --git a/a.ts b/a.ts
            --- a/a.ts
            +++ b/a.ts
            @@ -1,2 +1,2 @@
             keep
            -old
            +new

            """.trimIndent()
        val lines = parseUnifiedDiff(patch).single().hunks.single().lines
        assertEquals(listOf(DiffLineKind.Context, DiffLineKind.Removed, DiffLineKind.Added), lines.map { it.kind })
    }

    @Test
    fun `a blank context line inside the hunk is kept`() {
        val patch = "diff --git a/a.ts b/a.ts\n@@ -1,3 +1,3 @@\n one\n\n-two\n+three\n"
        val lines = parseUnifiedDiff(patch).single().hunks.single().lines
        assertEquals(listOf(DiffLineKind.Context, DiffLineKind.Context, DiffLineKind.Removed, DiffLineKind.Added), lines.map { it.kind })
        assertEquals(2, lines[1].newNo)
    }
}
