package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ThreadOpenPick
import club.touchtech.s5code.kotlin.model.ThreadOpenStats
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Cases from `packages/shared/src/paletteSearch.test.ts`, so the port ranks like RN. */
class PaletteSearchTest {
    private val now = 1_792_713_600_000L // 2026-10-23
    private val day = 24L * 60 * 60 * 1000

    private fun thread(
        title: String,
        activityAt: Long = now - 30 * day,
        opens: ThreadOpenStats? = null,
        branch: String? = null,
        projectTitle: String? = null,
        pullRequests: List<ThreadSearchPullRequest> = emptyList(),
    ) = ThreadSearchSubject(
        title = title,
        branch = branch,
        projectTitle = projectTitle,
        pullRequests = pullRequests,
        activityAtMillis = activityAt,
        opens = opens,
    )

    private fun rankTitles(query: String, threads: List<ThreadSearchSubject>): List<String> {
        val normalized = foldSearchQuery(query)
        return rankSearchItems(threads) { subject, typos -> scoreThreadSearch(normalized, subject, now, typos) }
            .map { it.item.title }
    }

    @Test
    fun `whole words rank over prefixes over newer mid-word matches`() {
        assertEquals(
            listOf("Share auth across worktrees", "Authentication cleanup", "Fix oauth redirect"),
            rankTitles(
                "auth",
                listOf(
                    thread("Fix oauth redirect", activityAt = now),
                    thread("Authentication cleanup"),
                    thread("Share auth across worktrees"),
                ),
            ),
        )
    }

    @Test
    fun `camelCase kebab and digit boundaries are word starts`() {
        fun starts(title: String, query: String) = scoreThreadSearch(query, thread(title), now)?.ranges
        assertEquals(listOf(SearchRange(9, 15)), starts("useThreadSearch hook", "search"))
        assertEquals(listOf(SearchRange(12, 16)), starts("fix/sidebar-perf", "perf"))
        assertEquals(listOf(SearchRange(15, 17)), starts("Upgrade to pnpm12", "12"))
        assertEquals(listOf(SearchRange(8, 11)), starts("Fix JSONSchema parsing", "sch"))
        // Short tokens never match mid-word.
        assertNull(scoreThreadSearch("ci", thread("Decision log"), now))
    }

    @Test
    fun `recent activity and frequent opens decide between equal matches`() {
        assertEquals(
            listOf("Goldbug processes", "Goldbug epilogue"),
            rankTitles(
                "goldbug",
                listOf(
                    thread("Goldbug epilogue", activityAt = now - 20 * day),
                    thread("Goldbug processes", activityAt = now - day),
                ),
            ),
        )
        assertEquals(
            listOf("Goldbug notes", "Goldbug processes"),
            rankTitles(
                "goldbug",
                listOf(
                    thread("Goldbug processes", activityAt = now - day),
                    thread("Goldbug notes", activityAt = now - 40 * day, opens = ThreadOpenStats(now - day, 12.0)),
                ),
            ),
        )
    }

    @Test
    fun `the thread picked for a query leads when it is typed again`() {
        val threads =
            listOf(
                thread(
                    "Release checklist",
                    opens = ThreadOpenStats(now - 5 * day, 1.0, listOf(ThreadOpenPick("rel", now - 5 * day))),
                ),
                thread("Relay pairing", activityAt = now),
            )
        assertEquals(listOf("Release checklist", "Relay pairing"), rankTitles("rel", threads))
        // Two letters are too little to go on.
        assertEquals(listOf("Relay pairing", "Release checklist"), rankTitles("re", threads))
        // Once the query leaves the pick behind, the better match wins again.
        assertEquals(listOf("Relay pairing"), rankTitles("rela", threads))
        // A pick never lifts a match inside a word above a word match.
        assertEquals(
            listOf("Share auth across worktrees", "Fix oauth redirect"),
            rankTitles(
                "auth",
                listOf(
                    thread(
                        "Fix oauth redirect",
                        opens = ThreadOpenStats(now - day, 1.0, listOf(ThreadOpenPick("auth", now - day))),
                    ),
                    thread("Share auth across worktrees"),
                ),
            ),
        )
    }

    @Test
    fun `every token must match somewhere across branch, project, and pull requests`() {
        val subject =
            thread(
                "Trim model lists",
                branch = "t3/trim-mcp-models",
                projectTitle = "t3code",
                pullRequests = listOf(ThreadSearchPullRequest(22889, "Smaller MCP payloads", emptyList())),
            )
        for (query in listOf("#22889", "22889", "trim mcp", "payloads t3code", "models trim")) {
            assertNotNull(query, scoreThreadSearch(foldSearchQuery(query), subject, now))
        }
        assertNull(scoreThreadSearch("trim lakebed", subject, now))
        // A branch pasted with the worktree prefix still finds it.
        assertNotNull(scoreThreadSearch(foldSearchQuery("t3/trim-mcp"), subject, now))
    }

    @Test
    fun `one typo is forgiven only when nothing matches exactly`() {
        val threads = listOf(thread("Hosting alternatives to Railway"), thread("Trim MCP model lists"))
        assertEquals(listOf("Hosting alternatives to Railway"), rankTitles("railwya", threads))
        assertEquals(listOf("Trim MCP model lists"), rankTitles("trim mpc", threads))
        assertEquals(
            listOf("De-risk idle tunnel cleanup", "Typesafe App Builder"),
            rankTitles("ilde", listOf(thread("Typesafe App Builder"), thread("De-risk idle tunnel cleanup"))),
        )
        assertEquals(emptyList<String>(), rankTitles("ing", listOf(thread("Nightly build fix"))))
        assertEquals(listOf("Fix Sidebar Lag"), rankTitles("fix the sidebar", listOf(thread("Fix Sidebar Lag"))))
        assertEquals(emptyList<String>(), rankTitles("the", listOf(thread("Fix sidebar lag"))))
        assertEquals(
            listOf("Fix Cmd+Enter"),
            rankTitles("cmd enter", listOf(thread("Fix Cmd+Enter"), thread("Open links external"))),
        )
    }

    @Test
    fun `queries that fold to nothing match nothing`() {
        assertEquals(listOf("Fix sidebar lag"), rankTitles("# fix", listOf(thread("Fix sidebar lag"))))
        assertEquals(emptyList<String>(), rankTitles("#", listOf(thread("Fix sidebar lag"))))
        assertNull(scoreSearchTerms("# fix", listOf("Fix sidebar")))
    }

    private data class Message(val key: String, val quality: Double, val hits: Int? = null)

    private fun rankMessages(query: String, matches: List<Message>, subjects: Map<String, ThreadSearchSubject>) =
        rankMessageMatches(
            matches,
            quality = { it.quality },
            hits = { it.hits },
            messageCreatedAt = { null },
            subjectOf = { subjects[it.key] },
            normalizedQuery = query,
            now = now,
        )
            .map { it.key }

    @Test
    fun `message matches rank by word match, then picks, then quality and use`() {
        val subjects =
            mapOf(
                "a-word" to thread("A word"),
                "a-word-2" to thread("A word too"),
                "b-prefix" to thread("B prefix"),
                "b-recent" to thread("B recent", activityAt = now),
                "b-picked" to
                    thread("B picked", opens = ThreadOpenStats(now - day, 1.0, listOf(ThreadOpenPick("needle", now - day)))),
            )
        val matches =
            listOf(
                Message("a-word", 1.0),
                Message("a-word-2", 1.0),
                Message("b-prefix", 0.8),
                Message("b-recent", 1.0),
                Message("b-picked", 0.8),
            )
        assertEquals(listOf("b-picked", "b-recent", "a-word", "a-word-2", "b-prefix"), rankMessages("needle", matches, subjects))
    }

    @Test
    fun `a thread that keeps using the word beats a passing mention, and words beat mid-word hits`() {
        val subjects =
            mapOf(
                "mention" to thread("Mention", activityAt = now - 3 * day, opens = ThreadOpenStats(now, 1.0)),
                "about" to thread("About it", activityAt = now - 48 * day),
            )
        assertEquals(
            listOf("about", "mention"),
            rankMessages("tailscale", listOf(Message("mention", 1.0, 1), Message("about", 1.0, 21)), subjects),
        )
        val busy =
            mapOf(
                "busy" to thread("Busy", activityAt = now, opens = ThreadOpenStats(now, 20.0)),
                "quiet" to thread("Quiet"),
            )
        assertEquals(listOf("quiet", "busy"), rankMessages("rust", listOf(Message("busy", 0.3), Message("quiet", 1.0)), busy))
    }

    @Test
    fun `message search needs a word the server can search`() {
        assertTrue(messageSearchTokens("ab cd").long.isEmpty())
        assertEquals(listOf("sidebar"), messageSearchTokens("fix sidebar").long.filter { it == "sidebar" })
        assertEquals(listOf("中文"), messageSearchTokens("中文").long)
    }

    @Test
    fun `picked queries are folded and cut to the stored length`() {
        assertEquals("fix sidebar", pickedQuery("  Fix   Sidebar "))
        assertEquals(32, pickedQuery("a".repeat(40)).length)
    }

    @Test
    fun `settings terms rank the visible name above hidden keywords`() {
        val byName = scoreSearchTerms("notifications", listOf("Notifications", "push", "alerts"))
        val byKeyword = scoreSearchTerms("alerts", listOf("Notifications", "push", "alerts"))
        assertNotNull(byName)
        assertNotNull(byKeyword)
        assertTrue(byName!!.groupQuality > byKeyword!!.groupQuality)
        assertNull(scoreSearchTerms("billing", listOf("Notifications", "push")))
    }
}
