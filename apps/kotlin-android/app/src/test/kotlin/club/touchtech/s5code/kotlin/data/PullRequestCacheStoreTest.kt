package club.touchtech.s5code.kotlin.data

import java.nio.file.Files
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PullRequestCacheStoreTest {
    @Test
    fun `a read and the list controls survive a new store instance, kept apart per environment`() = runTest {
        val root = Files.createTempDirectory("pull-request-cache").toFile()
        try {
            val value = buildJsonObject { put("number", 7) }
            val state =
                PullRequestListState(
                    environmentId = "env/a",
                    controls = mapOf("env/a" to DEFAULT_PR_LIST_CONTROLS.copy(state = "merged")),
                    environments = setOf("env/a"),
                )
            PullRequestCacheStore.forRoot(root).apply {
                write("env/a", "detail:{\"number\":7}", 1_000, value)
                writeListState(state)
            }

            val restored = PullRequestCacheStore.forRoot(root)
            assertEquals(PullRequestCacheStore.Entry(1_000, value), restored.read("env/a", "detail:{\"number\":7}"))
            assertNull(restored.read("env/b", "detail:{\"number\":7}"))
            assertEquals(state, restored.readListState())

            restored.write("env/a", "detail:{\"number\":7}", 2_000, JsonPrimitive("newer"))
            assertEquals(JsonPrimitive("newer"), restored.read("env/a", "detail:{\"number\":7}")?.value)
        } finally {
            root.deleteRecursively()
        }
    }
}
