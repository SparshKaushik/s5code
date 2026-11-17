package club.touchtech.s5code.kotlin.data

import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ReleaseCheckTest {

    private fun atomEntry(tag: String) =
        "<entry><id>tag:github.com,2008:Repository/123456/$tag</id><title>$tag</title></entry>"

    @Test
    fun `atom feed tags are read newest first`() {
        val feed = "<feed><id>tag:github.com,2008:/SparshKaushik/s5code/releases</id>" +
            atomEntry("v1.2.0-nightly.20261020.3") + atomEntry("v1.1.0") + "</feed>"
        assertEquals(
            listOf("v1.2.0-nightly.20261020.3", "v1.1.0"),
            releaseTagsFromAtom(feed).map { it.tag_name },
        )
    }

    @Test
    fun `stable reads the latest release page without the API`() = runTest {
        val requests = mutableListOf<Pair<String, String>>()
        val update = findEnvironmentUpdate("1.0.0") { url, accept ->
            requests += url to accept
            """{"tag_name":"v1.1.0","name":"ignored"}"""
        }
        assertEquals("1.1.0", update)
        assertEquals(
            listOf("https://github.com/SparshKaushik/s5code/releases/latest" to "application/json"),
            requests,
        )
    }

    @Test
    fun `a current server is up to date`() = runTest {
        assertNull(findEnvironmentUpdate("1.1.0") { _, _ -> """{"tag_name":"v1.1.0"}""" })
    }

    @Test
    fun `nightly picks its channel from the atom feed`() = runTest {
        val update = findEnvironmentUpdate("1.0.0-nightly.20261001.1") { url, accept ->
            assertEquals("https://github.com/SparshKaushik/s5code/releases.atom", url)
            assertEquals("application/atom+xml", accept)
            atomEntry("v1.1.0") + atomEntry("v1.1.0-nightly.20261020.2") + atomEntry("v1.0.0-nightly.20261010.1")
        }
        assertEquals("1.1.0-nightly.20261020.2", update)
    }

    @Test
    fun `a preview displaced from the feed falls back to the API pages`() = runTest {
        val requests = mutableListOf<String>()
        val update = findEnvironmentUpdate("1.0.0-preview.20261001.1") { url, accept ->
            requests += url
            when {
                url.endsWith(".atom") -> atomEntry("v1.1.0-nightly.20261020.2")
                else -> {
                    assertEquals("application/vnd.github+json", accept)
                    """[{"tag_name":"v1.1.0-nightly.20261020.2"},{"tag_name":"v1.1.0-preview.20261015.1","draft":true},
                        {"tag_name":"v1.0.5-preview.20261012.1"}]"""
                }
            }
        }
        assertEquals("1.0.5-preview.20261012.1", update)
        assertEquals(
            listOf(
                "https://github.com/SparshKaushik/s5code/releases.atom",
                "https://api.github.com/repos/SparshKaushik/s5code/releases?per_page=100&page=1",
            ),
            requests,
        )
    }

    @Test
    fun `no release on the channel is an error, not up to date`() = runTest {
        val error = runCatching {
            findEnvironmentUpdate("1.0.0-preview.20261001.1") { url, _ ->
                if (url.endsWith(".atom")) "" else "[]"
            }
        }.exceptionOrNull()
        assertEquals("No preview release was found.", error?.message)
    }
}
