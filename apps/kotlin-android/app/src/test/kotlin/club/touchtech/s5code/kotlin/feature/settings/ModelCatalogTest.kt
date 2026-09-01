package club.touchtech.s5code.kotlin.feature.settings

import club.touchtech.s5code.kotlin.model.ProviderCatalogEntry
import club.touchtech.s5code.kotlin.model.ProviderInstance
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Model search scoping.
 *
 * The scope rule is the part with a real consequence: switching agent mid-thread
 * does not hand the context over, so a thread's search must not surface another
 * agent's models. Existing threads also hide other provider rows entirely; the
 * model-search helper pins the query behavior while TaskSettingsSheet applies the
 * same scope to its instance list.
 */
class ModelCatalogTest {
    private val codex = ProviderInstance(instanceId = "codex", driver = "codex")
    private val pi = ProviderInstance(instanceId = "pi", driver = "pi")
    private val claude = ProviderInstance(instanceId = "claudeAgent", driver = "claudeAgent")

    private val catalog =
        listOf(
            ProviderCatalogEntry(codex, listOf("gpt-5-codex", "gpt-5")),
            ProviderCatalogEntry(pi, listOf("claude-opus-5", "gemini-3-pro")),
            ProviderCatalogEntry(claude, listOf("opus", "sonnet")),
        )

    @Test
    fun `no query shows only the selected agent's models`() {
        val groups = modelSearchResults(catalog, codex, "", ModelSearchScope.AllProviders)
        assertEquals(1, groups.size)
        assertEquals("codex", groups.single().instance.instanceId)
        assertEquals(listOf("gpt-5-codex", "gpt-5"), groups.single().models)
    }

    @Test
    fun `an empty catalog yields nothing rather than an empty group`() {
        assertTrue(modelSearchResults(emptyList(), codex, "", ModelSearchScope.AllProviders).isEmpty())
    }

    @Test
    fun `all-providers scope reaches other agents`() {
        val groups = modelSearchResults(catalog, codex, "opus", ModelSearchScope.AllProviders)
        assertEquals(listOf("pi", "claudeAgent"), groups.map { it.instance.instanceId })
        assertEquals(listOf("claude-opus-5"), groups.first().models)
    }

    @Test
    fun `active-provider scope stays inside the running agent`() {
        val groups = modelSearchResults(catalog, codex, "opus", ModelSearchScope.ActiveProvider)
        assertTrue(groups.isEmpty())
    }

    @Test
    fun `existing thread provider rows are limited to the exact running instance`() {
        val visible =
            catalog
                .map { it.instance }
                .filter { it.instanceId == codex.instanceId }
        assertEquals(listOf(codex), visible)
    }

    @Test
    fun `active-provider scope still filters within that agent`() {
        val groups = modelSearchResults(catalog, codex, "gpt-5-", ModelSearchScope.ActiveProvider)
        assertEquals(listOf("gpt-5-codex"), groups.single().models)
    }

    @Test
    fun `a query matching the agent's own label keeps its whole list`() {
        // "codex" is the provider label as well as part of a model name, and RN
        // matches on both, so narrowing by agent name is not a way to filter models.
        val groups = modelSearchResults(catalog, codex, "codex", ModelSearchScope.ActiveProvider)
        assertEquals(listOf("gpt-5-codex", "gpt-5"), groups.single().models)
    }

    @Test
    fun `the selected agent's group leads even when others match`() {
        val groups = modelSearchResults(catalog, claude, "o", ModelSearchScope.AllProviders)
        assertEquals("claudeAgent", groups.first().instance.instanceId)
    }

    @Test
    fun `matching an agent label returns that agent's whole list`() {
        val groups = modelSearchResults(catalog, codex, "claude", ModelSearchScope.AllProviders)
        // "Claude" matches the instance label, and `claude-opus-5` matches by name
        // under pi, so both groups are legitimate hits.
        assertEquals(listOf("pi", "claudeAgent"), groups.map { it.instance.instanceId })
        assertEquals(listOf("opus", "sonnet"), groups.last().models)
    }

    @Test
    fun `search is case insensitive and trims surrounding space`() {
        val groups = modelSearchResults(catalog, codex, "  GPT-5  ", ModelSearchScope.ActiveProvider)
        assertEquals(listOf("gpt-5-codex", "gpt-5"), groups.single().models)
    }

    @Test
    fun `a query with no matches returns nothing so the caller can say so`() {
        assertTrue(
            modelSearchResults(catalog, codex, "zzzz", ModelSearchScope.AllProviders).isEmpty()
        )
    }

    @Test
    fun `an agent missing from the catalog still searches nothing rather than crashing`() {
        val unknown = ProviderInstance(instanceId = "ghost", driver = "ghost")
        assertTrue(modelSearchResults(catalog, unknown, "", ModelSearchScope.ActiveProvider).isEmpty())
        assertTrue(
            modelSearchResults(catalog, unknown, "opus", ModelSearchScope.ActiveProvider).isEmpty()
        )
        // Widened, the same query does find the other agents.
        assertEquals(
            2,
            modelSearchResults(catalog, unknown, "opus", ModelSearchScope.AllProviders).size,
        )
    }

    @Test
    fun `query matching covers model name and provider label only`() {
        assertTrue(modelMatchesQuery("gpt-5", "Codex", "codex"))
        assertTrue(modelMatchesQuery("gpt-5", "Codex", "GPT"))
        assertTrue(!modelMatchesQuery("gpt-5", "Codex", "claude"))
        assertTrue(modelMatchesQuery("anything", "Anything", ""))
    }
}
