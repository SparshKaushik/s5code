package club.touchtech.s5code.kotlin.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * Deep-link parsing, which is a trust boundary.
 *
 * Launcher shortcuts persist across app updates and notification payloads come off
 * the network, so a link is untrusted input that must resolve to a declared route
 * or to nothing. The rejections below are the ones that would otherwise let an
 * outside caller assemble a route the graph never declared.
 */
class DeepLinkTest {

    @Test
    fun `known destinations resolve to their routes`() {
        assertEquals(DeepLink.Home, parseDeepLinkPath("/"))
        assertEquals(DeepLink.Home, parseDeepLinkPath(""))
        assertEquals(DeepLink.NewTask, parseDeepLinkPath("/new"))
        assertEquals(DeepLink.Connections, parseDeepLinkPath("environments"))
        assertEquals(DeepLink.Settings, parseDeepLinkPath("/settings"))
        assertEquals(DeepLink.Archive, parseDeepLinkPath("/archive"))
        assertEquals(DeepLink.Usage, parseDeepLinkPath("/usage"))
    }

    @Test
    fun `thread links carry their ids and build the graph's route`() {
        val link = parseDeepLinkPath("/threads/env-1/thread-2")
        assertEquals(DeepLink.Thread("env-1", "thread-2"), link)
        assertEquals("threads/env-1/thread-2", link?.route)
    }

    @Test
    fun `percent-encoded ids are decoded once`() {
        assertEquals(
            "threads/env%201/thread%2F2",
            (parseDeepLinkPath("/threads/env%201/thread%2F2") as DeepLink.Thread).route,
        )
    }

    @Test
    fun `allowlisted thread children resolve and others are refused`() {
        assertEquals(
            DeepLink.Thread("env-1", "thread-2", "terminal"),
            parseDeepLinkPath("/threads/env-1/thread-2/terminal"),
        )
        assertEquals(
            DeepLink.Thread("env-1", "thread-2", "git/commit"),
            parseDeepLinkPath("/threads/env-1/thread-2/git/commit"),
        )
        // Present in the route table, but only meaningful with state the app set
        // up: an external link must not be able to present a confirmation.
        assertNull(parseDeepLinkPath("/threads/env-1/thread-2/git-confirm"))
        // RN registers `review-comment` as a linked route; the composer handles
        // absent params the same way its sheet does.
        assertEquals(
            DeepLink.Thread("env-1", "thread-2", "review-comment"),
            parseDeepLinkPath("/threads/env-1/thread-2/review-comment"),
        )
        // `attachments/<id>` is the one parameterized child: the viewer
        // defaults the missing name/mime/size params rather than refusing.
        assertEquals(
            DeepLink.Thread("env-1", "thread-2", "attachments/file-1"),
            parseDeepLinkPath("/threads/env-1/thread-2/attachments/file-1"),
        )
        // But it is exactly two segments — nothing deeper is a real route.
        assertNull(parseDeepLinkPath("/threads/env-1/thread-2/attachments/file-1/extra"))
        assertNull(parseDeepLinkPath("/threads/env-1/thread-2/attachments"))
    }

    @Test
    fun `malformed and hostile paths resolve to nothing`() {
        assertNull(parseDeepLinkPath("/threads"))
        assertNull(parseDeepLinkPath(" /threads/env/thread"))
        assertNull(parseDeepLinkPath("/threads/env-1"))
        assertNull(parseDeepLinkPath("/../settings"))
        assertNull(parseDeepLinkPath("/threads/../../settings/x"))
        assertNull(parseDeepLinkPath("/unknown"))
        assertNull(parseDeepLinkPath("/threads/%zz/thread"))
        assertNull(parseDeepLinkPath("/threads/${"a".repeat(300)}/thread"))
    }

    @Test
    fun `queries and fragments are ignored rather than making a link invalid`() {
        assertEquals(DeepLink.NewTask, parseDeepLinkPath("/new?from=widget"))
        assertEquals(DeepLink.Home, parseDeepLinkPath("/#token=secret"))
    }

    @Test
    fun `custom scheme uris accept both host and path forms`() {
        assertEquals(DeepLink.NewTask, parseDeepLinkUri("s5code://new"))
        assertEquals(DeepLink.NewTask, parseDeepLinkUri("s5code:///new"))
        assertEquals(
            DeepLink.Thread("env-1", "thread-2"),
            parseDeepLinkUri("s5code://threads/env-1/thread-2"),
        )
    }

    @Test
    fun `other schemes are refused`() {
        assertNull(parseDeepLinkUri("https://app.s5code.touchtech.club/new"))
        assertNull(parseDeepLinkUri("javascript:alert(1)"))
        assertNull(parseDeepLinkUri("s5code-evil://new"))
    }

    @Test
    fun `a share opens the project picker`() {
        // The picker reserves the inbox share once a project is chosen; going
        // straight to the draft would strand "back" on Home instead.
        assertEquals(Routes.NewTask, DeepLink.Share("hello", null, emptyList()).route)
    }
}
