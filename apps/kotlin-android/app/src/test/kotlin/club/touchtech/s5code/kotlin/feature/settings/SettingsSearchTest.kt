package club.touchtech.s5code.kotlin.feature.settings

import org.junit.Assert.assertEquals
import org.junit.Test

/** Cases from RN's settings/settings-search.test.ts. */
class SettingsSearchTest {
    private data class Row(val id: String, val label: String)

    private val sections =
        listOf(
            SettingsSearchSection("Account", listOf(Row("account", "S5 account"), Row("provider-accounts", "Provider accounts"))),
            SettingsSearchSection("App", listOf(Row("environments", "Environments"), Row("appearance", "Appearance"))),
            SettingsSearchSection(
                "Server settings",
                listOf(Row("new-threads", "New threads"), Row("source-control", "Source control")),
            ),
            SettingsSearchSection(null, listOf(Row("usage", "Usage limits"))),
        )

    private fun visible(query: String) =
        filterSettingsSections(sections, query, Row::id, Row::label).map { section ->
            section.title to section.rows.map { it.label }
        }

    @Test
    fun `an empty query keeps every section`() {
        assertEquals(sections.map { it.title to it.rows.map(Row::label) }, visible(""))
        assertEquals(visible(""), visible("   "))
    }

    @Test
    fun `rows match by label, section, and page contents`() {
        assertEquals(listOf("App" to listOf("Appearance")), visible("appear"))
        assertEquals(listOf("App" to listOf("Appearance")), visible("dark mode"))
        assertEquals(listOf("Server settings" to listOf("Source control")), visible("branch naming"))
        assertEquals(listOf("Server settings" to listOf("New threads", "Source control")), visible("server settings"))
        assertEquals(listOf("Account" to listOf("Provider accounts")), visible("PROVIDER"))
        assertEquals(listOf(null to listOf("Usage limits")), visible("usage"))
        assertEquals(emptyList<Pair<String?, List<String>>>(), visible("zzzz"))
    }

    @Test
    fun `sections that start empty are dropped`() {
        val withEmpty = listOf(SettingsSearchSection<Row>("Empty", emptyList())) + sections
        assertEquals(sections.size, filterSettingsSections(withEmpty, "", Row::id, Row::label).size)
    }
}
