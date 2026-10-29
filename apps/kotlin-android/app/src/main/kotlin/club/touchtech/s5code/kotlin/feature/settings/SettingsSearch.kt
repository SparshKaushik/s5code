package club.touchtech.s5code.kotlin.feature.settings

import club.touchtech.s5code.kotlin.data.foldSearchQuery
import club.touchtech.s5code.kotlin.data.rankSearchItems
import club.touchtech.s5code.kotlin.data.scoreSearchTerms

/**
 * What each Settings page holds besides its row's label, so search finds a
 * setting by its own name. `SETTINGS_SEARCH_TERMS` in RN's
 * settings/settings-search.ts, keyed by this client's row ids; RN's About page
 * is split here into Client storage and Diagnostics.
 */
private val SETTINGS_SEARCH_TERMS: Map<String, List<String>> =
    mapOf(
        "account" to listOf("sign in", "sign out", "login", "connect"),
        "environments" to listOf("servers", "connections", "pair", "add environment", "remote", "tailscale"),
        "notifications" to listOf("push", "alerts", "live activity", "agent activity"),
        "appearance" to
            listOf(
                "theme",
                "dark mode",
                "light mode",
                "colors",
                "font size",
                "text size",
                "code",
                "diffs",
                "terminal",
                "word break",
                "math",
            ),
        "scheduled-tasks" to listOf("automations", "schedule", "cron", "recurring", "webhook"),
        "overview" to listOf("project", "checkouts", "worktrees", "delete project"),
        "organization" to listOf("project grouping", "group projects", "sidebar", "new threads", "last project"),
        "threads" to
            listOf("auto-settle", "settle", "snooze", "usage limits", "auto-resume", "plan mode", "working section"),
        "follow-ups" to listOf("queue", "steer", "while the agent is running"),
        "archive" to listOf("archive", "archived", "restore", "unarchive"),
        "provider-accounts" to listOf("codex", "claude", "cursor", "opencode", "sign in", "login", "auth"),
        "new-threads" to listOf("default workspace", "worktree", "submodules", "default permissions", "access"),
        "source-control" to
            listOf(
                "git",
                "branch naming",
                "pull requests",
                "co-authors",
                "agent credits",
                "default branch",
                "automatically pull",
                "worktrees",
                "start from remote",
            ),
        "agent-behavior" to listOf("response streaming", "preview browser", "browser access"),
        "maintenance" to listOf("updates", "provider updates", "restart", "manage environments"),
        "client-storage" to listOf("cache"),
        "diagnostics" to listOf("version", "legal", "privacy", "terms", "open source licenses"),
    )

data class SettingsSearchSection<R>(val title: String?, val rows: List<R>)

/**
 * `filterSettingsSections`: keeps the rows whose label, section, or page
 * contents match [query], in list order, and drops sections left empty.
 * Matching is the home search's, so both searches agree.
 */
fun <R> filterSettingsSections(
    sections: List<SettingsSearchSection<R>>,
    query: String,
    id: (R) -> String,
    label: (R) -> String,
): List<SettingsSearchSection<R>> {
    val normalized = foldSearchQuery(query)
    if (normalized.isEmpty()) return sections.filter { it.rows.isNotEmpty() }
    val rows = sections.flatMap { section -> section.rows.map { row -> row to section.title } }
    val matched =
        rankSearchItems(rows) { (row, section), typos ->
                scoreSearchTerms(
                    normalized,
                    listOf(label(row)) + listOfNotNull(section) + SETTINGS_SEARCH_TERMS[id(row)].orEmpty(),
                    typos,
                )
            }
            .mapTo(HashSet()) { it.item.first }
    return sections.mapNotNull { section ->
        section.copy(rows = section.rows.filter { it in matched }).takeIf { it.rows.isNotEmpty() }
    }
}
