package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.ThreadSummary

enum class ProjectRemovalTarget { Project, Checkout }

data class ProjectRemovalPlan(
    val target: ProjectRemovalTarget,
    val threadCount: Int,
    val confirmationMessage: String,
)

/**
 * client-runtime's `planProjectRemoval`. Settings → Project overview always
 * shows the whole group, so removing fewer than [groupMemberCount] members is
 * a checkout removal and removing all of them is a project removal.
 */
fun planProjectRemoval(
    members: List<Project>,
    groupMemberCount: Int,
    displayName: String,
    threads: List<ThreadSummary>,
    environmentLabel: (EnvironmentId) -> String?,
): ProjectRemovalPlan {
    val memberKeys = members.map { it.environmentId to it.id }.toSet()
    val threadCount = threads.count { (it.environmentId to it.projectId) in memberKeys }
    val isWholeGroup = members.size == groupMemberCount
    val target = if (isWholeGroup) ProjectRemovalTarget.Project else ProjectRemovalTarget.Checkout
    val kind = if (target == ProjectRemovalTarget.Project) "project" else "checkout"
    val single = members.singleOrNull()
    val label = single?.title ?: displayName
    val message =
        buildList {
            add(
                if (threadCount > 0) {
                    "Remove $kind \"$label\" and delete its $threadCount thread${if (threadCount == 1) "" else "s"}?"
                } else {
                    "Remove $kind \"$label\"?"
                }
            )
            if (single != null) {
                add("Path: ${single.workspaceRoot}")
                environmentLabel(single.environmentId)?.let { add("Environment: $it") }
            } else {
                add("This removes ${members.size} grouped project entries.")
            }
            add(
                if (threadCount > 0) {
                    "This permanently clears conversation history for those threads and any archived threads."
                } else {
                    "This permanently clears any archived conversation history."
                }
            )
            add(
                if (isWholeGroup) "This removes only the project entries, not the files on disk."
                else "Other entries in this grouped project are unaffected."
            )
            add("This action cannot be undone.")
        }.joinToString("\n")
    return ProjectRemovalPlan(target, threadCount, message)
}
