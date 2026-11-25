package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.SourceFile
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.WorkspaceAsset
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Test

class WorkspaceFileCacheTest {
    private val environmentId = EnvironmentId("env-1")
    private val threadId = ThreadId("thread-1")

    @Test
    fun `source cache is scoped and deduplicates concurrent reads`() = runBlocking {
        val cache = WorkspaceFileCache()
        val calls = AtomicInteger()

        val files =
            List(3) {
                async {
                    cache.source(environmentId, threadId, "src/main.kt") {
                        calls.incrementAndGet()
                        kotlinx.coroutines.yield()
                        SourceFile("src/main.kt", "kotlin", listOf("hello"), 5)
                    }
                }
            }.awaitAll()

        assertEquals(1, calls.get())
        assertEquals(3, files.size)
        cache.source(environmentId, ThreadId("thread-2"), "src/main.kt") {
            calls.incrementAndGet()
            SourceFile("src/main.kt", "kotlin", listOf("other"), 5)
        }
        assertEquals(2, calls.get())
    }

    @Test
    fun `source cache evicts least recently used entries`() = runBlocking {
        val cache = WorkspaceFileCache(maxSourceEntries = 1)
        val calls = mutableMapOf<String, Int>()
        suspend fun read(path: String) =
            cache.source(environmentId, threadId, path) {
                calls[path] = (calls[path] ?: 0) + 1
                SourceFile(path, "text", listOf(path))
            }

        read("a.txt")
        read("b.txt")
        read("a.txt")
        assertEquals(2, calls["a.txt"])
    }

    @Test
    fun `asset urls refresh before expiry`() = runBlocking {
        var now = 1_000_000L
        var calls = 0
        val cache = WorkspaceFileCache(now = { now })
        suspend fun asset() =
            cache.asset(environmentId, threadId, "report.html") {
                calls += 1
                WorkspaceAsset("https://example.test/$calls", now + 120_000)
            }

        assertEquals("https://example.test/1", asset().url)
        now += 30_000
        assertEquals("https://example.test/1", asset().url)
        now += 31_000
        assertEquals("https://example.test/2", asset().url)
    }
}
