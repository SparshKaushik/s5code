package club.touchtech.s5code.kotlin.feature.newtask

import org.junit.Assert.assertEquals
import org.junit.Test

/** The Add project path field survives its host dropping out and returning. */
class BrowsePathInputsTest {

    @Test
    fun `a host dropping out and returning keeps its typed path`() {
        val typed = BrowsePathInputs(last = "~/code/").commit("mac", "~/work/app")
        // The host is gone for a moment: the field keeps the last path.
        assertEquals("~/work/app", typed.pathFor(null, "~/"))
        assertEquals("~/work/app", typed.pathFor("mac", "~/code/"))
    }

    @Test
    fun `each host shows its own default until something is typed for it`() {
        val typed = BrowsePathInputs(last = "~/code/").commit("mac", "~/work/app")
        assertEquals("/srv/", typed.pathFor("server", "/srv/"))
        val both = typed.commit("server", "/srv/app")
        assertEquals("~/work/app", both.pathFor("mac", "~/code/"))
        assertEquals("/srv/app", both.pathFor("server", "/srv/"))
        // Typing with no host updates only the shared fallback.
        val orphan = both.commit(null, "/tmp/x")
        assertEquals("/tmp/x", orphan.pathFor(null, "~/"))
        assertEquals("~/work/app", orphan.pathFor("mac", "~/code/"))
    }
}
