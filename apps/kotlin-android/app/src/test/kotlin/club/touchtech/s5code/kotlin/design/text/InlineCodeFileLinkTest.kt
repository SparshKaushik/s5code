package club.touchtech.s5code.kotlin.design.text

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class InlineCodeFileLinkTest {
    @Test
    fun `inline code naming a file links to the same path`() {
        val positioned = inlineCodeFileLink("src/math.ts:2")!!
        assertEquals("src/math.ts", positioned.path)
        assertEquals("math.ts · L2", positioned.label)
        assertEquals("./README.md", inlineCodeFileLink("./README.md")?.path)
        assertEquals("C:\\repo\\_private\\file.ts", inlineCodeFileLink("C:\\repo\\_private\\file.ts")?.path)
        // A plus in a path is a plus, not a form-encoded space.
        assertEquals("/home/me/c++/main.cpp", inlineCodeFileLink("/home/me/c++/main.cpp")?.path)
    }

    @Test
    fun `commands identifiers and urls stay code`() {
        assertNull(inlineCodeFileLink("vp test run"))
        assertNull(inlineCodeFileLink("useMemo"))
        assertNull(inlineCodeFileLink("https://example.com/a.ts"))
        // A bare file name is only a link with a position, as on desktop.
        assertNull(inlineCodeFileLink("package.json"))
    }
}
