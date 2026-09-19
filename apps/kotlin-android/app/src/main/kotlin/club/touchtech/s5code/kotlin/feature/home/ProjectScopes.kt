package club.touchtech.s5code.kotlin.feature.home

import club.touchtech.s5code.kotlin.data.normalizeProjectPathForComparison
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.ProjectGrouping
import club.touchtech.s5code.kotlin.model.ThreadSort
import club.touchtech.s5code.kotlin.model.ThreadSummary

/**
 * The logical project groups the home list and the new-task picker render.
 *
 * A direct port of `buildProjectGroups` in `packages/client-runtime/state/
 * projectGrouping.ts` and `buildHomeProjectScopes`/`sortHomeProjectScopes` in
 * `apps/mobile/src/features/home/homeThreadList.ts`: physical projects are
 * deduplicated per workspace root, then merged into one scope per repository
 * (or repository path, or nothing) so the same repo on two machines is one
 * row, not two.
 */

/** One selectable row in the project picker: a logical project across machines. */
data class ProjectScope(
    val key: String,
    val title: String,
    val representative: Project,
    val projects: List<Project>,
)

private fun uniqueNonEmptyValues(values: List<String?>): List<String> {
    val seen = mutableSetOf<String>()
    val unique = mutableListOf<String>()
    for (value in values) {
        val trimmed = value?.trim() ?: continue
        if (seen.add(trimmed)) unique += trimmed
    }
    return unique
}

/** `deriveRepositoryRelativeProjectPath`: where inside the repo this workspace sits. */
private fun repositoryRelativePath(project: Project): String? {
    val rootPath = project.repositoryIdentity?.rootPath?.trim() ?: return null
    val normalizedProject = normalizeProjectPathForComparison(project.workspaceRoot)
    val normalizedRoot = normalizeProjectPathForComparison(rootPath)
    if (normalizedProject.isEmpty() || normalizedRoot.isEmpty()) return null
    if (normalizedProject == normalizedRoot) return ""
    val separator = if (normalizedRoot.contains('\\')) '\\' else '/'
    val rootPrefix = "$normalizedRoot$separator"
    if (!normalizedProject.startsWith(rootPrefix)) return null
    return normalizedProject.substring(rootPrefix.length).replace('\\', '/')
}

/** `derivePhysicalProjectKey`: one workspace root on one machine. */
private fun physicalProjectKey(project: Project): String =
    "${project.environmentId.value}:${normalizeProjectPathForComparison(project.workspaceRoot)}"

private fun repositoryScopedKey(project: Project, mode: ProjectGrouping): String? {
    val canonicalKey = project.repositoryIdentity?.canonicalKey ?: return null
    if (mode == ProjectGrouping.Repository) return canonicalKey
    val relativePath = repositoryRelativePath(project) ?: return canonicalKey
    return if (relativePath.isEmpty()) canonicalKey else "$canonicalKey::$relativePath"
}

private fun logicalProjectKey(project: Project, mode: ProjectGrouping): String {
    if (mode == ProjectGrouping.Separate) return physicalProjectKey(project)
    return repositoryScopedKey(project, mode)
        ?: physicalProjectKey(project)
        .ifEmpty { "${project.environmentId.value}:${project.id.value}" }
}

/** `deriveProjectGroupLabel`: a shared repo name beats the representative's title. */
private fun projectGroupLabel(representative: Project, members: List<Project>): String {
    val displayNames = uniqueNonEmptyValues(members.map { it.repositoryIdentity?.displayName })
    if (displayNames.size == 1) return displayNames[0]
    val repoNames = uniqueNonEmptyValues(members.map { it.repositoryIdentity?.name })
    if (repoNames.size == 1) return repoNames[0]
    return representative.title
}

private fun projectFreshnessTime(project: Project): Long =
    project.updatedAtMillis ?: project.createdAtMillis ?: 0L

private fun shouldReplaceWinner(existing: Project, candidate: Project): Boolean {
    val delta = projectFreshnessTime(candidate) - projectFreshnessTime(existing)
    return delta > 0 || (delta == 0L && candidate.id.value > existing.id.value)
}

/** The freshest member with a repository identity supplies the scope's key. */
private fun selectIdentitySource(projects: List<Project>, winner: Project): Project {
    if (winner.repositoryIdentity != null) return winner
    var freshest: Project? = null
    for (project in projects) {
        if (project.repositoryIdentity == null) continue
        if (freshest == null || shouldReplaceWinner(freshest!!, project)) freshest = project
    }
    return freshest ?: winner
}

/**
 * `buildProjectGroups`: physical dedup, then logical merge on the mode.
 * Member order follows input order; the representative is the first member.
 */
fun buildProjectScopes(
    projects: List<Project>,
    grouping: ProjectGrouping,
): List<ProjectScope> {
    val byPhysicalKey = LinkedHashMap<String, MutableList<Project>>()
    for (project in projects) {
        byPhysicalKey.getOrPut(physicalProjectKey(project)) { mutableListOf() }.add(project)
    }
    val grouped = LinkedHashMap<String, MutableList<Project>>()
    for ((_, physicalProjects) in byPhysicalKey) {
        val winner = physicalProjects.reduce { current, candidate ->
            if (shouldReplaceWinner(current, candidate)) candidate else current
        }
        val identitySource = selectIdentitySource(physicalProjects, winner)
        grouped.getOrPut(logicalProjectKey(identitySource, grouping)) { mutableListOf() }
            .add(winner)
    }
    return grouped.map { (key, members) ->
        val representative = members.first()
        ProjectScope(
            key = key,
            title =
                if (members.size > 1) projectGroupLabel(representative, members)
                else representative.title,
            representative = representative,
            projects = members,
        )
    }
}

/** `getProjectScopeSelectionTarget`: the member on the draft's environment wins. */
fun projectScopeSelectionTarget(scope: ProjectScope, environmentId: String?): Project =
    scope.projects.firstOrNull { it.environmentId.value == environmentId } ?: scope.representative

private fun projectSortTimestamp(project: Project, order: ThreadSort): Long =
    if (order == ThreadSort.Created) {
        project.createdAtMillis ?: Long.MIN_VALUE
    } else {
        project.updatedAtMillis ?: project.createdAtMillis ?: Long.MIN_VALUE
    }

private fun threadSortTimestamp(thread: ThreadSummary, order: ThreadSort): Long =
    if (order == ThreadSort.Created) thread.createdAtMillis else thread.updatedAtMillis

/**
 * `sortHomeProjectScopes`: latest thread activity wins, then the freshest
 * member, then title and key so the list is stable between refreshes.
 * Archived threads don't count — Kotlin's `workspace.threads` is already the
 * active-only projection, so every thread here does.
 */
fun sortProjectScopes(
    scopes: List<ProjectScope>,
    threads: List<ThreadSummary>,
    order: ThreadSort,
): List<ProjectScope> {
    val scopeKeyByRef = mutableMapOf<Pair<String, String>, String>()
    for (scope in scopes) {
        for (project in scope.projects) {
            scopeKeyByRef[project.environmentId.value to project.id.value] = scope.key
        }
    }
    val latestByScope = mutableMapOf<String, Long>()
    for (thread in threads) {
        val key = scopeKeyByRef[thread.environmentId.value to thread.projectId.value] ?: continue
        val timestamp = threadSortTimestamp(thread, order)
        latestByScope.merge(key, timestamp, ::maxOf)
    }
    return scopes.sortedWith(
        compareByDescending<ProjectScope> { scope ->
            latestByScope[scope.key]
                ?: scope.projects.maxOfOrNull { projectSortTimestamp(it, order) }
                ?: Long.MIN_VALUE
        }.thenBy { it.title }.thenBy { it.key }
    )
}
